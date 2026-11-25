//! Windows: the drive watcher (drives and mapped shares arriving and leaving, "Safely remove":
//! `volumes/platform/windows/watcher/`), and the system-look observers (the accent color,
//! Transparency effects, contrast themes, and Underline access keys, live through one settings
//! window: `system_look/platform/windows/`).

use tauri::AppHandle;

/// Starts every Windows installer.
pub(crate) fn install(app: &AppHandle) {
    // ❗ Without this call nothing hears a drive come or go: the app's `expect(dead_code)` for the
    // Windows port hides an unstarted watcher (#1481), so a drive showed only on the next
    // volume-list refresh and was never registered, and its panes never fell back.
    crate::volumes::start_volume_watcher(app);

    crate::system_look::observe_changes(app.clone());
}
