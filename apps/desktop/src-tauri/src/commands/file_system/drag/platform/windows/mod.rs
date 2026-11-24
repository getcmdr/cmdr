//! Windows: drags out through the Shell's `SHDoDragDrop` with the Shell's own data object for the
//! files (`data_object.rs`), and drags in through Cmdr's OLE drop target (`drop_target.rs`), which
//! replaces wry's on every webview. Rules: `effects.rs`. Decisions and the UI-thread budget:
//! `apps/desktop/src/lib/file-explorer/drag/DETAILS.md` § Windows.

mod data_object;
mod drop_target;
mod effects;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::rc::Rc;

use tauri::{AppHandle, Manager, Webview, Wry};
use tauri_specta::Event;
use windows::Win32::Foundation::{HWND, LPARAM, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Ole::{RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::SHDoDragDrop;
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetSystemMetrics, SM_SWAPBUTTON};
use windows::core::BOOL;

use super::locality::{SessionLocality, session_locality};
use crate::system_events::{DragModifiers, DragPoint, NativeDragDrop};
use data_object::{OwnedPidl, empty_data_object, files_data_object, mark_as_cmdr_drag};
use drop_target::{Sink, TargetEvent, drop_target_for};
use effects::{DropIntent, OFFERED_EFFECTS, clear_intent, set_intent};

/// A thread that asks the Shell for the dragged items' ID lists. The Shell loads third-party
/// extensions into the caller, so it gets the 8 MB stack a Shell call does elsewhere.
const ITEMS_THREAD_STACK: usize = 8 * 1024 * 1024;

/// Starts a drag of `paths` out of the main window. Returns at once, as the macOS arm does (the
/// frontend expects `startDragPaths` to resolve before the drag's events): the ID lists are built
/// on a thread of their own, then the drag runs on the UI thread. `icon_path` goes unused: the
/// Shell draws Explorer's own drag image for the items, thumbnails and count included.
pub(crate) fn start_drag(
    app: AppHandle,
    paths: Vec<String>,
    _icon_path: String,
    source_volume_id: Option<String>,
) -> Result<(), String> {
    let sent: Vec<PathBuf> = paths
        .iter()
        .map(|row| crate::commands::wire_path::as_sent_std_path(row))
        .collect();
    if sent.is_empty() {
        return Err("No valid files to drag".to_string());
    }
    let locality = session_locality(&sent, source_volume_id.as_deref());
    // A virtual session offers other apps nothing (task 43 will offer the bytes); a drop on Cmdr
    // still works, through the frontend's recorded self-drag identity.
    let natives: Vec<PathBuf> = match locality {
        SessionLocality::Local => paths.iter().filter_map(|row| crate::app_text::native(row)).collect(),
        SessionLocality::Virtual => Vec::new(),
    };
    if locality == SessionLocality::Local && natives.is_empty() {
        return Err("No valid files to drag".to_string());
    }
    std::thread::Builder::new()
        .name("drag-out-items".to_string())
        .stack_size(ITEMS_THREAD_STACK)
        .spawn(move || {
            let items = match in_apartment(|| item_lists(&natives)) {
                Ok(Some(items)) => items,
                Ok(None) => return,
                Err(hresult) => {
                    log::warn!(target: "native_drag", "no COM apartment for the drag's items: {hresult:#x}");
                    return;
                }
            };
            let main = app.clone();
            let dispatched = app.run_on_main_thread(move || run_drag(&main, items, locality));
            if let Err(e) = dispatched {
                log::warn!(target: "native_drag", "couldn't reach the UI thread to start the drag: {e}");
            }
        })
        .map(|_thread| ())
        .map_err(|e| format!("Couldn't start the drag: {e}"))
}

/// The ID list for each of `natives`, or `None` (logged) when one has none: a drag never quietly
/// leaves a file out.
fn item_lists(natives: &[PathBuf]) -> Option<Vec<OwnedPidl>> {
    let mut items = Vec::with_capacity(natives.len());
    for native in natives {
        let Some(item) = OwnedPidl::for_path(native) else {
            log::info!(
                target: "native_drag",
                "the Shell can't name a dragged item, so the drag didn't start: path={:?}",
                cmdr_fs::log_private::LogPrivate::path(native)
            );
            return None;
        };
        items.push(item);
    }
    Some(items)
}

/// Runs the drag on the UI thread. `SHDoDragDrop` is modal: it pumps the thread's messages until
/// the drop, so the window keeps painting and answering IPC (wry completes requests through its
/// window's messages), while events other threads post through Tauri's event loop wait for the
/// drop. Nothing here touches the disk; the Shell's data object answers from the ID lists.
fn run_drag(app: &AppHandle, items: Vec<OwnedPidl>, locality: SessionLocality) {
    let Some(window) = app.get_webview_window("main") else {
        log::warn!(target: "native_drag", "no main window to drag from");
        return;
    };
    let hwnd = match window.hwnd() {
        Ok(hwnd) => HWND(hwnd.0),
        Err(e) => {
            log::warn!(target: "native_drag", "no window handle to drag from: {e}");
            return;
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
            return;
        }
    };
    // The ID lists took a thread hop: if the button came up meanwhile, a drag started now would
    // drop at once wherever the cursor is. Don't.
    if !primary_button_down() {
        log::info!(target: "native_drag", "the mouse button came up before the drag could start");
        return;
    }
    // SAFETY: on the UI thread, which tao put in an OLE apartment; `data` lives through the call,
    // and a null drop source asks the Shell for its standard one (Esc cancels, release drops).
    let outcome = unsafe { SHDoDragDrop(Some(hwnd), &data, None, OFFERED_EFFECTS) };
    clear_intent();
    match outcome {
        // The target did the copy or move itself (Explorer's copy engine); Cmdr deletes nothing.
        Ok(effect) => log::debug!(target: "native_drag", "drag out ended: effect {:#x}", effect.0),
        Err(e) => log::warn!(target: "native_drag", "drag out failed: {e}"),
    }
}

/// Whether the primary mouse button is down right now (the right one when the buttons are
/// swapped).
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

/// Runs `call` inside a single-threaded apartment entered and left on this (fresh) thread.
///
/// # Errors
///
/// The `CoInitializeEx` `HRESULT` when the apartment can't be entered; `call` didn't run.
// TODO: switch to `crate::platform::shell::in_apartment` once PR #1450 lands; this is its twin.
fn in_apartment<T>(call: impl FnOnce() -> T) -> Result<T, i32> {
    /// Leaves the apartment on drop, so a panicking call leaves it too.
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: built only after a successful `CoInitializeEx` on this thread, never sent.
            unsafe { CoUninitialize() };
        }
    }
    // SAFETY: no reserved pointer; a success is balanced by `Apartment`'s drop on this thread.
    let entered = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    if entered.is_err() || entered == RPC_E_CHANGED_MODE {
        return Err(entered.0);
    }
    let _apartment = Apartment;
    Ok(call())
}

/// Nothing to swap: over Cmdr the drag shows no Shell image (the overlay draws), outside it the
/// Shell's own.
pub(crate) fn prepare_self_drag_overlay(_rich_image_path: String) {}

/// Nothing to clear.
pub(crate) fn clear_self_drag_overlay() {}

/// Remembers what a drop over Cmdr would do (`copy`, `move`, or `none`), so the drop target's
/// cursor says the same as the overlay. Unknown values are ignored.
pub(crate) fn set_self_drag_resolved_op(operation: String) {
    if let Some(intent) = DropIntent::from_frontend(&operation) {
        set_intent(intent);
    }
}

/// Registers Cmdr's drop target on `webview`'s windows in place of wry's, sending what it sees to
/// that webview as `native-drag-drop` and `drag-modifiers`. Called for every webview
/// (`tauri_builder.rs`). Where none is replaced, wry's stays, and the frontend still
/// hears Tauri's own event.
pub(crate) fn install_drop_target(webview: &Webview<Wry>) {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let dispatched = webview.with_webview(move |platform| {
        let mut container = HWND::default();
        // SAFETY: `controller()` is the live controller Tauri hands this closure on the UI thread;
        // `ParentWindow` is a plain getter writing one handle into the live local.
        match unsafe { platform.controller().ParentWindow(&mut container) } {
            Ok(()) => {}
            Err(e) => {
                log::warn!(target: "native_drag", "no webview window to take drops on: {e}");
                return;
            }
        };
        let replaced = replace_drop_targets(container, &emitting_sink(app, label.clone()));
        if replaced == 0 {
            // allowed-log-private: a Tauri window label of our own
            log::warn!(target: "native_drag", "wry's drop target stays on `{label}`: none to replace");
        }
    });
    if let Err(e) = dispatched {
        log::warn!(target: "native_drag", "couldn't reach the webview to take its drops: {e}");
    }
}

/// Swaps wry's drop target for Cmdr's on every window under `container` that had one, as wry
/// registered them (it enumerates the same children). Returns how many it replaced.
fn replace_drop_targets(container: HWND, sink: &Sink) -> usize {
    let mut children: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `&mut Vec<HWND>` passed below, alive for the enumeration.
        let children = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        children.push(hwnd);
        true.into()
    }
    // SAFETY: the callback only pushes into `children`, which outlives the synchronous call.
    let _enumerated = unsafe {
        EnumChildWindows(
            Some(container),
            Some(collect),
            LPARAM(std::ptr::from_mut(&mut children) as isize),
        )
    };
    let mut replaced = 0;
    for hwnd in children {
        // SAFETY: windows of this thread; revoking one with no target fails harmlessly, and only
        // a window whose target we revoked gets ours.
        unsafe {
            if RevokeDragDrop(hwnd).is_ok() && RegisterDragDrop(hwnd, &drop_target_for(hwnd, sink.clone())).is_ok() {
                replaced += 1;
            }
        }
    }
    replaced
}

/// The sink that sends a target's events to the webview `label`, its files crossed to app paths.
fn emitting_sink(app: AppHandle, label: String) -> Sink {
    Rc::new(move |event| {
        let sent = match native_drag_drop(event) {
            Ok(drag) => drag.emit_to(&app, label.as_str()),
            Err(modifiers) => modifiers.emit_to(&app, label.as_str()),
        };
        if let Err(e) = sent {
            log::warn!(target: "native_drag", "couldn't send a drag event: {e}");
        }
    })
}

/// The event the frontend reads for `event`, with its native paths crossed to app paths: a
/// `native-drag-drop`, or the `drag-modifiers` a key change is.
fn native_drag_drop(event: TargetEvent) -> Result<NativeDragDrop, DragModifiers> {
    let point = |(x, y): (i32, i32)| DragPoint {
        x: f64::from(x),
        y: f64::from(y),
    };
    Ok(match event {
        TargetEvent::Enter { paths, position } => {
            let (paths, refused) = crossed(&paths);
            NativeDragDrop::Enter {
                paths,
                refused,
                position: point(position),
            }
        }
        TargetEvent::Drop { paths, position } => {
            let (paths, refused) = crossed(&paths);
            NativeDragDrop::Drop {
                paths,
                refused,
                position: point(position),
            }
        }
        TargetEvent::Over { position } => NativeDragDrop::Over {
            position: point(position),
        },
        TargetEvent::Leave => NativeDragDrop::Leave,
        TargetEvent::Modifiers(modifiers) => return Err(modifiers),
    })
}

/// `natives` crossed to app paths (`commands/native_paths.rs`), and how many no app path names.
fn crossed(natives: &[PathBuf]) -> (Vec<String>, u32) {
    use crate::commands::native_paths::{NativePathCrossing, answer};
    let mut paths = Vec::with_capacity(natives.len());
    let mut refused = 0u32;
    for native in natives {
        let crossing = cmdr_fs::app_path::AppPathBuf::from_native_unicode(native);
        match answer(&native.to_string_lossy(), crossing) {
            NativePathCrossing::App { path } => paths.push(path),
            NativePathCrossing::Refused => refused = refused.saturating_add(1),
        }
    }
    (paths, refused)
}
