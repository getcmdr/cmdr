//! A server that REALLY goes away under a live volume, and comes back.
//!
//! `reconnect_test.rs` drives the policy against `fake_nfs.rs`; these cells cut
//! the actual TCP connection to the knfsd fixture instead, which is what a
//! person hits: a NAS that sleeps or reboots, Wi-Fi that drops.
//!
//! ❗ The cut happens in a `cmdr_fs::testing::tcp_proxy::TcpProxy` this test
//! owns, in front of the fixture's NFS port. ❌ Never pause or stop the
//! container: the stack is shared by lease with other test binaries,
//! worktrees, and sessions, and every one of them would see the outage too.
//! MOUNT and portmap stay direct (the dial takes MOUNT's port from portmap, and
//! an explicit NFS port skips only the NFS lookup), so a redial reaches the
//! proxy exactly where the volume's calls and its `NULL` probe do.
//!
//! Two ways down, because they end differently:
//!
//! - **Refused**: the connection closes and a new one is refused, so the next
//!   call answers `DeviceDisconnected` at once.
//! - **Silent** (a black hole): nothing closes and nothing answers, so only the
//!   silence ladder ends the wait: 5 s quiet, then two 5 s `NULL` probes on a
//!   fresh connection. It runs on a paused clock, so the deadline elapses in
//!   virtual time.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::testing::tcp_proxy::TcpProxy;
use cmdr_fs::testing::wait_until_async;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::events::{RecordingVolumeEvents, VolumeConnection, VolumeEventSink};
use cmdr_fs::volume::{ConnectionState, Volume, VolumeError};
use tokio_util::sync::CancellationToken;

use super::testing::*;
use super::{NfsVolume, connect_nfs_volume};

const FIXTURE: &str = "nfs-servers/start.sh (nfs-fixture)";

/// How long a silent server keeps a waiting call before the volume is
/// reported down: NFS's own ladder, 5 s of quiet and two 5 s probes.
const SILENCE_DEADLINE: Duration = Duration::from_secs(15);

/// How long a call on a REFUSED connection may take to answer. Generous: the
/// real answer is milliseconds, and this is a hang backstop that keeps the
/// failure message ours rather than nextest's.
const ANSWERS_WITHIN: Duration = Duration::from_secs(3);

/// How long the backend's own loop gets to bring a volume back: its first
/// backoff step (2 s) plus a whole dial, under the 8 s nextest cap.
const COMES_BACK_WITHIN: Duration = Duration::from_secs(6);

/// A stable folder of the seeded tree. ❌ Not the root: the write cells create
/// and remove scratch folders there while these run.
const LISTED: &str = "/photos";

/// A volume on the fixture's `/insecure` export, its NFS port THROUGH a proxy
/// this test owns.
struct Proxied {
    proxy: TcpProxy,
    events: Arc<RecordingVolumeEvents>,
    volume: NfsVolume,
}

/// Connects through a fresh proxy in front of the fixture's NFS port.
///
/// ❗ The host runs background work on THIS runtime (`Handle::current`), so a
/// paused clock pauses the backoff loop along with the proxy and the call.
async fn through_a_proxy() -> Proxied {
    let nfs_port = std::env::var("NFS_FIXTURE_NFS_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(15481);
    let proxy = TcpProxy::start(SocketAddr::from(([127, 0, 0, 1], nfs_port))).await;
    let mut params = fixture_params(FIXTURE_INSECURE_EXPORT);
    params.server.port = Some(proxy.port());
    let events = Arc::new(RecordingVolumeEvents::new());
    let host = VolumeHost::builder()
        .runtime(tokio::runtime::Handle::current())
        .events(Arc::clone(&events) as Arc<dyn VolumeEventSink>)
        .build();
    let volume = connect_nfs_volume("fixture", params, host, CancellationToken::new())
        .await
        .unwrap_or_else(|err| panic!("the proxied fixture refused a connection ({err:?}); is the stack up? {FIXTURE}"));
    Proxied { proxy, events, volume }
}

/// The listed folder's names, sorted, or the error the listing answered.
async fn names(volume: &NfsVolume) -> Result<Vec<String>, VolumeError> {
    let mut names: Vec<String> = volume
        .list_directory(&app_path(volume, LISTED), None)
        .await?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    Ok(names)
}

/// The states a volume reported, in order.
fn reported(events: &RecordingVolumeEvents) -> Vec<VolumeConnection> {
    events.transitions().into_iter().map(|(_, state)| state).collect()
}

/// Lists while the server refuses: what the listing answered, bounded.
async fn list_while_refused(volume: &NfsVolume) -> Result<Vec<String>, VolumeError> {
    tokio::time::timeout(ANSWERS_WITHIN, names(volume))
        .await
        .expect("❗ a call to a refusing server has to answer, not hang")
}

/// Silences the proxy and lists, on a paused clock: what the listing answered
/// and how much virtual time it took. Resumed before returning: nothing after
/// it may run on a clock that jumps whenever the runtime waits on the network.
async fn list_through_silence(proxy: &TcpProxy, volume: &NfsVolume) -> (Result<Vec<String>, VolumeError>, Duration) {
    proxy.black_hole();
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let failed = tokio::time::timeout(Duration::from_secs(300), names(volume)).await;
    let waited = started.elapsed();
    tokio::time::resume();
    let failed = failed.expect("❗ a listing on a silent server waited five minutes: nothing bounds it");
    (failed, waited)
}

/// Asserts the silent listing ended the way a refused one does, at the
/// silence deadline and not before: a paused clock fires each timer exactly at
/// its deadline, so the whole ladder shows up in `waited`.
fn assert_given_up_at_the_deadline(failed: &Result<Vec<String>, VolumeError>, waited: Duration) {
    assert!(
        matches!(failed, Err(VolumeError::DeviceDisconnected(_))),
        "❗ a silent server is a lost connection, the one error that flips the state and starts the backoff: \
         {failed:?}"
    );
    assert!(
        waited >= SILENCE_DEADLINE - Duration::from_secs(1) && waited <= SILENCE_DEADLINE + Duration::from_secs(1),
        "given up on after {waited:?}; the silence deadline is {SILENCE_DEADLINE:?}"
    );
}

/// ❗ **A server that goes away is reported down at once, with a typed error,
/// and comes back on its own once it's reachable again.**
///
/// A retry while it's still gone answers the same way and reports nothing new:
/// a refused redial is transient, ❌ never a sign-in (there's no secret).
#[tokio::test]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_server_that_goes_away_is_reported_down_and_comes_back_on_its_own() {
    let Proxied { proxy, events, volume } = through_a_proxy().await;
    let before = names(&volume).await.expect(FIXTURE);

    proxy.refuse().await;

    let failed = list_while_refused(&volume).await;
    assert!(
        matches!(failed, Err(VolumeError::DeviceDisconnected(_))),
        "only DeviceDisconnected flips the state and starts the backoff, got {failed:?}"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    assert_eq!(reported(&events), vec![VolumeConnection::Disconnected]);

    // The frontend's own retry, while the server is still gone.
    let retry = tokio::time::timeout(ANSWERS_WITHIN, volume.attempt_reconnect())
        .await
        .expect("a redial of a refusing server answers, not hangs");
    assert!(
        matches!(retry, Err(VolumeError::DeviceDisconnected(_))),
        "a refused redial is transient, never a sign-in: {retry:?}"
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
    assert_eq!(volume.connection_state(), Some(ConnectionState::Direct));
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

/// ❗ **A server that goes SILENT is reported down inside the silence ladder,
/// once, and comes back on its own once the path clears**: the refused cell's
/// promise, reached without anything ever closing. The 60 s a listing may take
/// never gets to decide.
#[tokio::test]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn a_server_that_goes_silent_is_given_up_inside_the_ladder_and_comes_back() {
    let Proxied { proxy, events, volume } = through_a_proxy().await;
    let before = names(&volume).await.expect(FIXTURE);

    let (failed, waited) = list_through_silence(&proxy, &volume).await;

    assert_given_up_at_the_deadline(&failed, waited);
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    assert_eq!(reported(&events), vec![VolumeConnection::Disconnected]);

    proxy.restore().await;

    wait_until_async(COMES_BACK_WITHIN, "the backoff loop to bring the volume back", || {
        reported(&events).contains(&VolumeConnection::Connected)
    })
    .await;
    assert_eq!(volume.connection_state(), Some(ConnectionState::Direct));
    assert_eq!(
        names(&volume).await.expect(FIXTURE),
        before,
        "the same volume lists again"
    );
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected, VolumeConnection::Connected],
        "one silence, one drop, one recovery"
    );
}

/// After a drop with the switch off, `connections_before` connections and a
/// window past the backoff's first 2 s step: nothing may have dialed, and the
/// switch, not the server, refuses an unattended ask. Switching it on is what
/// a person does, and it brings the volume back.
async fn assert_stays_down_until_switched_on(proxied: &Proxied) {
    let Proxied { proxy, events, volume } = proxied;
    let connections_before = proxy.connections_accepted();

    // allowed-test-sleep: a negative assertion over a window. Nothing can signal "no dial happened"; the window runs
    // past the backoff's first 2 s step, which is when a loop that shouldn't exist would have dialed.
    tokio::time::sleep(Duration::from_millis(2_500)).await;

    assert_eq!(
        proxy.connections_accepted(),
        connections_before,
        "❌ nothing may dial unattended with the switch off, however reachable the server is"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    assert!(
        matches!(volume.attempt_reconnect().await, Err(VolumeError::NotSupported)),
        "an unattended ask is refused by the switch, not by the server"
    );

    volume.set_auto_reconnect(true);
    wait_until_async(COMES_BACK_WITHIN, "switching it on to bring the volume back", || {
        reported(events).contains(&VolumeConnection::Connected)
    })
    .await;
    assert!(names(volume).await.is_ok(), "the person's switch brings it back");
    assert_eq!(
        reported(events),
        vec![VolumeConnection::Disconnected, VolumeConnection::Connected]
    );
}

/// ❗ **With "Reconnect automatically" off, a refused server that comes back is
/// left alone until someone asks.** The proxy counts every connection it
/// accepts, so "nothing dialed" is a number, ❌ not an absence of events.
#[tokio::test]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn with_the_switch_off_a_returning_server_stays_down_until_asked() {
    let proxied = through_a_proxy().await;
    proxied.volume.set_auto_reconnect(false);

    proxied.proxy.refuse().await;
    let failed = list_while_refused(&proxied.volume).await;
    assert!(matches!(failed, Err(VolumeError::DeviceDisconnected(_))), "{failed:?}");
    proxied.proxy.restore().await;

    assert_stays_down_until_switched_on(&proxied).await;
}

/// ❗ **With "Reconnect automatically" off, a silent server that comes back is
/// left alone until someone asks**, exactly like a refused one.
#[tokio::test]
#[ignore = "needs the NFS fixture stack: nfs-servers/start.sh (nfs-fixture)"]
async fn with_the_switch_off_a_server_back_from_silence_stays_down_until_asked() {
    let proxied = through_a_proxy().await;
    proxied.volume.set_auto_reconnect(false);

    let (failed, waited) = list_through_silence(&proxied.proxy, &proxied.volume).await;
    assert_given_up_at_the_deadline(&failed, waited);
    // allowed-test-sleep: the ladder's last probe dialed DURING the silence, and on the paused clock its budget can run
    // out before the proxy's accept loop gets to it, leaving the dial in the listen backlog. This lets the proxy count
    // it before the baseline is taken; a dial made after the silence is what the window is about.
    tokio::time::sleep(Duration::from_millis(200)).await;
    proxied.proxy.restore().await;

    assert_stays_down_until_switched_on(&proxied).await;
}
