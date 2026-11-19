//! The change poller against `fake_nfs.rs` on a paused clock: a change another
//! client made is seen within one tick, one `GETATTR` per shown directory per
//! tick and nothing else, nothing at all with no pane open or while the volume
//! is down, and nothing for a directory no pane shows any more.

use cmdr_fs::app_path::AppPathBuf;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::listings::{ListingHost, RecordingListings};
use cmdr_fs::volume::{ConnectionState, DirectoryChange, Volume, VolumeError};
use tokio_util::sync::CancellationToken;

use super::POLL_EVERY;
use crate::dial::DialOptions;
use crate::volume::fake_nfs::{FakeNfs, proc};
use crate::volume::{NfsVolume, connect_with};

/// A volume on `fake` over an in-memory pipe, its panes answered by `panes`.
async fn shown_through(fake: &FakeNfs, panes: &Arc<RecordingListings>) -> NfsVolume {
    let host = VolumeHost::builder()
        .runtime(tokio::runtime::Handle::current())
        .listings(Arc::clone(panes) as Arc<dyn ListingHost>)
        .build();
    connect_with(
        "fake",
        fake.params(),
        host,
        fake.duplex_dialer(DialOptions::PRODUCTION),
        CancellationToken::new(),
    )
    .await
    .expect("the fake mounts")
}

fn seeded() -> FakeNfs {
    let fake = FakeNfs::new();
    fake.add_dir("/photos");
    fake.add_file("/photos/a.jpg", b"a");
    fake.add_dir("/docs");
    fake
}

/// `remote` as a pane addresses it on `volume`.
fn at(volume: &NfsVolume, remote: &str) -> AppPathBuf {
    crate::volume::testing::app_path(volume, remote)
}

/// Where the store files a pane on `remote`: the listing host still keys
/// listings by `std::path`, spelled as the text the pane holds.
fn filed(volume: &NfsVolume, remote: &str) -> PathBuf {
    PathBuf::from(at(volume, remote).into_string())
}

/// A pane opens on `remote`: the volume lists it, and the store files it.
async fn open_pane(volume: &NfsVolume, panes: &RecordingListings, remote: &str) -> PathBuf {
    volume
        .list_directory(&at(volume, remote), None)
        .await
        .expect("the folder lists");
    let filed = filed(volume, remote);
    panes.open_path(volume.volume_id(), &filed);
    filed
}

/// One poll interval and a little, in virtual time.
async fn a_tick() {
    // allowed-test-sleep: virtual time on a paused clock; the poll interval is the thing under test.
    tokio::time::sleep(POLL_EVERY + Duration::from_millis(100)).await;
}

/// The full refreshes reported, by path.
fn refreshed(panes: &RecordingListings) -> Vec<PathBuf> {
    panes
        .changes()
        .into_iter()
        .filter(|(_, _, change)| matches!(change, DirectoryChange::FullRefresh))
        .map(|(_, path, _)| path)
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_change_another_client_made_is_seen_within_one_tick_and_reported_once() {
    let fake = seeded();
    let panes = Arc::new(RecordingListings::new());
    let volume = shown_through(&fake, &panes).await;
    let photos = open_pane(&volume, &panes, "/photos").await;

    // Between the listing and the first tick: the listing's own stamp is the
    // baseline, so this is a change, ❌ never the starting point.
    fake.add_file("/photos/b.jpg", b"b");
    a_tick().await;

    assert_eq!(refreshed(&panes), std::slice::from_ref(&photos), "seen within one tick");
    a_tick().await;
    assert_eq!(
        refreshed(&panes),
        [photos],
        "one change, one refresh, ❌ not one per tick"
    );
}

#[tokio::test(start_paused = true)]
async fn a_tick_is_one_getattr_per_shown_directory_and_nothing_else() {
    let fake = seeded();
    let panes = Arc::new(RecordingListings::new());
    let volume = shown_through(&fake, &panes).await;
    open_pane(&volume, &panes, "/photos").await;
    open_pane(&volume, &panes, "/docs").await;
    open_pane(&volume, &panes, "").await;
    let before = fake.total_calls();
    let getattrs = fake.calls(proc::GETATTR);

    a_tick().await;
    a_tick().await;

    assert_eq!(fake.calls(proc::GETATTR) - getattrs, 6, "three directories, two ticks");
    assert_eq!(
        fake.total_calls() - before,
        6,
        "❌ no LOOKUP, no listing, nothing per entry: the handles came from the listings"
    );
    assert!(refreshed(&panes).is_empty(), "nothing moved, so nothing re-lists");
}

#[tokio::test(start_paused = true)]
async fn with_no_pane_open_nothing_is_sent_and_the_poller_stops() {
    let fake = seeded();
    let panes = Arc::new(RecordingListings::new());
    let volume = shown_through(&fake, &panes).await;
    // A scan or a copy lists without a pane: the store shows nothing.
    volume
        .list_directory(&at(&volume, "/photos"), None)
        .await
        .expect("the folder lists");
    let before = fake.total_calls();

    for _ in 0..10 {
        a_tick().await;
    }

    assert_eq!(fake.total_calls(), before, "❌ no traffic with nothing shown");
    let photos = open_pane(&volume, &panes, "/photos").await;
    fake.touch("/photos");
    a_tick().await;
    assert_eq!(refreshed(&panes), [photos], "the next pane's listing starts it again");
}

#[tokio::test(start_paused = true)]
async fn a_change_for_a_directory_no_pane_shows_any_more_is_dropped() {
    let fake = seeded();
    let panes = Arc::new(RecordingListings::new());
    let volume = shown_through(&fake, &panes).await;
    let photos = open_pane(&volume, &panes, "/photos").await;
    let docs = open_pane(&volume, &panes, "/docs").await;

    fake.touch("/photos");
    fake.touch("/docs");
    panes.close_path(volume.volume_id(), &photos);
    a_tick().await;

    assert_eq!(refreshed(&panes), [docs], "only the directory a pane still shows");
    let before = fake.calls(proc::GETATTR);
    panes.close_path(volume.volume_id(), &filed(&volume, "/docs"));
    a_tick().await;
    a_tick().await;
    assert_eq!(
        fake.calls(proc::GETATTR),
        before,
        "the last pane closed: nothing more is asked"
    );
}

#[tokio::test(start_paused = true)]
async fn nothing_is_sent_while_the_volume_is_down() {
    let fake = seeded();
    let panes = Arc::new(RecordingListings::new());
    let volume = shown_through(&fake, &panes).await;
    volume.set_auto_reconnect(false);
    open_pane(&volume, &panes, "/photos").await;

    // The server goes silent: the next tick's GETATTR is what finds it gone,
    // inside the silence ladder, and the volume goes down.
    fake.go_silent(true);
    for _ in 0..8 {
        a_tick().await;
    }
    assert_eq!(volume.connection_state(), Some(ConnectionState::Disconnected));
    assert!(
        matches!(
            volume.list_directory(&at(&volume, "/photos"), None).await,
            Err(VolumeError::DeviceDisconnected(_))
        ),
        "down means down"
    );
    let before = fake.total_calls();

    for _ in 0..10 {
        a_tick().await;
    }

    assert_eq!(
        fake.total_calls(),
        before,
        "❌ never a GETATTR on a volume that's down, on top of whatever brings it back"
    );
}
