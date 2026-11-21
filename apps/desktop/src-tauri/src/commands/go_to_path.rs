//! IPC commands for the "Go to path" dialog.
//!
//! Thin pass-throughs over the `go_to_path` module. `resolve_go_to_path` touches
//! the filesystem (`metadata`, a `..`'s link read), so every one of those waits
//! runs detached under ONE 2 s `Deadline` inside `go_to_path::resolve`: a hung
//! mount must never freeze IPC.

use crate::go_to_path::history::{MAX_RECENTS, RECENT_PATHS, RecentPathEntry};
use crate::go_to_path::{self, GoToPathResolution};
use cmdr_fs::deadline::DeadlineError;

/// Resolves a typed input against the focused pane's `base_dir`. Serves the
/// live as-you-type warning, the actual jump, and the clipboard-prefill check.
///
/// `go_to_path::resolve` always answers inside its deadline (an unreachable path or a
/// volume that didn't answer is a `GoToPathResolution` variant, not an error), so this
/// never reports `DeadlineError` today; the type stays for the wire.
#[tauri::command]
#[specta::specta]
pub async fn resolve_go_to_path(input: String, base_dir: String) -> Result<GoToPathResolution, DeadlineError> {
    Ok(go_to_path::resolve(&input, &base_dir).await)
}

/// Reads the persisted recent-path entries (newest first).
#[tauri::command]
#[specta::specta]
pub fn get_recent_paths() -> Vec<RecentPathEntry> {
    RECENT_PATHS.entries(None)
}

/// Adds a recent-path entry. Dedupes by resolved path, moves the match to the
/// top, and trims to the fixed cap.
#[tauri::command]
#[specta::specta]
pub fn add_recent_path(app: tauri::AppHandle, entry: RecentPathEntry) -> Result<(), String> {
    RECENT_PATHS.add(&app, entry, MAX_RECENTS);
    Ok(())
}

/// Removes a recent-path entry by id. No-op when the id isn't present.
#[tauri::command]
#[specta::specta]
pub fn remove_recent_path(app: tauri::AppHandle, id: String) -> Result<(), String> {
    RECENT_PATHS.remove(&app, &id);
    Ok(())
}

/// Clears every recent-path entry.
#[tauri::command]
#[specta::specta]
pub fn clear_recent_paths(app: tauri::AppHandle) -> Result<(), String> {
    RECENT_PATHS.clear(&app);
    Ok(())
}
