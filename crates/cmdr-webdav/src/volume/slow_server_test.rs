//! A server that's SLOW is not a server that's gone, and the silence watch
//! tells them apart (`cmdr_fs::volume::liveness`).
//!
//! These cells need a server that answers one request while it holds another,
//! trickles a body a byte at a time, or goes quiet on command, so they bring
//! their own: [`FakeDav`]. The silence ladder is shortened (`fake_dav::SHORT`)
//! so they run in real time; the production-length ladder is `cmdr-fs`'s
//! `volume/liveness_test.rs` (paused clock) and `connection_drop_test.rs` (a
//! real server behind a black-holed proxy).

use cmdr_fs::app_path::AppPath;
use std::time::{Duration, Instant};

use cmdr_fs::volume::host::events::VolumeConnection;
use cmdr_fs::volume::{ConnectionState, Volume, VolumeError};

use super::WebdavVolume;
use super::fake_dav::{FakeDav, Mode, SERVING};

/// When the short ladder gives up: quiet plus every probe's whole budget.
const SHORT_DEADLINE: Duration = Duration::from_millis(2_300);

/// How long the slow cells keep the server busy: well past `SHORT_DEADLINE`,
/// so a watch that mistook slow for gone would have fired in the middle.
const BUSY_FOR: Duration = Duration::from_millis(3_000);

async fn names(volume: &WebdavVolume) -> Result<Vec<String>, VolumeError> {
    Ok(volume
        .list_directory(volume.root(), None)
        .await?
        .into_iter()
        .map(|entry| entry.name)
        .collect())
}

/// ❗ **A listing the server takes its time over is waited for, and the volume
/// never reads as down on the way**: the huge folder on a slow NAS.
///
/// The server sends nothing for the listing at all for `BUSY_FOR`, longer
/// than the ladder, but it answers the watch's probes on fresh connections,
/// which is what "busy, not gone" looks like from here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_a_busy_server_takes_its_time_over_is_waited_for() {
    let server = FakeDav::start().await;
    let (volume, events) = server.connected().await;
    server.set(Mode {
        hold_listings: true,
        ..SERVING
    });

    let listing = tokio::spawn(async move {
        let listed = names(&volume).await;
        (listed, volume)
    });
    // allowed-test-sleep: the busy stretch IS the scenario; the server has to stay busy past the whole ladder.
    tokio::time::sleep(BUSY_FOR).await;
    server.set(SERVING);
    let (listed, volume) = listing.await.expect("the listing task");

    assert_eq!(listed.expect("a busy server's listing arrives"), vec!["slow.txt"]);
    assert!(
        events.transitions().is_empty(),
        "❌ never a disconnect for a server that's busy"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Direct));
    assert!(
        server.probes_answered() >= 2,
        "the watch did ask, and got its answers: {} probes",
        server.probes_answered()
    );
}

/// ❗ **A body that keeps moving is never cut, even when the probe would go
/// unanswered**: bytes on the wire are the answer already, so a slow read
/// can't be mistaken for a lost server whatever the probe says.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_that_keeps_moving_is_never_cut() {
    let server = FakeDav::start().await;
    let (volume, events) = server.connected().await;
    server.set(Mode {
        answer_probes: false,
        ..SERVING
    });
    let started = Instant::now();

    let read = volume
        .read_range(
            &AppPath::legacy_unchecked(&volume.root().as_std_legacy().join("slow.txt")),
            0,
            10,
        )
        .await;

    assert_eq!(read.expect("a trickling body arrives whole"), b"slow bytes");
    assert!(
        started.elapsed() > SHORT_DEADLINE,
        "the read outlasted the ladder ({:?}), or it proves nothing",
        started.elapsed()
    );
    assert!(events.transitions().is_empty());
    assert_eq!(server.probes_answered(), 0, "nothing was ever quiet long enough to ask");
}

/// ❗ **A server that goes silent cuts every waiting operation at the ladder's
/// end with `DeviceDisconnected`, and the volume reports it ONCE.** The
/// production-length twin against a real server is in
/// `connection_drop_test.rs`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_server_cuts_every_waiting_operation_and_reports_once() {
    let server = FakeDav::start().await;
    let (volume, events) = server.connected().await;
    volume.set_auto_reconnect(false);
    server.set(Mode {
        silent: true,
        ..SERVING
    });
    let started = Instant::now();

    let (first, second) = tokio::join!(names(&volume), names(&volume));
    let waited = started.elapsed();

    for listed in [&first, &second] {
        assert!(matches!(listed, Err(VolumeError::DeviceDisconnected(_))), "{listed:?}");
    }
    assert!(
        waited >= SHORT_DEADLINE - Duration::from_millis(100) && waited < SHORT_DEADLINE * 2,
        "cut after {waited:?}; the ladder ends at {SHORT_DEADLINE:?}"
    );
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    assert_eq!(
        events
            .transitions()
            .into_iter()
            .map(|(_, state)| state)
            .collect::<Vec<_>>(),
        vec![VolumeConnection::Disconnected],
        "two operations found it gone, one report"
    );
}
