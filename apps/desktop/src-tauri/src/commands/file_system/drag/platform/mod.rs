//! How each OS starts a drag out of the app, swaps the image of a drag inside it, and (Windows)
//! takes drops. A platform seam (`docs/guides/platform-seams.md`).

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows/mod.rs")]
mod imp;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod locality;
#[cfg(target_os = "linux")]
mod unsupported;

type StartDrag = fn(tauri::AppHandle, Vec<String>, String, Option<String>) -> Result<(), String>;

const _: StartDrag = start_drag;
const _: fn(String) = prepare_self_drag_overlay;
const _: fn() = clear_self_drag_overlay;
const _: fn(String) = set_self_drag_resolved_op;
const _: fn(&tauri::Webview<tauri::Wry>) = install_drop_target;

/// Starts a native drag of `paths` with `icon_path` as its image; `source_volume_id` decides
/// whether the session hands out file URLs or promises the bytes.
pub(super) use imp::start_drag;

/// Marks a drag of ours as active, with the rich image to show outside the window.
pub(super) use imp::prepare_self_drag_overlay;

/// Clears that state after the drop or a cancel.
pub(super) use imp::clear_self_drag_overlay;

/// The operation a drop would do (`copy`, `move`, or `none`), so the OS badge or cursor matches
/// it.
pub(super) use imp::set_self_drag_resolved_op;

/// Takes drops on a webview's windows with Cmdr's own drop target where wry's won't do (Windows:
/// OLE). macOS hooks wry's own view instead (`drag_image_detection.rs`), and Linux keeps wry's.
pub(crate) use imp::install_drop_target;
