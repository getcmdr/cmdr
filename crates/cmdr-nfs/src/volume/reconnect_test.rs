//! "Reconnect automatically" against `fake_nfs.rs` on a paused clock: the switch
//! off dials nothing, a refused redial keeps backing off and never asks for a
//! sign-in, a transfer survives a new connection, and a start-folder edit moves
//! what the live volume dials with. Real drops through
//! `cmdr_fs::testing::tcp_proxy::TcpProxy` in front of the fixture are
//! `connection_drop_test.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::events::{RecordingVolumeEvents, VolumeConnection, VolumeEventSink};
use cmdr_fs::volume::{ConnectionState, Volume, VolumeError, VolumeReadStream};
use tokio_util::sync::CancellationToken;

use super::fake_nfs::{FakeNfs, proc};
use super::{Dialer, NfsVolume, connect_with};
use crate::dial::DialOptions;

/// A volume on `fake` over an in-memory pipe, its connection events recorded
/// and its every dial (the first one included) counted.
struct Watched {
    volume: NfsVolume,
    events: Arc<RecordingVolumeEvents>,
    dials: Arc<AtomicUsize>,
}

async fn watched(fake: &FakeNfs) -> Watched {
    let events = Arc::new(RecordingVolumeEvents::new());
    let host = VolumeHost::builder()
        .runtime(tokio::runtime::Handle::current())
        .events(Arc::clone(&events) as Arc<dyn VolumeEventSink>)
        .build();
    let dials = Arc::new(AtomicUsize::new(0));
    let through = fake.duplex_dialer(DialOptions::PRODUCTION);
    let counted = Arc::clone(&dials);
    let dialer = Dialer::scripted(DialOptions::PRODUCTION, move |params, cancel| {
        counted.fetch_add(1, Ordering::SeqCst);
        let through = through.clone();
        Box::pin(async move { through.first(&params, &cancel).await })
    });
    let volume = connect_with("fake", fake.params(), host, dialer, CancellationToken::new())
        .await
        .expect("the fake mounts");
    Watched { volume, events, dials }
}

/// The states a volume reported, in order.
fn reported(events: &RecordingVolumeEvents) -> Vec<VolumeConnection> {
    events.transitions().into_iter().map(|(_, state)| state).collect()
}

/// `remote` as a pane addresses it on `volume`.
fn at(volume: &NfsVolume, remote: &str) -> cmdr_fs::app_path::AppPathBuf {
    crate::volume::testing::app_path(volume, remote)
}

/// Every 16-byte line holds its own number, so any offset says where it
/// belongs.
fn numbered(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut line = 0u64;
    while out.len() < len {
        out.extend_from_slice(format!("{line:015}\n").as_bytes());
        line += 1;
    }
    out.truncate(len);
    out
}

/// Cuts the volume's connection the way a server going away mid-reply would:
/// the next `LOOKUP` reply is cut and the connection closes, so the call
/// that rides it is the detector.
async fn cut_under(fake: &FakeNfs, volume: &NfsVolume) {
    fake.reset_mid_reply(proc::LOOKUP);
    let failed = volume.get_metadata(&at(volume, "/big.bin")).await;
    assert!(
        matches!(failed, Err(VolumeError::DeviceDisconnected(_))),
        "a cut connection is a lost server: {failed:?}"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
}

/// Virtual time on a paused clock.
async fn wait(span: Duration) {
    // allowed-test-sleep: virtual time on a paused clock; the backoff's cadence is the thing under test.
    tokio::time::sleep(span).await;
}

async fn read_rest(stream: &mut dyn VolumeReadStream) -> Result<Vec<u8>, VolumeError> {
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

fn seeded() -> FakeNfs {
    let fake = FakeNfs::new();
    fake.add_dir("/photos");
    fake.add_file("/big.bin", &numbered(1 << 20));
    fake.set_transfer_size(16 * 1024);
    fake
}

/// ❗ **With "Reconnect automatically" off, a server that's back is left alone
/// until someone asks**: the dial count is the evidence, over a window past
/// every backoff step.
#[tokio::test(start_paused = true)]
async fn with_the_switch_off_nothing_dials_unattended() {
    let fake = seeded();
    let Watched { volume, events, dials } = watched(&fake).await;
    volume.set_auto_reconnect(false);

    cut_under(&fake, &volume).await;
    wait(Duration::from_secs(300)).await;

    assert_eq!(
        dials.load(Ordering::SeqCst),
        1,
        "❌ nothing redials with the switch off"
    );
    assert!(
        matches!(volume.attempt_reconnect().await, Err(VolumeError::NotSupported)),
        "an unattended ask is refused by the switch, ❌ not by the server"
    );
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    assert_eq!(reported(&events), vec![VolumeConnection::Disconnected]);

    // Switching it on brings the volume that's already down back on its own.
    volume.set_auto_reconnect(true);
    wait(Duration::from_secs(3)).await;
    assert_eq!(volume.connection_state(), Some(ConnectionState::Direct));
    assert_eq!(dials.load(Ordering::SeqCst), 2);
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected, VolumeConnection::Connected]
    );
}

/// ❗ **A redial the server refuses keeps backing off, and the volume never
/// asks for a sign-in**: there's no secret to ask for, so `NeedsCredentials`
/// would be a sign-in view with nothing to type. Once the server takes dials
/// again, the volume comes back on its own.
#[tokio::test(start_paused = true)]
async fn a_refused_redial_keeps_backing_off_and_comes_back_on_its_own() {
    let fake = seeded();
    let Watched { volume, events, dials } = watched(&fake).await;

    fake.refuse_dials(true);
    cut_under(&fake, &volume).await;
    // The backoff's first three steps: 2 s, 5 s, 15 s.
    wait(Duration::from_secs(25)).await;

    assert_eq!(
        dials.load(Ordering::SeqCst),
        1 + 3,
        "every step tried, ❌ none skipped as hopeless"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected],
        "❌ never NeedsCredentials: there is nothing to sign in with"
    );

    fake.refuse_dials(false);
    wait(Duration::from_secs(31)).await;

    assert_eq!(volume.connection_state(), Some(ConnectionState::Direct));
    assert_eq!(
        reported(&events),
        vec![VolumeConnection::Disconnected, VolumeConnection::Connected]
    );
    volume
        .list_directory(&at(&volume, "/photos"), None)
        .await
        .expect("the same volume lists again");
}

/// ❗ **A read in flight while the volume reconnects keeps reading on its own
/// connection to the end**, byte-exact, and the volume stays on the new one.
/// What the transfer engine leans on: work on the replaced connection finishes
/// there, and only the NEW generation reads as the volume's state.
#[tokio::test(start_paused = true)]
async fn a_read_in_flight_finishes_on_its_own_connection_across_a_reconnect() {
    let fake = seeded();
    let Watched { volume, .. } = watched(&fake).await;
    let mut stream = volume
        .open_read_stream(&at(&volume, "/big.bin"))
        .await
        .expect("the file opens");
    let first = stream.next_chunk().await.expect("a first chunk").expect("read");
    let generation = volume.connection_generation().expect("an NFS volume reports one");

    volume.simulate_session_loss().await;
    volume.attempt_reconnect().await.expect("no secret needed to redial");

    let mut read = first.to_vec();
    read.extend(read_rest(&mut *stream).await.expect("the old connection still answers"));
    assert_eq!(
        read,
        numbered(1 << 20),
        "byte-exact, ❌ no gap or repeat at the reconnect"
    );
    assert_eq!(volume.connection_generation(), Some(generation + 1));
    assert_eq!(volume.connection_state(), Some(ConnectionState::Direct));
}

/// ❗ **A read the server cut mid-file resumes on the next generation from
/// where it stopped**: the engine's outage wait, as the backend owes it. The
/// cut is a transport fault (so the engine waits rather than failing), the
/// next call is the detector, one reconnect installs a new generation, and a
/// stream opened at the offset reads the rest exactly.
#[tokio::test(start_paused = true)]
async fn a_read_cut_mid_file_resumes_on_the_next_generation_at_its_offset() {
    let fake = seeded();
    let Watched { volume, .. } = watched(&fake).await;
    let big = at(&volume, "/big.bin");
    let mut stream = volume.open_read_stream(&big).await.expect("the file opens");
    let mut read = stream
        .next_chunk()
        .await
        .expect("a first chunk")
        .expect("read")
        .to_vec();
    let generation = volume.connection_generation().expect("an NFS volume reports one");

    fake.reset_mid_reply(proc::READ);
    let cut = read_rest(&mut *stream).await;
    let fault = cut.as_ref().err().and_then(VolumeError::transport_fault);
    assert!(
        fault.is_some(),
        "a cut is the connection's fault, ❌ not the file's: {cut:?}"
    );
    drop(stream);

    // The engine's per-file retry is the detector: it reopens through `noting`.
    let reopened = volume.open_read_stream_at_offset(&big, read.len() as u64).await.err();
    assert!(
        matches!(reopened, Some(VolumeError::DeviceDisconnected(_))),
        "{reopened:?}"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    volume.attempt_reconnect().await.expect("the server is back");
    assert_eq!(volume.connection_generation(), Some(generation + 1));

    let mut rest = volume
        .open_read_stream_at_offset(&big, read.len() as u64)
        .await
        .expect("the file opens at its offset");
    read.extend(read_rest(&mut *rest).await.expect("the new connection answers"));
    assert_eq!(read, numbered(1 << 20), "resumed exactly where the cut left it");
}

/// A start-folder edit to a connected place moves what every instance on the
/// connection answers in its params, and what the next redial dials with, with
/// no redial of its own.
#[tokio::test(start_paused = true)]
async fn a_start_folder_edit_moves_the_params_every_instance_and_the_next_redial_see() {
    let fake = seeded();
    let Watched { volume, dials, .. } = watched(&fake).await;
    let renamed = volume.sharing_connection("renamed");

    volume.set_start_folder(Some("/photos".to_string()));

    assert_eq!(renamed.params().start_folder.as_deref(), Some("/photos"));
    assert_eq!(
        dials.load(Ordering::SeqCst),
        1,
        "❌ an edit to the folder never redials"
    );
    volume.simulate_session_loss().await;
    volume.attempt_reconnect().await.expect("no secret needed to redial");
    assert_eq!(volume.params().start_folder.as_deref(), Some("/photos"));
    assert!(volume.exists(&at(&volume, "/photos")).await);
}
