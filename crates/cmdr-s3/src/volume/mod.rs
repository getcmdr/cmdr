//! The S3 backend: a `Volume` over one signed HTTP client, for one PLACE on an
//! account (a bucket, or the account root that lists the buckets).
//!
//! There is no OS mount under this: every listing and stat is a request, and
//! every request costs money, so nothing here asks the server for more than
//! the operation in hand needs (no stat per child, no watcher, no space poll).
//!
//! Nothing here names the application. What the backend needs from it arrives
//! through the [`VolumeHost`] seams handed to [`connect_s3_volume`].
//! `CLAUDE.md` has the must-knows, `DETAILS.md` the decisions.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::entry::FileEntry;

use cmdr_fs::ignore_poison::RwLockIgnorePoison;
use cmdr_fs::volume::VolumeError;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::settings::BackendName;
use cmdr_fs::volume::http_session::{HttpBackend, HttpSession};
use cmdr_fs::volume::remote_paths::RemoteRoot;
use tokio_util::sync::CancellationToken;

use crate::multipart::MIN_PART_SIZE;
use crate::params::S3ConnectionParams;
use crate::refusal::S3ConnectError;
use crate::sigv4::Credentials;
use crate::transport::S3Client;
use upload_ledger::{UploadLedger, ledger_account};

mod batch;
mod cut_off;
mod errors;
mod listing;
mod multipart_upload;
mod mutation;
mod paths;
mod query;
mod reconnect;
mod scan;
mod server_copy;
mod share_link;
mod streams;
mod upload;
mod upload_body;
mod upload_ledger;
mod volume_impl;
mod writes;

pub use cmdr_fs::volume::http_session::UnattendedReconnect;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// This backend's settings namespace, for everything it reads through
/// [`VolumeHost::settings`].
const BACKEND: BackendName = "s3";

/// One place on an S3 account: a bucket, or the account root.
pub struct S3Volume {
    /// Display name, as the app chose to label the place.
    name: String,
    /// Both spellings of the place's root: the app's
    /// (`s3://AKIA…@s3.eu-west-1.amazonaws.com:443/photos`) and the server's
    /// (`/photos`). ❗ Every path translation goes through it, ❌ never by hand.
    root: RemoteRoot,
    /// The account's `s3://<key id>@<host>:<port>`, for a row whose server name
    /// the typed path can't hold yet (`paths::S3Volume::row_path`).
    prefix: String,
    /// The root `Volume::root` answers, typed.
    app_root: AppPathBuf,
    inner: Arc<S3VolumeInner>,
}

/// The connection-scoped half.
struct S3VolumeInner {
    /// The key every piece of durable per-volume state is filed under.
    volume_id: String,
    /// How to reach the place again. Behind a lock for the same reason as
    /// `cmdr-webdav`'s (an edit may move what a redial asks for); read through
    /// `params()`, a snapshot.
    params: std::sync::RwLock<S3ConnectionParams>,
    /// The client, the connection it reports, and the switches that gate a
    /// rebuild: the HTTP lifecycle this backend shares with `cmdr-webdav`
    /// (`cmdr_fs::volume::http_session`).
    session: HttpSession<S3VolumeInner>,
    /// Everything this backend asks the app around it.
    host: VolumeHost,
    /// The multipart uploads this account started and hasn't settled
    /// (`upload_ledger.rs`), under the host's state directory.
    ledger: UploadLedger,
    /// Entries a write just verified, by server-side path, for the pane patch
    /// that follows it (`writes.rs`). Bounded; taken on read.
    written: std::sync::Mutex<HashMap<String, FileEntry>>,
    /// The smallest part a multipart upload cuts: `MIN_PART_SIZE`, except in a
    /// Docker cell that wants several parts from a small file.
    part_floor: AtomicU64,
    /// How long a paused upload holds its in-flight requests open, in
    /// milliseconds: `writes::PAUSE_HOLD`, except in a cell that can't wait it out.
    pause_hold_ms: AtomicU64,
    /// The server-side paths of files the last listing of their folder found
    /// beside a folder of their own name, and whether it showed each as
    /// `<name> (file)` or hid it (`paths.rs` § "A file beside a folder of its
    /// name").
    beside_folders: std::sync::Mutex<HashMap<String, paths::Shown>>,
}

impl S3VolumeInner {
    /// How to reach the place again, as it stands now. A snapshot, so ❌ no
    /// guard is ever held across a probe.
    fn params(&self) -> S3ConnectionParams {
        self.params.read_ignore_poison().clone()
    }
}

impl S3Volume {
    /// The volume id every listing-cache lookup and connection event uses.
    pub fn volume_id(&self) -> &str {
        &self.inner.volume_id
    }

    /// Moves the user's "reconnect automatically" switch on a mounted volume.
    /// ❗ Switching it ON while the volume sits `Disconnected` starts the
    /// backoff loop then and there.
    pub fn set_auto_reconnect(&self, on: bool) {
        self.inner.set_auto_reconnect(on);
    }

    /// Whether an unattended reconnect can actually happen as this volume
    /// stands. Reads the secret store only when the switch is on: a needless
    /// read is a needless Keychain prompt.
    pub async fn unattended_reconnect(&self) -> UnattendedReconnect {
        self.inner.unattended_reconnect().await
    }

    /// An empty [`Workload`](crate::cost::Workload) for this place's provider,
    /// for a cost estimate: the caller adds each file the operation touches.
    pub fn cost_workload(&self) -> crate::cost::Workload {
        crate::cost::Workload::for_provider(self.inner.params().provider())
    }

    /// Whether a copy from `source` to here would run on the server, asked
    /// without a request: the same account, and buckets the provider copies
    /// between (Spaces copies within one). `copy_on_server_impl` decides it
    /// for real per file.
    pub fn copies_on_server_from(&self, source: &S3Volume) -> bool {
        let (here, there) = (self.inner.params(), source.inner.params());
        source.inner.account() == self.inner.account()
            && (here.bucket() == there.bucket()
                || here
                    .provider()
                    .profile()
                    .is_ok_and(|profile| profile.cross_bucket_copy()))
    }

    /// Drops the live client. There is no session to close: dropping IS the
    /// shutdown, and the connection pool goes with it.
    pub async fn disconnect(&self) {
        self.inner.session.disconnect().await;
    }

    /// The live client, cloned out from under a short read guard. ❗ Clone and
    /// release: holding the guard across a request would serialize every other
    /// request behind it.
    async fn clone_client(&self) -> Result<Arc<S3Client>, VolumeError> {
        self.inner.session.client().await
    }

    /// Drops the live client the way a server going away would, ❗ WITHOUT
    /// starting the backoff loop, so a cell drives the recovery itself.
    #[cfg(any(test, feature = "testing"))]
    pub async fn simulate_session_loss(&self) {
        self.inner.session.simulate_loss().await;
    }

    /// The smallest part a multipart upload cuts.
    fn part_floor(&self) -> u64 {
        self.inner.part_floor.load(Ordering::Relaxed)
    }

    /// Cuts multipart uploads into parts as small as `bytes` (S3's 5 MiB
    /// floor at the least), so a cell sees several parts without uploading
    /// hundreds of megabytes. ❗ Production always cuts 64 MiB parts.
    #[cfg(any(test, feature = "testing"))]
    pub fn set_part_floor(&self, bytes: u64) {
        self.inner.part_floor.store(bytes, Ordering::Relaxed);
    }

    /// How long a paused upload holds its in-flight requests open.
    fn pause_hold(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.inner.pause_hold_ms.load(Ordering::Relaxed))
    }

    /// Lets a paused upload hold its requests open for `hold` only, so a cell
    /// sees a request set aside without waiting out the production hold.
    #[cfg(any(test, feature = "testing"))]
    pub fn set_pause_hold(&self, hold: std::time::Duration) {
        self.inner
            .pause_hold_ms
            .store(u64::try_from(hold.as_millis()).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    /// Points this volume's no-overwrite writes at the `If-None-Match: *`
    /// header, for a cell proving that path against VersityGW. ❌ Never in
    /// production (`ProviderProfile::trust_conditional_writes`).
    #[cfg(any(test, feature = "testing"))]
    pub async fn trust_conditional_writes(&self) {
        if let Some(client) = self.inner.session.installed_client().await {
            client.profile().trust_conditional_writes();
        }
    }

    /// Makes this volume's provider copy within one bucket only, the way
    /// Spaces' does, for a cell proving a cross-bucket copy streams instead.
    #[cfg(any(test, feature = "testing"))]
    pub async fn forbid_cross_bucket_copy(&self) {
        if let Some(client) = self.inner.session.installed_client().await {
            client.profile().forbid_cross_bucket_copy();
        }
    }

    /// Runs server-side copies and uploads at other part widths, for a live
    /// cell measuring throughput.
    #[cfg(any(test, feature = "testing"))]
    pub async fn set_concurrency(&self, copy: usize, upload: usize) {
        if let Some(client) = self.inner.session.installed_client().await {
            client.profile().set_concurrency(copy, upload);
        }
    }

    /// Runs the unfinished-upload sweep now and answers how many uploads it
    /// aborted, for a cell that can't wait on the one a connect starts.
    #[cfg(any(test, feature = "testing"))]
    pub async fn sweep_unfinished_uploads(&self) -> usize {
        self.inner.sweep_unfinished_uploads().await
    }
}

/// Carries the writes an account left unfinished (multipart uploads to abort,
/// cut-off PUTs to settle) from the address in `from` to the one in `to`, so
/// the next connect's sweep at the new address finds them: the records are
/// keyed by endpoint and key id, and no connect asks about the old endpoint
/// again (`DETAILS.md` § "Unfinished uploads"). For a server that MOVED, the
/// same storage at a new address; ❌ never for another server. Reads and
/// rewrites a small file under `host`'s state directory, so call it off the
/// async runtime. Answers how many records moved.
pub fn follow_account_move(host: &VolumeHost, from: &S3ConnectionParams, to: &S3ConnectionParams) -> usize {
    UploadLedger::at(host.state_dir(BACKEND)).move_account(&ledger_account(from), &ledger_account(to))
}

/// Opens one S3 place: reads the account's secret from the store, builds a
/// signed client, and probes (`ListBuckets`, then `HeadBucket` for a bucket).
///
/// `volume_id` must be the one the caller registers the volume under
/// (`cmdr_fs::volume::s3_volume_id`). Cancelling `cancel` ends the attempt
/// where it stands and leaves ❗ nothing behind: no volume, no secret written.
pub async fn connect_s3_volume(
    name: &str,
    volume_id: &str,
    params: S3ConnectionParams,
    host: VolumeHost,
    cancel: CancellationToken,
) -> Result<S3Volume, S3ConnectError> {
    let secret = {
        let host = host.clone();
        let service = params.credential_service();
        let scope = params.access_key_id().to_string();
        // ❗ On a blocking task: the store may put a Keychain prompt in front
        // of this, and a modal dialog on the async runtime stalls every volume.
        tokio::task::spawn_blocking(move || host.credentials().credentials(&service, Some(&scope)))
            .await
            .ok()
            .flatten()
    };
    let Some(secret) = secret else {
        return Err(S3ConnectError::NeedsCredentials);
    };
    let place = paths::Place::of(&params)?;
    let client = build_and_probe(&params, &secret.secret, &cancel).await?;
    if cancel.is_cancelled() {
        return Err(S3ConnectError::Cancelled);
    }
    // PII-free: an S3 client came up, and which provider preset it was. ❌ No
    // endpoint, key, bucket, or region crosses.
    host.analytics()
        .record("s3_connected", &[("provider", params.provider().kind_name())]);
    let volume = S3Volume::assemble(name, volume_id, place, params, client, host);
    // Uploads an earlier session left unfinished are billed until aborted.
    volume.inner.spawn_upload_sweep();
    Ok(volume)
}

/// A client for `params` with `secret`, proven by the connect probe.
async fn build_and_probe(
    params: &S3ConnectionParams,
    secret: &str,
    cancel: &CancellationToken,
) -> Result<S3Client, S3ConnectError> {
    let profile = params.profile().map_err(|_| S3ConnectError::InvalidProvider)?;
    let mut client = S3Client::new(profile, Credentials::new(params.access_key_id(), secret))?;
    // An account root reaches buckets in every region; a bucket place keeps
    // the probe's wrong-region refusal, which names the region to use.
    if params.bucket().is_none() {
        client.route_each_bucket();
    }
    tokio::select! {
        () = cancel.cancelled() => Err(S3ConnectError::Cancelled),
        probed = client.probe(params.bucket()) => probed.map(|()| client),
    }
}

impl S3Volume {
    fn assemble(
        name: &str,
        volume_id: &str,
        place: paths::Place,
        params: S3ConnectionParams,
        client: S3Client,
        host: VolumeHost,
    ) -> Self {
        let auto_reconnect = params.auto_reconnect;
        let ledger = UploadLedger::at(host.state_dir(BACKEND));
        Self {
            name: name.to_string(),
            root: place.root,
            prefix: place.prefix,
            app_root: place.app,
            inner: Arc::new_cyclic(|me| S3VolumeInner {
                session: HttpSession::new(me, volume_id, &host, Some(client), auto_reconnect),
                volume_id: volume_id.to_string(),
                params: std::sync::RwLock::new(params),
                host,
                ledger,
                written: std::sync::Mutex::new(HashMap::new()),
                beside_folders: std::sync::Mutex::new(HashMap::new()),
                part_floor: AtomicU64::new(MIN_PART_SIZE),
                pause_hold_ms: AtomicU64::new(u64::try_from(writes::PAUSE_HOLD.as_millis()).unwrap_or(u64::MAX)),
            }),
        }
    }
}

#[cfg(test)]
mod abort_confirm_test;
#[cfg(test)]
mod account_move_test;
#[cfg(test)]
mod batch_retry_test;
#[cfg(test)]
mod batch_test;
#[cfg(test)]
mod beside_folder_test;
#[cfg(test)]
mod conformance_test;
#[cfg(test)]
mod connection_drop_test;
#[cfg(test)]
mod copy_landed_test;
#[cfg(test)]
mod copy_test;
#[cfg(test)]
mod cut_off_record_test;
#[cfg(test)]
mod ended_upload_test;
#[cfg(test)]
mod fake_s3;
#[cfg(test)]
mod folder_delete_test;
#[cfg(test)]
mod fresh_folder_test;
#[cfg(test)]
mod get_length_test;
#[cfg(test)]
mod integration_test;
#[cfg(test)]
mod key_collision_test;
#[cfg(test)]
mod late_cancel_test;
#[cfg(test)]
mod lifecycle_test;
#[cfg(test)]
mod live_connect_test;
#[cfg(test)]
mod live_copy_pin_test;
#[cfg(test)]
mod live_flow_test;
#[cfg(test)]
mod live_hostile_failure_test;
#[cfg(test)]
mod live_hostile_support;
#[cfg(test)]
mod live_hostile_test;
#[cfg(test)]
mod live_protocol_test;
#[cfg(test)]
mod live_support;
#[cfg(test)]
mod long_key_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod put_retry_test;
#[cfg(test)]
mod put_source_test;
#[cfg(test)]
mod read_test;
#[cfg(test)]
mod reconnect_test;
#[cfg(test)]
mod refused_name_test;
#[cfg(test)]
mod share_link_test;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod write_test;
