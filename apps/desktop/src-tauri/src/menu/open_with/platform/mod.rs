//! How each OS draws "Open with" and what its "Other…" is. A platform seam
//! (`docs/guides/platform-seams.md`); the menu module exists on macOS and Windows only.

use std::path::{Path, PathBuf};

use crate::file_system::open_with::OpenWithLaunchError;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(PathBuf) -> OpenWithTarget = OpenWithTarget::app;
const _: fn(&OpenWithTarget) -> Option<&Path> = OpenWithTarget::app_path;
const _: fn(&OpenWithTarget) -> Option<String> = OpenWithTarget::app_name;
const _: fn(&OpenWithTarget, &[PathBuf]) -> Result<(), OpenWithLaunchError> = OpenWithTarget::launch;
const _: fn(&tauri::AppHandle<tauri::Wry>) -> Option<OpenWithTarget> = other_target;

/// What an "Open with" pick launches. `app` makes the app target for a path (on Windows, a Shell
/// handler's id); `app_path` is that path, `None` for the OS's dialog (Windows' "Choose another
/// app", which picks its own app); `app_name` names it for a toast; `launch` hands the files over.
pub(crate) use imp::OpenWithTarget;

/// What "Other…" launches: macOS's `NSOpenPanel` pick of an app (`None` when cancelled), or
/// Windows' dialog over the main window. Runs on the main thread, where the pick lands.
pub(crate) use imp::other_target;

/// macOS's native submenu: built whole, built pending ("Finding apps…"), and filled late, each
/// answering the `id → app path` map a pick resolves through; and the default's label in its
/// dimmed and undimmed parts, for `context_menu_icons.rs`.
#[cfg(target_os = "macos")]
pub(crate) use imp::{
    LabelPart, build_open_with_submenu, build_pending_open_with_submenu, default_label, fill_open_with_submenu,
};

/// Windows' submenu as page rows, with the `id → handler` map a pick resolves through.
#[cfg(target_os = "windows")]
pub(crate) use imp::page_open_with_submenu;
