//! Windows: the system-look observers (the accent color, Transparency effects, contrast themes,
//! and Underline access keys, live through one settings window: `system_look/platform/windows/`).

use tauri::AppHandle;

/// Starts every Windows installer.
pub(crate) fn install(app: &AppHandle) {
    crate::system_look::observe_changes(app.clone());
}
