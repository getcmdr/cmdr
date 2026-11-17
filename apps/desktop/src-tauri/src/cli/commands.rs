//! IPC for the app menu's "Install “cmdr” command" item, and the item's live label.
//!
//! Every call reads through to the filesystem: ❌ never a stored flag. `install.rs` says why.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use super::install::{
    BUNDLE_SCRIPT, CliCommandBlocker, CliCommandOutcome, CliCommandResult, CliCommandState, CliCommandStatus,
    CliInstall, Layout,
};
use super::system_host::SystemHost;
use crate::menu::{CLI_INSTALL_ID, MenuState};

/// Reading the state is a handful of `lstat`s on local folders. The bound is the house rule for
/// filesystem calls, not an expectation.
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);

/// An install or uninstall may sit on the administrator prompt; `system_host` stops that at
/// five minutes. This only has to outlast it.
const ACTION_TIMEOUT: Duration = Duration::from_secs(330);

fn installer() -> CliInstall<SystemHost> {
    // Only a production build in a real bundle may point a link at itself: anything else lives
    // in a `target/` dir that comes and goes. Same gate as the reveal handler's.
    let own_script = crate::reveal::registration::own_bundle_id()
        .and_then(|_| crate::updater::platform::installer::running_bundle().ok())
        .map(|bundle| bundle.join(BUNDLE_SCRIPT));
    let install_blocker =
        (!crate::install_location::running_copy_is_installed()).then_some(CliCommandBlocker::NotInApplications);
    CliInstall::new(SystemHost, Layout::system(), own_script, install_blocker)
}

/// Where the `cmdr` command stands. `None` only if the filesystem didn't answer in time.
#[tauri::command]
#[specta::specta]
pub async fn get_cli_command_status() -> Option<CliCommandStatus> {
    read_status().await
}

/// Put the `cmdr` command on PATH. `admin_prompt` is the sentence macOS shows in its password
/// dialog, when the folder needs one; the frontend words it so it's in the user's language.
#[tauri::command]
#[specta::specta]
pub async fn install_cli_command(app: AppHandle, admin_prompt: String) -> CliCommandResult {
    run_action(app, move || installer().install(&admin_prompt)).await
}

/// Take the `cmdr` command off PATH, if it's ours to take.
#[tauri::command]
#[specta::specta]
pub async fn uninstall_cli_command(app: AppHandle, admin_prompt: String) -> CliCommandResult {
    run_action(app, move || installer().uninstall(&admin_prompt)).await
}

async fn read_status() -> Option<CliCommandStatus> {
    cmdr_fs::deadline::blocking_with_timeout(STATUS_TIMEOUT, None, || Some(installer().status())).await
}

async fn run_action<R: Runtime>(
    app: AppHandle<R>,
    action: impl FnOnce() -> CliCommandResult + Send + 'static,
) -> CliCommandResult {
    let result = match cmdr_fs::deadline::blocking_with_timeout(ACTION_TIMEOUT, None, move || Some(action())).await {
        Some(result) => result,
        None => CliCommandResult {
            outcome: CliCommandOutcome::Failed { code: None },
            // Whatever the stuck step did or didn't do, the filesystem has the answer.
            status: read_status().await.unwrap_or(CliCommandStatus {
                state: CliCommandState::NotInstalled,
                blocked_by: None,
            }),
        },
    };
    log::info!(target: "cli", "The cmdr command: {:?}, now {:?}", result.outcome, result.status);
    set_menu_label(&app, result.status.state);
    result
}

/// Re-read the state and put the matching words on the menu item. Fire and forget: called after
/// every menu build and on every main-window focus-gain, which is when the user can have changed
/// things from a terminal.
pub(crate) fn refresh_menu_item<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(status) = read_status().await {
            set_menu_label(&app, status.state);
        }
    });
}

/// The catalog key the menu item reads in `state`.
fn menu_label_key(state: CliCommandState) -> &'static str {
    match state {
        CliCommandState::NotInstalled | CliCommandState::TakenByOther => "menu.app.cliCommandInstall",
        CliCommandState::Installed => "menu.app.cliCommandUninstall",
        CliCommandState::InstalledByHomebrew => "menu.app.cliCommandHomebrew",
    }
}

fn set_menu_label<R: Runtime>(app: &AppHandle<R>, state: CliCommandState) {
    let Some(menu_state) = app.try_state::<MenuState<R>>() else {
        return;
    };
    if let Err(err) = menu_state.set_item_label(CLI_INSTALL_ID, crate::intl::menu_t(menu_label_key(state))) {
        log::warn!(target: "cli", "Couldn't relabel the cmdr command menu item: {err}");
        return;
    }
    // `set_text` leaves a plain title behind, so the display-only glyphs and the icon pass have
    // to run again (`menu/DETAILS.md`).
    crate::menu::set_display_accelerators_from_command(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_offers_what_a_click_would_do() {
        assert_eq!(
            menu_label_key(CliCommandState::NotInstalled),
            "menu.app.cliCommandInstall"
        );
        assert_eq!(
            menu_label_key(CliCommandState::Installed),
            "menu.app.cliCommandUninstall"
        );
        assert_eq!(
            menu_label_key(CliCommandState::InstalledByHomebrew),
            "menu.app.cliCommandHomebrew"
        );
        // Someone else's `cmdr` still reads as Install: the click explains why it can't.
        assert_eq!(
            menu_label_key(CliCommandState::TakenByOther),
            "menu.app.cliCommandInstall"
        );
    }
}
