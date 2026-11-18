//! Cmdr's updater, on every OS: the check, the managed policy, the minisign verification, and the
//! advisory list are all Cmdr's own. Only the last step differs (`install_kind.rs`, `platform/`):
//!
//! - macOS syncs files *into* the running `.app` bundle (`platform/macos/installer.rs`), preserving
//!   the bundle's `com.apple.macl` xattr and giving each file the fresh inode the code-signing cache
//!   needs.
//! - Windows and Linux stage the verified installer and hand it to the Tauri updater plugin as Cmdr
//!   quits to restart (`handoff.rs`).
//!
//! Three Tauri commands carry the flow:
//! - `check_for_update`: fetches `latest.json`, compares versions
//! - `download_update`: downloads the artifact, verifies its minisign signature
//! - `install_update`: macOS syncs it into the bundle; elsewhere it's staged for Restart

mod advisories;
pub(crate) mod handoff;
mod install_kind;
mod manifest;
// Crate-visible for macOS's own pieces (`platform::installer`, `platform::local_network_refresh`).
pub(crate) mod platform;
mod security_events;
mod signature;
mod write_blocker;

pub use advisories::SecurityNotice;
use install_kind::{Arch, InstallKind, NotUpdatable};
use manifest::UpdateInfo;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::State;
pub use write_blocker::BundleWriteBlocker;

// Per-call timeouts for the manifest fetch. The default `reqwest::get` client has no
// overall timeout. A stuck TCP handshake against the redirect target can hang for
// minutes before the OS gives up. These bounds keep a flaky network from looking like
// a hung app and stop the auto-error-reporter from firing on long hangs.
const MANIFEST_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MANIFEST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `update_write_blocker` waits on the bundle's `statfs` and folder walk. Milliseconds on a
/// local disk; the bound is for a bundle on a mount that stopped answering.
const CLASSIFY_TIMEOUT: Duration = Duration::from_secs(5);

// Per-call timeouts for the tarball download. No overall `timeout` here: a 60+ MB
// download on a slow connection can legitimately take minutes. `read_timeout` bounds
// "no bytes received in N seconds" instead, which catches mid-download stalls without
// punishing slow-but-working networks.
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(30);

use crate::ignore_poison::IgnorePoison as _;
use crate::managed_policy::{Egress, UpdatePolicy, UpdateRefusal};
use crate::server_request::describe_error_chain;
use cmdr_fs::log_private::LogPrivate;

/// What the latest check offered and what the latest download staged, shared by the three
/// commands. The backend keeps both so a download fetches only what a check offered (never a URL
/// the frontend names) and an install knows which version it's about to write.
pub struct UpdateState {
    slots: Mutex<UpdateSlots>,
}

#[derive(Default)]
struct UpdateSlots {
    offered: Option<UpdateInfo>,
    downloaded: Option<DownloadedUpdate>,
    /// Windows and Linux: the verified installer `install_update` staged for Restart.
    staged_installer: Option<handoff::StagedInstaller>,
    /// Versions refused this session because they didn't carry Cmdr's signature (minisign on
    /// the archive, or the Developer ID on the bundle). ❗ Never fetched again until Cmdr
    /// restarts, so the background loop can't re-download a fake every run; a newer version
    /// still goes through, so a real fix can arrive.
    not_genuine: std::collections::HashSet<semver::Version>,
}

/// A verified download waiting for `install_update`: the offer it answers, where its bytes are,
/// and the trusted key that verified them.
#[derive(Debug)]
struct DownloadedUpdate {
    offer: UpdateInfo,
    file: PathBuf,
    pubkey: &'static str,
}

impl UpdateState {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(UpdateSlots::default()),
        }
    }

    // A poisoned lock only means an earlier command panicked mid-write; every slot is plain data
    // that the next check or download replaces whole.
    fn slots(&self) -> std::sync::MutexGuard<'_, UpdateSlots> {
        self.slots.lock_ignore_poison()
    }
}

/// What set an update check going. The frontend's `update_check` analytics event carries the same
/// token, and the backend reads it to refuse a background check under
/// `DisableAutomaticUpdateChecks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckTrigger {
    /// The first wake of the poll loop as the app comes up.
    Startup,
    /// A background tick of the poll loop.
    Poll,
    /// `updates.autoCheck` going from off to on.
    AutoCheckOn,
    /// The `app.checkForUpdates` command (menu, command palette, shortcut).
    Command,
    /// The "Check for updates" button on Settings > Updates.
    Settings,
}

impl UpdateCheckTrigger {
    /// Whether the check is the automatic checking `DisableAutomaticUpdateChecks` turns off, as
    /// opposed to a person asking.
    fn is_automatic(self) -> bool {
        match self {
            Self::Startup | Self::Poll | Self::AutoCheckOn => true,
            Self::Command | Self::Settings => false,
        }
    }
}

/// What one update check found. The managed outcomes are answers, not failures: the frontend
/// renders them and ❌ never logs them at warn or error.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateCheckOutcome {
    /// Nothing newer than what's running (or this isn't a production install, see `skip_reason`).
    UpToDate,
    /// A newer release this Mac may install. `download_update` fetches exactly this one.
    Available { version: String },
    /// A newer release is out, but `MaxUpdateVersion` holds this Mac at `ceiling` or earlier.
    HeldByPolicy { available: String, ceiling: String },
    /// `DisableUpdates`: no request was made.
    UpdatesDisabledByPolicy,
    /// `DisableAutomaticUpdateChecks` refused a background check: no request was made. A check a
    /// person asks for still runs.
    AutomaticChecksDisabledByPolicy,
}

/// Why this process must not run an update check. Carried (rather than collapsed to a bool) so
/// the log names the exact condition that fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    /// Nothing here an update could reach (`install_kind::current`): a dev build, or an install
    /// the updater doesn't own.
    NotUpdatable(NotUpdatable),
    /// One of [`crate::prod_instance::NON_PROD_ENV_VARS`] is set in this process's environment.
    NonProdEnv(&'static str),
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotUpdatable(reason) => reason.fmt(f),
            Self::NonProdEnv(name) => write!(f, "{name} is set"),
        }
    }
}

/// Pure core of the gate, with the install kind and `env_is_set` injected so the matrix is
/// unit-testable without mutating the process environment or faking a bundle on disk.
///
/// The install condition is checked first: it's the one that makes an update impossible rather
/// than merely unwanted, so it's the more useful thing to see in a log.
fn skip_reason_for(
    install: Result<InstallKind, NotUpdatable>,
    env_is_set: &dyn Fn(&str) -> bool,
) -> Option<SkipReason> {
    if let Err(reason) = install {
        return Some(SkipReason::NotUpdatable(reason));
    }
    crate::prod_instance::non_prod_env_var_in(env_is_set).map(SkipReason::NonProdEnv)
}

/// The gate against this process's real environment and install.
fn skip_reason(install: Result<InstallKind, NotUpdatable>) -> Option<SkipReason> {
    skip_reason_for(install, &|name| std::env::var_os(name).is_some())
}

/// The update-check URL: the api-server counts the check as an active install by version and
/// `arch` (`InstallKind::check_arch_label`), then redirects to `latest.json`.
fn update_check_url(current_version: &str, kind: InstallKind, arch: Arch) -> String {
    format!(
        "https://api.getcmdr.com/update-check/{current_version}?arch={}",
        kind.check_arch_label(arch)
    )
}

/// Fetches `latest.json` (via the update check proxy for analytics) and says what it found, with
/// the organization's policy applied. An `Available` release is remembered for `download_update`.
///
/// Answers `UpToDate` when this isn't a real user's production install ([`skip_reason`]): nothing
/// an update could reach (`install_kind::current`: a dev build, a `.deb`), or one of
/// [`crate::prod_instance::NON_PROD_ENV_VARS`] is set. Every check reaches
/// `api.getcmdr.com/update-check`, which writes an `update_checks` row that the dashboard counts as
/// an active install, so Cmdr's own runs must never call it. Also `UpToDate` when the remote version
/// isn't newer, or the manifest has no entry for this platform.
#[tauri::command]
#[specta::specta]
pub async fn check_for_update(
    trigger: UpdateCheckTrigger,
    state: State<'_, UpdateState>,
) -> Result<UpdateCheckOutcome, crate::server_request::ServerRequestError> {
    let current_version = env!("CARGO_PKG_VERSION");
    let install = install_kind::current();
    let arch = Arch::current();
    let platform_keys = install.map(|kind| kind.platform_keys(arch)).unwrap_or_default();
    run_check(
        trigger,
        &state,
        skip_reason(install),
        current_version,
        &platform_keys,
        || async move {
            log::info!("Checking for updates (current version: {current_version})");
            // `skip_reason` stops every `Err` before the fetch, so this always has a kind.
            let kind = install.map_err(crate::server_request::ServerRequestError::unexpected)?;
            fetch_manifest(&update_check_url(current_version, kind, arch)).await
        },
    )
    .await
}

/// The check with its environment injected, so tests can run every policy without a bundle or a
/// network. `fetch` runs only when the policy and `skip` let the check reach the network.
///
/// The policy goes FIRST, ahead of `skip`: a dev build run with `CMDR_MANAGED_PREFS_FILE` then
/// shows the managed answer, which is what a person testing a profile wants to see.
async fn run_check<F, Fut>(
    trigger: UpdateCheckTrigger,
    state: &UpdateState,
    skip: Option<SkipReason>,
    current_version: &str,
    platform_keys: &[String],
    fetch: F,
) -> Result<UpdateCheckOutcome, crate::server_request::ServerRequestError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<manifest::UpdateManifest, crate::server_request::ServerRequestError>>,
{
    // Every check replaces the last offer, so a download can only fetch what the newest check
    // offered under the newest policy.
    state.slots().offered = None;

    let policy = crate::managed_policy::for_egress().await;
    match policy.updates() {
        UpdatePolicy::Disabled => {
            log::info!(target: "managed_policy", "Not checking for updates: the organization's policy turns updates off");
            return Ok(UpdateCheckOutcome::UpdatesDisabledByPolicy);
        }
        UpdatePolicy::Enabled {
            automatic_checks: false,
            ..
        } if trigger.is_automatic() => {
            log::info!(target: "managed_policy", "Not running the {trigger:?} update check: the organization's policy turns automatic checks off");
            return Ok(UpdateCheckOutcome::AutomaticChecksDisabledByPolicy);
        }
        UpdatePolicy::Enabled { .. } => {}
    }

    if let Some(reason) = skip {
        log::info!("Skipping update check: {reason}");
        return Ok(UpdateCheckOutcome::UpToDate);
    }

    let manifest = fetch().await?;
    let Some(update) = manifest::check_manifest(&manifest, current_version, platform_keys) else {
        return Ok(UpdateCheckOutcome::UpToDate);
    };
    let version = update.version.to_string();
    match policy.update_to(&update.version) {
        Ok(()) => {
            state.slots().offered = Some(update);
            Ok(UpdateCheckOutcome::Available { version })
        }
        Err(UpdateRefusal::AboveCeiling(ceiling)) => {
            log::info!(target: "managed_policy", "Update {version} is out, but the organization's policy holds this Mac at {ceiling} or earlier");
            Ok(UpdateCheckOutcome::HeldByPolicy {
                available: version,
                ceiling: ceiling.to_string(),
            })
        }
        // The policy said "enabled" a moment ago and it's the same read, so this can't happen;
        // answering it truthfully costs nothing.
        Err(UpdateRefusal::Disabled) => Ok(UpdateCheckOutcome::UpdatesDisabledByPolicy),
    }
}

/// Fetches and parses the manifest at `url`. Split from the command so a test can point it at a mock
/// server instead of the update endpoint.
///
/// The status is checked before the body is parsed (`server_request::send`): a 5xx or an HTML
/// maintenance page would otherwise read as "the manifest is malformed" and send whoever reads the log
/// to the wrong layer. A 2xx that doesn't parse is `BadResponse`, which the frontend logs at error:
/// Cmdr's server and this build disagree on the contract. The frontend owns the log line, gated once
/// per condition, so a Rust warn here would only repeat it every poll tick.
async fn fetch_manifest(url: &str) -> Result<manifest::UpdateManifest, crate::server_request::ServerRequestError> {
    let client = cmdr_http::client_builder()
        .connect_timeout(MANIFEST_CONNECT_TIMEOUT)
        .timeout(MANIFEST_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| {
            crate::server_request::ServerRequestError::unexpected(format!(
                "update HTTP client: {}",
                describe_error_chain(&e)
            ))
        })?;
    let response = crate::server_request::send(Egress::UpdateCheck, client.get(url)).await?;
    crate::server_request::read_json(response).await
}

/// Reports whether the running bundle sits somewhere an update can be written into, or `None`
/// when nothing is in the way.
///
/// The frontend asks after a check finds an update and before the download starts. Skipping the
/// download is the point: an install that can't write its own bundle would otherwise pull ~63 MB
/// and rewrite nothing, once per poll interval, for as long as the app runs. It also gives the
/// user a reason for a failure they'd otherwise never see, since no blocker can be fixed from
/// inside the app.
///
/// Returns `None` outside a `.app` bundle too: there's no bundle to classify, and the check gate
/// (`skip_reason`) has already stopped that process from getting here. A classification that
/// doesn't finish in `CLASSIFY_TIMEOUT` (a stalled mount under the bundle) answers `None` as
/// well: the installer runs the same check again before writing anything.
///
/// Off macOS there's no bundle to classify, so it answers `None`: the NSIS installer writes a
/// per-user install, and an AppImage the plugin can't replace fails the hand-off at exit.
#[tauri::command]
#[specta::specta]
pub async fn update_write_blocker() -> Result<Option<BundleWriteBlocker>, String> {
    Ok(cmdr_fs::deadline::blocking_with_timeout(CLASSIFY_TIMEOUT, None, platform::write_blocker).await)
}

/// The running bundle's path, for the notice that asks someone to replace it ("`/Applications/Cmdr.app`
/// belongs to another account"), so it names the real folder and app name. `None` outside a bundle,
/// and off macOS, where no notice asks for it. No filesystem access: it walks up from the
/// executable's own path.
#[tauri::command]
#[specta::specta]
pub fn update_bundle_path() -> Option<String> {
    platform::bundle_path().map(|bundle| bundle.to_string_lossy().into_owned())
}

/// Why a tarball download didn't leave a verified file behind. The frontend picks the log level
/// off the variant: a `Request` failure follows the api-server rule (no network, a timeout, or a
/// 5xx is the person's network or the host's bad moment, so warn), while a signature mismatch or a
/// disk failure means something is wrong with the release or this machine, so error.
///
/// ❌ `detail` is for logs only, never a sentence a person reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateDownloadError {
    /// The tarball request didn't come back with the bytes.
    Request {
        failure: crate::server_request::ServerRequestError,
    },
    /// The bytes arrived but don't verify against the manifest's signature.
    SignatureMismatch { detail: String },
    /// The verified tarball couldn't be written to the temp dir.
    Disk { detail: String },
    /// No check has offered an update since the last one: the frontend asked out of turn.
    NothingOffered,
    /// The organization's policy, read fresh, no longer allows the offered version (it arrived
    /// after the check). Nothing was fetched. Not a failure: ❌ never log it at warn or error.
    BlockedByPolicy,
    /// This version didn't carry Cmdr's signature earlier this session, so nothing was fetched
    /// again. The frontend words it exactly like the refusal it repeats.
    NotGenuineEarlier,
}

impl From<crate::server_request::ServerRequestError> for UpdateDownloadError {
    fn from(failure: crate::server_request::ServerRequestError) -> Self {
        Self::Request { failure }
    }
}

/// Downloads the artifact at `url` and verifies it against `signature`. Split from the command so a
/// test can point it at a mock server. `bind_to` is the version the signature must name when the
/// install kind binds it there (Windows, Linux: [`bind_signed_version`]).
///
/// The status is checked before the bytes are trusted (`server_request::send`), so a 5xx
/// maintenance page reads as the host's bad moment rather than as an artifact that fails its
/// signature.
async fn fetch_verified(
    url: &str,
    signature: &str,
    bind_to: Option<&semver::Version>,
) -> Result<(Vec<u8>, signature::VerifiedSignature), UpdateDownloadError> {
    let client = cmdr_http::client_builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .build()
        .map_err(|e| {
            crate::server_request::ServerRequestError::unexpected(format!(
                "update HTTP client: {}",
                describe_error_chain(&e)
            ))
        })?;

    let response = crate::server_request::send(Egress::UpdateDownload, client.get(url)).await?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| crate::server_request::ServerRequestError::from_transport(&e))?;

    log::info!("Downloaded {} bytes, verifying signature", bytes.len());
    let verified =
        signature::verify(&bytes, signature).map_err(|detail| UpdateDownloadError::SignatureMismatch { detail })?;
    if let Some(offered) = bind_to {
        bind_signed_version(&verified, offered).map_err(|detail| UpdateDownloadError::SignatureMismatch { detail })?;
    }
    log::info!("Signature verified");
    // Zero-copy when the buffer is uniquely owned, which a freshly read body is.
    Ok((Vec::from(bytes), verified))
}

/// Refuses a signature that doesn't name the offered version. Windows and Linux can't open an
/// installer before it runs, so the signature's trusted comment (`--app-version`, covered by the
/// signature) is the one signed statement of which release the bytes are. Without it, an unsigned
/// manifest could pair a newer version number with an older genuine installer and roll the install
/// back. macOS reads the extracted bundle's `Info.plist` instead (`installer::refuse_unless_newer`).
///
/// A signature with no version is refused too: every Windows and Linux artifact is signed with one.
fn bind_signed_version(verified: &signature::VerifiedSignature, offered: &semver::Version) -> Result<(), String> {
    let Some(signed) = verified.signed_version.as_deref() else {
        return Err(format!(
            "the signature names no version, so it can't vouch for {offered}"
        ));
    };
    match semver::Version::parse(signed.trim_start_matches('v')) {
        Ok(signed) if &signed == offered => Ok(()),
        _ => Err(format!(
            "the signature is for version {signed}, not the offered {offered}"
        )),
    }
}

/// Downloads the update the last check offered and verifies its minisign signature. Takes no URL:
/// the backend fetches only what it offered, so a bypassed frontend can't stage anything else.
///
/// On success, records the file and its offer in `UpdateState` for `install_update`.
#[tauri::command]
#[specta::specta]
pub async fn download_update(state: State<'_, UpdateState>) -> Result<(), UpdateDownloadError> {
    // `check_for_update` never offers to an install it skips, so there's nothing to download.
    let kind = install_kind::current().map_err(|_| UpdateDownloadError::NothingOffered)?;
    let (offer, bytes, verified) = download_verified(&state, kind).await?;

    let temp_dir = std::env::temp_dir().join("cmdr-update");
    std::fs::create_dir_all(&temp_dir).map_err(|e| UpdateDownloadError::Disk {
        detail: format!("couldn't create temp dir: {e}"),
    })?;

    let file = temp_dir.join(kind.download_file_name(&offer.version));
    std::fs::write(&file, &bytes).map_err(|e| UpdateDownloadError::Disk {
        detail: format!("couldn't write the update: {e}"),
    })?;

    state.slots().downloaded = Some(DownloadedUpdate {
        offer,
        file,
        pubkey: verified.pubkey,
    });
    Ok(())
}

/// Fetches the offered artifact and verifies it. A signature that doesn't hold reaches the security
/// log (and so the person's notice) and marks the version as not genuine for the session.
async fn download_verified(
    state: &UpdateState,
    kind: InstallKind,
) -> Result<(UpdateInfo, Vec<u8>, signature::VerifiedSignature), UpdateDownloadError> {
    let offer = offer_to_download(state).await?;
    log::info!("Downloading update {} from {}", offer.version, offer.url); // allowed-log-private: our own release URL
    let bind_to = kind.binds_version_in_signature().then_some(&offer.version);
    let (bytes, verified) = fetch_verified(&offer.url, &offer.signature, bind_to)
        .await
        .inspect_err(|e| {
            security_events::on_download_error(&offer.version, e);
            if matches!(e, UpdateDownloadError::SignatureMismatch { .. }) {
                state.slots().not_genuine.insert(offer.version.clone());
            }
        })?;
    Ok((offer, bytes, verified))
}

/// The offered update, if the policy, read fresh, still allows its version and it hasn't
/// already turned out not to be genuine this session. A profile that arrived
/// between the check and the download stops it here, before a byte is fetched.
async fn offer_to_download(state: &UpdateState) -> Result<UpdateInfo, UpdateDownloadError> {
    let offer = state
        .slots()
        .offered
        .clone()
        .ok_or(UpdateDownloadError::NothingOffered)?;
    if state.slots().not_genuine.contains(&offer.version) {
        log::info!(
            "Not downloading update {} again: it didn't carry Cmdr's signature earlier this session",
            offer.version
        );
        return Err(UpdateDownloadError::NotGenuineEarlier);
    }
    if let Err(refusal) = crate::managed_policy::for_egress().await.update_to(&offer.version) {
        log::info!(target: "managed_policy", "Not downloading update {}: the organization's policy refuses it ({refusal:?})", offer.version);
        return Err(UpdateDownloadError::BlockedByPolicy);
    }
    Ok(offer)
}

/// Why `install_update` didn't install. `Failed` covers everything local (extraction, the version
/// check, the sync), which the frontend logs at error.
///
/// ❌ `detail` is for logs only, never a sentence a person reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateInstallError {
    /// The organization's policy, read fresh, doesn't allow the staged version. Nothing was
    /// written. Not a failure: ❌ never log it at warn or error.
    BlockedByPolicy,
    /// No verified download is waiting: the frontend asked out of turn.
    NothingStaged,
    /// The staged bundle's Apple code signature isn't a valid Cmdr Developer ID signature, though
    /// its archive passed minisign. Nothing was written. Means the updater key signed something
    /// that isn't a Cmdr release (or the release itself is broken): log it at error. macOS only.
    #[allow(
        dead_code,
        reason = "only the macOS install (`platform::install_bundle`) checks a code signature"
    )]
    UntrustedCodeSignature { detail: String },
    /// This install can't write its own bundle (`BundleWriteBlocker`), caught by the installer's own
    /// pre-flight check or by a write the sync was refused. The same answer `update_write_blocker`
    /// gives, so the frontend raises the same notice. Not a failure: ❌ never log it at error.
    /// macOS only.
    #[allow(
        dead_code,
        reason = "only the macOS install (`platform::install_bundle`) classifies its bundle"
    )]
    Unwritable { blocker: BundleWriteBlocker },
    /// The install ran and didn't finish.
    Failed { detail: String },
}

impl From<String> for UpdateInstallError {
    fn from(detail: String) -> Self {
        Self::Failed { detail }
    }
}

/// What a finished install tells the frontend beyond "restart to apply".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstalled {
    /// The security fixes this update carries, from a signed advisory list (the staged bundle's on
    /// macOS, the release's signed asset elsewhere; never from `latest.json` itself), or `None`
    /// when it carries none the running build lacks.
    pub security_notice: Option<SecurityNotice>,
}

/// Makes a previously downloaded update ready for the restart: macOS syncs it into the running
/// `.app` bundle; Windows and Linux stage the verified installer, which Restart hands off
/// (`handoff.rs`).
///
/// Takes the download `download_update` recorded, and refuses it when the CURRENT policy doesn't
/// allow its version: a download staged before a profile arrived must not install.
#[tauri::command]
#[specta::specta]
pub async fn install_update(state: State<'_, UpdateState>) -> Result<UpdateInstalled, UpdateInstallError> {
    match install_kind::current() {
        Ok(InstallKind::MacAppBundle) => install_into_bundle(&state).await,
        Ok(InstallKind::WindowsNsis | InstallKind::LinuxAppImage) => {
            handoff::stage(&state, env!("CARGO_PKG_VERSION"), |entry| async move {
                advisories::fetch_signed_list(&entry).await
            })
            .await
        }
        // `check_for_update` never offers to an install it skips, so nothing can be downloaded.
        Err(_) => Err(UpdateInstallError::NothingStaged),
    }
}

/// What a staged bundle that passed every check holds: the version its own `Info.plist` names (the
/// one the policy was asked about, and so the one that gets installed) and the security notice its
/// own advisory list names. macOS only (`platform::install_bundle`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VettedBundle {
    pub version: semver::Version,
    pub security_notice: Option<SecurityNotice>,
}

/// What the Windows and Linux hand-off gives the plugin: the release exactly as Cmdr verified it.
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the Windows and Linux arm (`platform/plugin.rs`); macOS never hands off"
)]
pub struct HandoffPlan {
    /// The `latest.json` key the release came from, which the plugin asks for by name.
    pub platform_key: String,
    /// The trusted key that verified the download; the plugin re-verifies with it.
    pub pubkey: &'static str,
    /// A one-platform manifest of exactly the verified offer (`handoff::restore_manifest`).
    pub manifest: serde_json::Value,
}

/// The macOS install: extracts the tarball and syncs it into the running bundle.
async fn install_into_bundle(state: &UpdateState) -> Result<UpdateInstalled, UpdateInstallError> {
    let (staged, policy) = staged_to_install(state).await?;
    log::info!(
        "Installing update {} from {}",
        staged.offer.version,
        LogPrivate::file_path(&staged.file)
    );

    // The archive's own `Info.plist` is what binds its version (the manifest isn't signed), so the
    // installer asks the policy again about THAT version once it has extracted it.
    let offered = staged.offer.version.clone();
    let result = tokio::task::spawn_blocking(move || {
        platform::install_bundle(&staged.file, &|version| {
            policy
                .update_to(version)
                .inspect_err(|refusal| security_events::on_policy_refusal(version, *refusal))
                .is_ok()
        })
    })
    .await
    .map_err(|e| UpdateInstallError::Failed {
        detail: format!("Install task panicked: {e}"),
    })
    .and_then(|installed| installed);
    security_events::on_install_result(&offered, result.as_ref().map(|vetted| &vetted.version));
    if let Err(error) = &result {
        note_install_refusal(state, &offered, error);
    }
    let security_notice = result?.security_notice;
    if let Some(notice) = &security_notice {
        log::info!(
            "The update fixes {} security issue(s), the most severe {:?}",
            notice.count,
            notice.severity
        );
    }
    Ok(UpdateInstalled { security_notice })
}

/// A bundle refused for its code signature marks its version as not genuine for the session.
fn note_install_refusal(state: &UpdateState, offered: &semver::Version, error: &UpdateInstallError) {
    if matches!(error, UpdateInstallError::UntrustedCodeSignature { .. }) {
        state.slots().not_genuine.insert(offered.clone());
    }
}

/// The staged download and the fresh policy that allows it, or why not.
async fn staged_to_install(
    state: &UpdateState,
) -> Result<(DownloadedUpdate, std::sync::Arc<crate::managed_policy::ManagedPolicy>), UpdateInstallError> {
    let staged = state
        .slots()
        .downloaded
        .take()
        .ok_or(UpdateInstallError::NothingStaged)?;
    let policy = crate::managed_policy::for_egress().await;
    let version = &staged.offer.version;
    if let Err(refusal) = policy.update_to(version) {
        log::info!(target: "managed_policy", "Not installing update {version}: the organization's policy refuses it ({refusal:?})");
        security_events::on_policy_refusal(version, refusal);
        return Err(UpdateInstallError::BlockedByPolicy);
    }
    Ok((staged, policy))
}

#[cfg(test)]
mod tests;
