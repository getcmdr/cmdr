//! Dragging out of Cmdr: the Shell's data object for the items, then `SHDoDragDrop` on the UI
//! thread. Every way a drag ends, started or not, is told to the frontend (`drag-out-ended`), or
//! its self-drag state would outlive the drag and a later drop from Explorer would move the old
//! selection.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::SHDoDragDrop;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};

use super::super::locality::SessionLocality;
use super::data_object::{OwnedPidl, empty_data_object, files_data_object, fits_cf_hdrop, mark_as_cmdr_drag};
use super::effects::{OFFERED_EFFECTS, clear_intent};
use crate::system_events::{DragOutEnded, SessionCompleteEvent};

/// A thread that asks the Shell for the dragged items' ID lists. The Shell loads third-party
/// extensions into the caller, so it gets the 8 MB stack a Shell call does elsewhere.
const ITEMS_THREAD_STACK: usize = 8 * 1024 * 1024;

/// The newest drag asked for. A drag still building its items when a newer one starts gives way,
/// so two modal drags can never queue up.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// How a drag out ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DragOutEnd {
    /// The drag ran; the target did whatever it does.
    Finished,
    /// A newer drag started meanwhile. It owns the frontend's self-drag state, so this one says
    /// nothing.
    Superseded,
    /// The mouse button came up before the drag could start: starting it then would drop at once,
    /// wherever the cursor is.
    ButtonUp,
    /// Items other apps can't be handed (leaf names): the Shell has no name for them, or their
    /// path is past `MAX_PATH`, which `CF_HDROP` can't carry. The drag is refused whole rather
    /// than quietly leaving them out.
    Refused(Vec<String>),
    /// No apartment, window, or data object: logged where it happened.
    Broken,
}

impl DragOutEnd {
    /// Whether the frontend hears of it. Every end does but a superseded one.
    pub(super) fn tells_frontend(&self) -> bool {
        !matches!(self, Self::Superseded)
    }
}

/// Starts a drag of `natives` (empty for a virtual session). Returns at once; the ID lists are
/// built on a thread of their own, then the drag runs on the UI thread.
pub(super) fn start(app: AppHandle, natives: Vec<PathBuf>, locality: SessionLocality) -> Result<(), String> {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::Builder::new()
        .name("drag-out-items".to_string())
        .stack_size(ITEMS_THREAD_STACK)
        .spawn(move || {
            let built = crate::platform::shell::in_apartment(|| item_lists(&natives, || still_wanted(generation)));
            let items = match built {
                Ok(Ok(items)) => items,
                Ok(Err(end)) => return report(&app, end),
                Err(hresult) => {
                    log::warn!(target: "native_drag", "no COM apartment for the drag's items: {hresult:#x}");
                    return report(&app, DragOutEnd::Broken);
                }
            };
            let main = app.clone();
            let dispatched = app.run_on_main_thread(move || {
                let end = run_drag(&main, items, locality, generation);
                report(&main, end);
            });
            if let Err(e) = dispatched {
                log::warn!(target: "native_drag", "couldn't reach the UI thread to start the drag: {e}");
                report(&app, DragOutEnd::Broken);
            }
        })
        .map(|_thread| ())
        .map_err(|e| format!("Couldn't start the drag: {e}"))
}

/// Why the drag of `generation` should stop now, if it should.
fn still_wanted(generation: u64) -> Option<DragOutEnd> {
    if GENERATION.load(Ordering::SeqCst) != generation {
        Some(DragOutEnd::Superseded)
    } else if !primary_button_down() {
        Some(DragOutEnd::ButtonUp)
    } else {
        None
    }
}

/// The ID list for each of `natives`, asking `stop` before each lookup (a lookup can wait on a
/// share, so a released button or a newer drag ends the work early). An item past `MAX_PATH` or
/// one the Shell can't name refuses the drag, naming every such item.
pub(super) fn item_lists(
    natives: &[PathBuf],
    stop: impl Fn() -> Option<DragOutEnd>,
) -> Result<Vec<OwnedPidl>, DragOutEnd> {
    let mut items = Vec::with_capacity(natives.len());
    let mut refused = Vec::new();
    for native in natives {
        if let Some(end) = stop() {
            return Err(end);
        }
        let item = fits_cf_hdrop(native).then(|| OwnedPidl::for_path(native)).flatten();
        match item {
            Some(item) => items.push(item),
            None => {
                log::info!(
                    target: "native_drag",
                    "a dragged item can't be handed to other apps (past MAX_PATH, or no Shell name): path={:?}",
                    cmdr_fs::log_private::LogPrivate::path(native)
                );
                let leaf = native.file_name().unwrap_or(native.as_os_str());
                refused.push(leaf.to_string_lossy().into_owned());
            }
        }
    }
    if refused.is_empty() {
        Ok(items)
    } else {
        Err(DragOutEnd::Refused(refused))
    }
}

/// Runs the drag on the UI thread. `SHDoDragDrop` is modal: it pumps the thread's messages until
/// the drop, so the window keeps painting and answering IPC (wry completes requests through its
/// window's messages), while events other threads post through Tauri's event loop wait for the
/// drop. Nothing here touches the disk; the Shell's data object answers from the ID lists.
fn run_drag(app: &AppHandle, items: Vec<OwnedPidl>, locality: SessionLocality, generation: u64) -> DragOutEnd {
    if let Some(end) = still_wanted(generation) {
        return end;
    }
    let Some(window) = app.get_webview_window("main") else {
        log::warn!(target: "native_drag", "no main window to drag from");
        return DragOutEnd::Broken;
    };
    let hwnd = match window.hwnd() {
        Ok(hwnd) => HWND(hwnd.0),
        Err(e) => {
            log::warn!(target: "native_drag", "no window handle to drag from: {e}");
            return DragOutEnd::Broken;
        }
    };
    let data = match locality {
        SessionLocality::Local => files_data_object(&items),
        SessionLocality::Virtual => empty_data_object(),
    };
    drop(items);
    let data = match data.and_then(|data| mark_as_cmdr_drag(&data).map(|()| data)) {
        Ok(data) => data,
        Err(e) => {
            log::warn!(target: "native_drag", "couldn't build the drag's data object: {e}");
            return DragOutEnd::Broken;
        }
    };
    // Asked again: building the object took a moment, and a drag started with the button up
    // drops at once.
    if let Some(end) = still_wanted(generation) {
        return end;
    }
    // SAFETY: on the UI thread, which tao put in an OLE apartment; `data` lives through the call,
    // and a null drop source asks the Shell for its standard one (Esc cancels, release drops).
    let outcome = unsafe { SHDoDragDrop(Some(hwnd), &data, None, OFFERED_EFFECTS) };
    clear_intent();
    match outcome {
        // Explorer does the copy or move itself; a target that leaves a move's delete to its
        // source gets a copy, since Cmdr deletes nothing here.
        Ok(effect) => log::debug!(target: "native_drag", "drag out ended: effect {:#x}", effect.0),
        Err(e) => log::warn!(target: "native_drag", "drag out failed: {e}"),
    }
    DragOutEnd::Finished
}

/// Tells the frontend how the drag ended: `drag-out-ended` for every end it should hear of, and
/// for items the Shell couldn't name, the drag-out failure toast that names them
/// (`drag-out-session-complete` with nothing copied).
fn report(app: &AppHandle, end: DragOutEnd) {
    if !end.tells_frontend() {
        return;
    }
    if let DragOutEnd::Refused(names) = &end {
        let toast = SessionCompleteEvent {
            session_key: -1,
            files_succeeded: 0,
            folders_succeeded: 0,
            failures: names.clone(),
        };
        if let Err(e) = toast.emit_to(app, "main") {
            log::warn!(target: "native_drag", "couldn't send the refused drag's toast: {e}");
        }
    }
    let ended = DragOutEnded {};
    if let Err(e) = ended.emit_to(app, "main") {
        log::warn!(target: "native_drag", "couldn't tell the frontend the drag ended: {e}");
    }
}

/// Whether the primary mouse button is down right now (the right one when the buttons are
/// swapped). Global state, so any thread may ask.
fn primary_button_down() -> bool {
    // SAFETY: plain reads of system state.
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        GetAsyncKeyState(i32::from(key.0)) < 0
    }
}
