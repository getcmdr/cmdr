//! Windows and Linux share the installer hand-off: the Tauri updater plugin runs the installer
//! Cmdr already verified and staged. `../handoff.rs` decides when; this only drives the plugin.

use tauri_plugin_updater::UpdaterExt as _;

use crate::updater::HandoffPlan;

/// Hands `bytes` to the plugin as exactly the release `plan` names. The plugin re-verifies them
/// under `plan.pubkey` and re-checks the signed version (`restore_update` always requires it),
/// then installs. Windows: it starts the NSIS installer and exits this process, so it returns
/// only on a refusal. Linux: it replaces the AppImage file and returns.
pub(crate) fn hand_off<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    plan: &HandoffPlan,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .target(plan.platform_key.clone())
        .pubkey(plan.pubkey)
        .build()
        .map_err(|e| format!("couldn't build the updater: {e}"))?;
    let update = updater
        .restore_update(plan.manifest.clone())
        .map_err(|e| format!("the updater refused the staged release: {e}"))?
        .ok_or_else(|| "the updater says the staged release isn't newer".to_string())?;
    update
        .install(bytes)
        .map_err(|e| format!("the installer hand-off didn't go through: {e}"))
}
