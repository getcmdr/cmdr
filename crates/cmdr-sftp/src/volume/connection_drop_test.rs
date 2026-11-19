//! A server that REALLY goes away under a live volume, and comes back.
//!
//! `reconnect_test.rs` drives the gates by dropping the session in-process
//! (`simulate_session_loss`), which proves the policy and never the wire. These
//! cells cut the actual TCP connection instead, which is what a user hits: a NAS
//! that sleeps or reboots, Wi-Fi that drops.
//!
//! ❗ The cut happens in a `cmdr_fs::testing::tcp_proxy::TcpProxy` this test owns,
//! between the client and `sftp-fixture-openssh`. ❌ Never pause or stop the
//! container: the stack is shared by lease with other test binaries, worktrees,
//! and sessions, and every one of them would see the outage too.
//!
//! Two ways down, because they end differently:
//!
//! - **Refused**: the connection closes and a redial is refused. The session's
//!   own EOF is what the next operation finds.
//! - **Silent** (a black hole): nothing closes, nothing answers. Only a timer
//!   can end a wait, so this is the cell that finds a missing deadline. It runs
//!   under a paused tokio clock so the production-length deadline elapses in
//!   virtual time.
//!
//! Every cell here needs the SFTP fixture stack: `sftp-servers/start.sh`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::testing::tcp_proxy::TcpProxy;
use cmdr_fs::testing::wait_until_async;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::credentials::InMemoryCredentials;
use cmdr_fs::volume::host::events::{RecordingVolumeEvents, VolumeConnection, VolumeEventSink};
use cmdr_fs::volume::host::host_keys::InMemoryHostKeys;
use cmdr_fs::volume::{ConnectionState, Volume, VolumeError};

use super::super::SftpVolume;
use super::super::testing::*;
use crate::params::SftpConnectionParams;
use crate::transport::SILENT_SERVER_DEADLINE;

const FIXTURE: &str = "sftp-servers/start.sh (sftp-fixture)";

/// How long an operation on a CLOSED connection may take to answer. Generous:
/// the real answer is milliseconds, and this is a hang backstop that keeps the
/// failure message ours rather than nextest's.
const ANSWERS_WITHIN: Duration = Duration::from_secs(3);

/// How long the backend's own loop gets to bring a volume back: its first
/// backoff step (2 s) plus a full dial, under the 8 s nextest cap.
const COMES_BACK_WITHIN: Duration = Duration::from_secs(6);

/// A volume on `sftp-fixture-openssh`, connected THROUGH a proxy this test owns.
struct Proxied {
    proxy: TcpProxy,
    events: Arc<RecordingVolumeEvents>,
    volume: SftpVolume,
}

/// Connects through a fresh proxy, password rung, secret remembered.
///
/// ❗ The host runs background work on THIS runtime (`Handle::current`), so a
/// paused clock pauses the session's keepalive timer and the backoff loop along
/// with the proxy. The fallback runtime would keep real time and make the
/// silent cell wait out a real deadline.
async fn through_a_proxy() -> Proxied {
    let fixture = fixture_params("OPENSSH", 12480);
    let proxy = TcpProxy::start(SocketAddr::from(([127, 0, 0, 1], fixture.port))).await;
    let params = SftpConnectionParams::new("127.0.0.1", proxy.port(), FIXTURE_USER, FIXTURE_ROOT).without_agent();
    let events = Arc::new(RecordingVolumeEvents::new());
    let host = VolumeHost::builder()
        .runtime(tokio::runtime::Handle::current())
        .events(Arc::clone(&events) as Arc<dyn VolumeEventSink>)
        .credentials(Arc::new(InMemoryCredentials::new().with_entry(
            &params.credential_service(),
            Some(FIXTURE_USER),
            FIXTURE_USER,
            FIXTURE_PASSWORD,
        )))
        .host_keys(Arc::new(InMemoryHostKeys::new()))
        .build();
    let volume = connect_fixture(&host, params).await;
    Proxied { proxy, events, volume }
}

/// The root's seeded names, sorted, or the error the listing answered.
///
/// ❗ Scratch directories left out: other cells create and delete theirs in
/// the same export while this one runs, so two listings a few seconds apart
/// legitimately differ there.
async fn names(volume: &SftpVolume) -> Result<Vec<String>, VolumeError> {
    let mut names: Vec<String> = volume
        .list_directory(&app_at(volume, "."), None)
        .await?
        .into_iter()
        .map(|entry| entry.name)
        .filter(|name| !name.starts_with("cmdr-test-"))
        .collect();
    names.sort();
    Ok(names)
}

/// The states a volume reported, in order.
fn reported(events: &RecordingVolumeEvents) -> Vec<VolumeConnection> {
    events.transitions().into_iter().map(|(_, state)| state).collect()
}

/// Where the volume stands, asked the way the app asks.
fn state(volume: &SftpVolume) -> Option<ConnectionState> {
    let asked_the_way_the_app_asks: &dyn Volume = volume;
    asked_the_way_the_app_asks.connection_state()
}

/// ❗ **A server that goes away is reported down at once, with a typed error,
/// and comes back on its own once it's reachable again.**
///
/// The first operation after the drop is the detector (there is no watcher), so
/// it has to answer `DeviceDisconnected` promptly: that is the one error that
/// flips the state and starts the backoff. A retry while the server is still
/// gone answers the same way and reports nothing new. Then the server returns
/// and the backend's own loop, with nobody asking, brings the SAME volume back.
#[tokio::test]
#[ignore = "needs the SFTP fixture stack: sftp-servers/start.sh (sftp-fixture)"]
async fn sftp_integration_a_server_that_goes_away_is_reported_down_and_comes_back_on_its_own() {
    let Proxied { proxy, events, volume } = through_a_proxy().await;
    let before = names(&volume).await.expect(FIXTURE);

    proxy.refuse().await;

    let failed = tokio::time::timeout(ANSWERS_WITHIN, names(&volume))
        .await
        .expect("❗ an operation on a closed connection has to answer, not hang");
    assert!(
        matches!(failed, Err(VolumeError::DeviceDisconnected(_))),
        "only DeviceDisconnected flips the state and starts the backoff, got {failed:?}"
    );
    assert_eq!(state(&volume), Some(ConnectionState::Disconnected));
    assert_eq!(reported(&events), vec![VolumeConnection::Disconnected]);

    // The frontend's own backoff tick, while the server is still gone.
    let retry = tokio::time::timeout(ANSWERS_WITHIN, volume.attempt_reconnect())
        .await
        .expect("a redial to a refusing server answers, not hangs");
    assert!(
        matches!(retry, Err(VolumeError::DeviceDisconnected(_))),
        "a refused dial is transient, never a sign-in: {retry:?}"
    );
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected],
        "a failed retry is not news"
    );

    proxy.restore().await;

    wait_until_async(COMES_BACK_WITHIN, "the backoff loop to bring the volume back", || {
        reported(&events).contains(&VolumeConnection::Connected)
    })
    .await;
    assert_eq!(state(&volume), Some(ConnectionState::Direct));
    assert_eq!(
        names(&volume).await.expect(FIXTURE),
        before,
        "the same volume lists again"
    );
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected, VolumeConnection::Connected],
        "one drop, one recovery"
    );
}

/// ❗ **With "Reconnect automatically" off, a server that comes back is left
/// alone until someone asks.**
///
/// The proxy counts every dial it accepts, so "nothing redialed" is a number,
/// not an absence of events. The window runs past the backoff's first step
/// (2 s), which is when a loop that shouldn't exist would have dialed.
#[tokio::test]
#[ignore = "needs the SFTP fixture stack: sftp-servers/start.sh (sftp-fixture)"]
async fn sftp_integration_with_the_switch_off_a_returning_server_stays_down_until_asked() {
    let Proxied { proxy, events, volume } = through_a_proxy().await;
    volume.set_auto_reconnect(false);

    proxy.refuse().await;
    let failed = tokio::time::timeout(ANSWERS_WITHIN, names(&volume))
        .await
        .expect("❗ an operation on a closed connection has to answer, not hang");
    assert!(matches!(failed, Err(VolumeError::DeviceDisconnected(_))), "{failed:?}");
    proxy.restore().await;
    let dials_before = proxy.connections_accepted();

    // allowed-test-sleep: a negative assertion over a window. Nothing can signal "no dial happened"; the window runs
    // past the backoff's first 2 s step, which is when a loop that shouldn't exist would have dialed.
    tokio::time::sleep(Duration::from_millis(2_500)).await;

    assert_eq!(
        proxy.connections_accepted(),
        dials_before,
        "❌ nothing may dial unattended with the switch off, however reachable the server is"
    );
    assert_eq!(state(&volume), Some(ConnectionState::Disconnected));
    assert!(
        matches!(volume.attempt_reconnect().await, Err(VolumeError::NotSupported)),
        "and an unattended ask is refused by the switch, not by the server"
    );

    // The user signs in by hand.
    volume
        .reconnect_with_credentials(cmdr_fs::volume::secret_store::SignIn {
            username: FIXTURE_USER.to_string(),
            secret: FIXTURE_PASSWORD.to_string(),
            remember: false,
        })
        .await
        .expect(FIXTURE);
    assert!(names(&volume).await.is_ok(), "an attended reconnect brings it back");
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected, VolumeConnection::Connected]
    );
}

/// ❗ **A server that goes SILENT is given up on at a deadline, not waited on
/// forever**, and then comes back like any other drop.
///
/// Nothing closes in a black hole, so no error ever arrives on its own: only a
/// timer can end the wait. The clock is paused for exactly the silent stretch,
/// so the production deadline elapses in virtual time and a missing one shows
/// up as the five-minute backstop firing rather than as a hung test.
///
/// ❗ Resumed before the proxy is restored and before anything else is awaited:
/// the backoff loop spawned by the failure mustn't burn through its steps in
/// virtual time against a server that's still silent.
#[tokio::test]
#[ignore = "needs the SFTP fixture stack: sftp-servers/start.sh (sftp-fixture)"]
async fn sftp_integration_a_server_that_goes_silent_is_given_up_on_and_comes_back() {
    let Proxied { proxy, events, volume } = through_a_proxy().await;
    let before = names(&volume).await.expect(FIXTURE);

    proxy.black_hole();
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let failed = tokio::time::timeout(Duration::from_secs(300), names(&volume)).await;
    let waited = started.elapsed();
    tokio::time::resume();

    let failed = failed.expect("❗ a listing on a silent server waited five minutes: nothing bounds it");
    assert!(
        matches!(failed, Err(VolumeError::DeviceDisconnected(_))),
        "a server given up on is a lost connection, which is what starts the backoff: {failed:?}"
    );
    assert!(
        waited <= SILENT_SERVER_DEADLINE + Duration::from_secs(1),
        "given up on after {waited:?}, past the {SILENT_SERVER_DEADLINE:?} a silent server gets"
    );
    assert_eq!(reported(&events), vec![VolumeConnection::Disconnected]);

    proxy.restore().await;

    wait_until_async(COMES_BACK_WITHIN, "the backoff loop to bring the volume back", || {
        reported(&events).contains(&VolumeConnection::Connected)
    })
    .await;
    assert_eq!(names(&volume).await.expect(FIXTURE), before);
}
