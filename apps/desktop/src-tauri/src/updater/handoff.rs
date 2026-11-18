//! Windows and Linux: staging a verified installer, and handing it to the Tauri updater plugin as
//! Cmdr quits to restart.
//!
//! Everything up to the hand-off is Cmdr's own and the same as on macOS: the check, the managed
//! policy, the minisign verification against both updater keys, the not-genuine memory, and the
//! advisory list. The plugin only runs the installer (`Update::install`): on Windows it starts the
//! NSIS installer and exits the process, and the installer reopens Cmdr; on Linux it replaces the
//! AppImage file, and Cmdr restarts itself. `DETAILS.md` § Windows and Linux.
//!
//! "Install" means something different here. On macOS `install_update` writes into the running
//! bundle, and the next launch runs it. An installer can't run under a live app, so here
//! `install_update` STAGES: it re-asks the policy, reads the advisory list, and keeps the verified
//! file. The restart toast's Restart is what applies it: `quit::relaunch` arms the hand-off when
//! [`is_staged`], the exit goes through the quit gate like ⌘Q, and [`run_at_exit`] runs last. A
//! quit without Restart installs nothing; the next launch checks, downloads, and asks again.

use std::path::PathBuf;

use super::manifest::{PlatformEntry, UpdateInfo};
use super::{DownloadedUpdate, HandoffPlan, UpdateInstallError, UpdateInstalled, UpdateState, advisories};

/// A verified installer waiting for Restart.
#[derive(Debug, Clone)]
pub(super) struct StagedInstaller {
    pub offer: UpdateInfo,
    pub file: PathBuf,
    /// The trusted key that verified it, which the plugin re-verifies with.
    pub pubkey: &'static str,
}

/// Turns a verified download into the staged installer Restart applies: the policy, read fresh,
/// must still allow it (`staged_to_install`), and the release's signed advisory list, if the
/// manifest names one, decides the security notice. `fetch_list` is injected so tests run it
/// without a network; production passes `advisories::fetch_signed_list`.
pub(super) async fn stage<F, Fut>(
    state: &UpdateState,
    running: &str,
    fetch_list: F,
) -> Result<UpdateInstalled, UpdateInstallError>
where
    F: FnOnce(PlatformEntry) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let (downloaded, _policy) = super::staged_to_install(state).await?;
    let DownloadedUpdate { offer, file, pubkey } = downloaded;
    let security_notice = match (&offer.security_advisories, semver::Version::parse(running)) {
        (Some(entry), Ok(running)) => fetch_list(entry.clone())
            .await
            .and_then(|json| advisories::notice_from(&json, &running, &offer.version)),
        _ => None,
    };
    log::info!(
        "Update {} is verified and staged; Restart hands it to the installer",
        offer.version
    );
    let replaced = state.slots().staged_installer.replace(StagedInstaller {
        offer,
        file: file.clone(),
        pubkey,
    });
    // The installer this one supersedes has nothing left to do; its own name keeps it apart.
    if let Some(old) = replaced.filter(|old| old.file != file)
        && let Err(e) = std::fs::remove_file(&old.file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("Couldn't remove the superseded installer: {e}");
    }
    Ok(UpdateInstalled { security_notice })
}

/// The plugin's view of the staged installer: a one-platform `latest.json` naming exactly the
/// offer this build verified, for `Updater::restore_update`. The plugin re-checks that it's newer
/// than the running build and that its signature, under `pubkey`, binds this version
/// (`require_signed_version`, which `restore_update` always turns on).
pub(super) fn restore_manifest(staged: &StagedInstaller) -> serde_json::Value {
    serde_json::json!({
        "version": staged.offer.version.to_string(),
        "platforms": {
            staged.offer.platform_key.clone(): {
                "url": staged.offer.url,
                "signature": staged.offer.signature,
            }
        }
    })
}

/// Whether an installer is staged, which turns Restart into "quit, install, reopen". The arming
/// itself is the quit gate's (`quit::relaunch`), next to macOS's relaunch waiter, so "Keep
/// working" calls both off in one place.
pub(crate) fn is_staged<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    tauri::Manager::try_state::<UpdateState>(app).is_some_and(|state| staged_version(&state).is_some())
}

fn staged_version(state: &UpdateState) -> Option<semver::Version> {
    state
        .slots()
        .staged_installer
        .as_ref()
        .map(|staged| staged.offer.version.clone())
}

/// Notes the install in the security log while the log can still write it: `RunEvent::Exit` calls
/// this before `security_log::on_exit` flushes, and [`run_at_exit`] runs last, since on Windows it
/// never returns.
pub(crate) fn note_handoff<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = tauri::Manager::try_state::<UpdateState>(app) {
        note_handoff_of(&state);
    }
}

fn note_handoff_of(state: &UpdateState) {
    if let Some(version) = staged_version(state) {
        super::security_events::on_handoff(&version);
    }
}

/// What the plugin gets: the staged release exactly as Cmdr verified it.
fn plan_for(staged: &StagedInstaller) -> HandoffPlan {
    HandoffPlan {
        platform_key: staged.offer.platform_key.clone(),
        pubkey: staged.pubkey,
        manifest: restore_manifest(staged),
    }
}

/// The last step of `RunEvent::Exit` after an armed Restart: hands the staged installer to the
/// plugin (`platform::hand_off`), then restarts Cmdr where the plugin doesn't (Linux). A hand-off
/// that doesn't go through is logged at error and still restarts Cmdr on the version it was,
/// rather than leaving the person with no Cmdr.
pub(crate) fn run_at_exit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager as _;

    let staged = app
        .try_state::<UpdateState>()
        .and_then(|state| state.slots().staged_installer.take());
    if let Some(staged) = staged {
        log::info!("Handing update {} to the installer", staged.offer.version);
        let installed = std::fs::read(&staged.file)
            .map_err(|e| format!("couldn't read the staged installer: {e}"))
            // Windows: starts the installer and exits this process, so nothing below runs.
            .and_then(|bytes| super::platform::hand_off(app, &plan_for(&staged), bytes));
        if let Err(detail) = installed {
            crate::log_error!("Update {} wasn't installed: {detail}", staged.offer.version);
        }
    }
    tauri::process::restart(&app.env());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_policy::testing::{DISABLE_UPDATES, forcing, override_for_test};
    use std::cell::Cell;

    fn offer(version: &str, advisories: bool) -> UpdateInfo {
        UpdateInfo {
            version: semver::Version::parse(version).expect("a valid test version"),
            platform_key: "windows-x86_64-nsis".into(),
            url: "https://example.invalid/Cmdr_0.53.0_x64-setup.exe".into(),
            signature: "sig".into(),
            security_advisories: advisories.then(|| PlatformEntry {
                url: "https://example.invalid/security-advisories.json".into(),
                signature: "list-sig".into(),
            }),
        }
    }

    fn downloaded(state: &UpdateState, offer: UpdateInfo) {
        state.slots().downloaded = Some(DownloadedUpdate {
            offer,
            file: PathBuf::from("/nonexistent/Cmdr-setup.exe"),
            pubkey: "key",
        });
    }

    const LIST: &str = r#"{"advisories":[
        {"id":"GHSA-crit-0000-0000","fixedIn":"0.53.0","severity":"critical","url":"https://github.com/getcmdr/cmdr/security/advisories/GHSA-crit-0000-0000"}
    ]}"#;

    #[tokio::test]
    async fn staging_keeps_the_installer_and_answers_the_lists_notice() {
        let state = UpdateState::new();
        downloaded(&state, offer("0.53.0", true));
        let fetched = Cell::new(None);
        let installed = stage(&state, "0.52.0", |entry| {
            fetched.set(Some(entry.url));
            async { Some(LIST.to_string()) }
        })
        .await
        .expect("staged");
        assert_eq!(
            fetched.take().as_deref(),
            Some("https://example.invalid/security-advisories.json")
        );
        let notice = installed.security_notice.expect("the list names a fix");
        assert_eq!(notice.count, 1);
        let staged = state.slots().staged_installer.clone().expect("staged");
        assert_eq!(staged.offer.version, semver::Version::new(0, 53, 0));
        assert!(state.slots().downloaded.is_none(), "a download stages once");
    }

    /// No advisory entry in the manifest (or a list that didn't verify, which `fetch_signed_list`
    /// answers as `None`) costs the notice, never the update.
    #[tokio::test]
    async fn no_list_means_no_notice_and_still_stages() {
        let state = UpdateState::new();
        downloaded(&state, offer("0.53.0", false));
        let installed = stage(&state, "0.52.0", |_| async { panic!("nothing to fetch") })
            .await
            .expect("staged");
        assert_eq!(installed.security_notice, None);

        let state = UpdateState::new();
        downloaded(&state, offer("0.53.0", true));
        let installed = stage(&state, "0.52.0", |_| async { None }).await.expect("staged");
        assert_eq!(installed.security_notice, None);
        assert!(state.slots().staged_installer.is_some());
    }

    /// Hostile: a profile that arrives between the download and the stage keeps it off the
    /// restart path, so a Restart can't install a version the organization refuses.
    #[tokio::test]
    async fn a_policy_that_arrives_after_the_download_stages_nothing() {
        let state = UpdateState::new();
        downloaded(&state, offer("0.53.0", true));
        let _policy = override_for_test(forcing(&[DISABLE_UPDATES]));
        let err = stage(&state, "0.52.0", |_| async { panic!("never fetched") })
            .await
            .expect_err("refused");
        assert_eq!(err, UpdateInstallError::BlockedByPolicy);
        assert!(state.slots().staged_installer.is_none());
        assert_eq!(
            staged_version(&state),
            None,
            "nothing staged, so Restart is a plain restart"
        );
    }

    /// A newer release staged over an older one replaces it: Restart installs the newest.
    #[tokio::test]
    async fn a_newer_stage_replaces_the_older_one() {
        let state = UpdateState::new();
        downloaded(&state, offer("0.53.0", false));
        stage(&state, "0.52.0", |_| async { None }).await.expect("staged");
        downloaded(&state, offer("0.54.0", false));
        stage(&state, "0.52.0", |_| async { None }).await.expect("staged");
        assert_eq!(staged_version(&state), Some(semver::Version::new(0, 54, 0)));
    }

    #[tokio::test]
    async fn the_hand_off_reaches_the_security_log() {
        let state = UpdateState::new();
        let capture = crate::security_log::testing::capture_events();
        note_handoff_of(&state);
        assert!(capture.events().is_empty(), "nothing staged, nothing noted");
        downloaded(&state, offer("0.53.0", false));
        stage(&state, "0.52.0", |_| async { None }).await.expect("staged");
        note_handoff_of(&state);
        assert!(
            matches!(
                capture.events().as_slice(),
                [crate::security_log::SecurityEvent::UpdateInstalled { to_version, .. }] if to_version == "0.53.0"
            ),
            "{:?}",
            capture.events()
        );
    }

    /// The plugin reads exactly the offer Cmdr verified, under the key it came from.
    #[test]
    fn the_plugin_gets_a_one_platform_manifest_of_the_verified_offer() {
        let staged = StagedInstaller {
            offer: offer("0.53.0", true),
            file: PathBuf::from("/nonexistent"),
            pubkey: "key",
        };
        let plan = plan_for(&staged);
        assert_eq!(plan.platform_key, "windows-x86_64-nsis");
        assert_eq!(plan.pubkey, "key");
        assert_eq!(
            plan.manifest,
            serde_json::json!({
                "version": "0.53.0",
                "platforms": {
                    "windows-x86_64-nsis": {
                        "url": "https://example.invalid/Cmdr_0.53.0_x64-setup.exe",
                        "signature": "sig",
                    }
                }
            })
        );
    }
}
