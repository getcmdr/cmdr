//! Windows: drags out through the Shell's `SHDoDragDrop` with the Shell's own data object for the
//! files (`data_object.rs`), and drags in through Cmdr's OLE drop target (`drop_target.rs`), which
//! replaces wry's on every webview. Rules: `effects.rs`. Decisions and the UI-thread budget:
//! `apps/desktop/src/lib/file-explorer/drag/DETAILS.md` § Windows.

mod data_object;
mod drag_out;
mod drop_target;
mod effects;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::rc::Rc;

use tauri::{AppHandle, Manager, Webview, Wry};
use tauri_specta::Event;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Ole::{RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
use windows::core::BOOL;

use super::locality::{SessionLocality, session_locality};
use crate::system_events::{DragModifiers, DragPoint, NativeDragDrop};
use drop_target::{Sink, TargetEvent, drop_target_for};
use effects::{DropIntent, set_intent};

/// Starts a drag of `paths` out of the main window. Returns at once, as the macOS arm does (the
/// frontend expects `startDragPaths` to resolve before the drag's events), and tells the frontend
/// `drag-out-ended` however the drag ends (`drag_out.rs`). `icon_path` goes unused: the Shell
/// draws Explorer's own drag image for the items, thumbnails and count included.
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
    drag_out::start(app, natives, locality)
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
    let window = webview.window();
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
        if replaced.is_empty() {
            // allowed-log-private: a Tauri window label of our own
            log::warn!(target: "native_drag", "wry's drop target stays on `{label}`: none to replace");
            return;
        }
        // OLE holds a reference to each target until it's revoked. Tauri says `Destroyed` once
        // the window is going; a child already gone answers an error, which costs nothing.
        let handles: Vec<isize> = replaced.iter().map(|hwnd| hwnd.0 as isize).collect();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                for handle in &handles {
                    // SAFETY: a window handle this thread registered; revoking a gone one fails
                    // harmlessly.
                    let _revoked = unsafe { RevokeDragDrop(HWND(*handle as *mut std::ffi::c_void)) };
                }
            }
        });
    });
    if let Err(e) = dispatched {
        log::warn!(target: "native_drag", "couldn't reach the webview to take its drops: {e}");
    }
}

/// Swaps wry's drop target for Cmdr's on every window under `container` that had one, as wry
/// registered them (it enumerates the same children). Returns the windows it replaced.
fn replace_drop_targets(container: HWND, sink: &Sink) -> Vec<HWND> {
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
    let mut replaced = Vec::new();
    for hwnd in children {
        // SAFETY: windows of this thread; revoking one with no target fails harmlessly, and only
        // a window whose target we revoked gets ours.
        unsafe {
            if RevokeDragDrop(hwnd).is_ok() && RegisterDragDrop(hwnd, &drop_target_for(hwnd, sink.clone())).is_ok() {
                replaced.push(hwnd);
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
