//! The transfer session against a real server: transfers ride it, browsing
//! doesn't, it opens lazily and closes when idle, a loss falls back to the main
//! session without moving the share's state, and the watchdog reads it.
//!
//! The Docker cells are `#[ignore]`d so default runs skip them. Start the
//! containers with `apps/desktop/test/smb-servers/start.sh`, then run
//! `cargo nextest run smb_integration --run-ignored all`.

use super::*;
use crate::volume::SmbVolume;
use crate::volume::state::{ConnectionState, RanOn};
use crate::volume::test_support::*;
use cmdr_fs::testing::wait_until_async;
use cmdr_fs::volume::{StreamLength, WriteMode};

/// Bigger than one compound frame on a fresh connection (one 512 KiB chunk),
/// so both directions stream.
const STREAMED: usize = 3 * 1024 * 1024;

/// The volume's main connection, as the liveness handle holds it.
fn main_conn(vol: &SmbVolume) -> Connection {
    vol.inner
        .live_connection
        .current()
        .expect("a connected volume has a main connection")
}

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

async fn upload(vol: &SmbVolume, path: &str, bytes: Vec<u8>) {
    let len = bytes.len() as u64;
    let written = vol
        .write_from_stream(
            &app_at(vol.root(), path),
            WriteMode::CreateOrReplace,
            StreamLength::Known(len),
            inline_read_stream(bytes),
            &|_| std::ops::ControlFlow::Continue(()),
        )
        .await
        .expect("the upload lands");
    assert_eq!(written.bytes, len);
}

#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_download_rides_the_transfer_session_and_browsing_stays_on_main() {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    vol.create_directory(&app_at(vol.root(), &dir)).await.unwrap();
    let file = format!("{dir}/big.bin");
    let data = payload(STREAMED);
    // Seeded through `create_file`, a mutation, which stays on the main session.
    vol.create_file(&app_at(vol.root(), &file), &data).await.unwrap();
    assert!(
        vol.inner.transfer.member_for_test().is_none(),
        "nothing opens a transfer session before a transfer"
    );

    let main_before = main_conn(&vol).diagnostics().metrics.wire_bytes_received;
    let stream = vol.open_read_stream(&app_at(vol.root(), &file)).await.unwrap();
    let member = vol
        .inner
        .transfer
        .member_for_test()
        .expect("a download opens the transfer session");
    let main_id = vol.inner.session.lock().await.as_ref().unwrap().client.session_id();
    assert_ne!(
        member.session_id_for_test().await,
        main_id,
        "the transfer session is a session of its own"
    );
    assert_eq!(
        vol.inner.transfer.active_for_test(),
        1,
        "the open stream holds the session"
    );

    // Browsing mid-download answers on the main session.
    let listed = vol.list_directory(&app_at(vol.root(), &dir), None).await.unwrap();
    assert!(listed.iter().any(|e| e.name == "big.bin"));

    assert_eq!(drain(stream).await, data, "the bytes come through whole");
    let main_moved = main_conn(&vol).diagnostics().metrics.wire_bytes_received - main_before;
    assert!(
        main_moved < 256 * 1024,
        "the main connection carried too much of the download: {main_moved} of {STREAMED}"
    );
    wait_until_async(
        Duration::from_secs(5),
        "the finished download releases its hold",
        || vol.inner.transfer.active_for_test() == 0,
    )
    .await;

    ensure_clean(&vol, &dir).await;
}

#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_uploads_ride_the_transfer_session_streamed_and_single_frame() {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    vol.create_directory(&app_at(vol.root(), &dir)).await.unwrap();

    let main_before = main_conn(&vol).diagnostics().metrics.wire_bytes_sent;
    let big = payload(STREAMED);
    upload(&vol, &format!("{dir}/big.bin"), big.clone()).await;
    let small = payload(4096);
    upload(&vol, &format!("{dir}/small.bin"), small.clone()).await;
    let main_moved = main_conn(&vol).diagnostics().metrics.wire_bytes_sent - main_before;
    assert!(
        main_moved < 256 * 1024,
        "the main connection sent too much of the uploads: {main_moved}"
    );
    let transfer = vol
        .inner
        .transfer
        .live
        .current()
        .expect("the uploads opened a transfer session");
    assert!(
        transfer.diagnostics().metrics.wire_bytes_sent >= STREAMED as u64,
        "the transfer session carried the uploads"
    );

    let big_back = drain(
        vol.open_read_stream(&app_at(vol.root(), &format!("{dir}/big.bin")))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(big_back, big);
    let small_back = drain(
        vol.open_read_stream_with_hint(&app_at(vol.root(), &format!("{dir}/small.bin")), Some(4096))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(small_back, small);

    ensure_clean(&vol, &dir).await;
}

#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_the_transfer_session_closes_when_idle_and_reopens_on_demand() {
    let vol = make_docker_volume().await;
    vol.inner.transfer.set_idle_for_test(Duration::from_millis(300));
    let dir = test_dir_name();
    vol.create_directory(&app_at(vol.root(), &dir)).await.unwrap();

    upload(&vol, &format!("{dir}/one.bin"), payload(STREAMED)).await;
    let first = vol.inner.transfer.member_for_test().expect("open after a transfer");
    let first_id = first.session_id_for_test().await;
    drop(first);
    // A transfer right after reuses it: no churn inside the idle window.
    upload(&vol, &format!("{dir}/two.bin"), payload(4096)).await;
    let again = vol.inner.transfer.member_for_test().expect("still open");
    assert_eq!(
        again.session_id_for_test().await,
        first_id,
        "a burst shares one session"
    );
    drop(again);

    wait_until_async(Duration::from_secs(5), "the idle session closes", || {
        vol.inner.transfer.is_closed_for_test()
    })
    .await;
    assert!(!vol.inner.transfer.is_up());
    assert!(
        vol.inner.transfer.live.current().is_none(),
        "the watchdog stops reading a closed session"
    );

    upload(&vol, &format!("{dir}/three.bin"), payload(STREAMED)).await;
    assert!(
        vol.inner.transfer.member_for_test().is_some(),
        "the next transfer opens a fresh one"
    );

    ensure_clean(&vol, &dir).await;
}

#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_lost_transfer_session_falls_back_to_main_and_reconnects() {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    vol.create_directory(&app_at(vol.root(), &dir)).await.unwrap();
    upload(&vol, &format!("{dir}/one.bin"), payload(4096)).await;
    let generation = vol.inner.transfer.generation_for_test().expect("open after a transfer");
    let main_generation = Volume::connection_generation(&vol);

    // What a transfer's call reports when its connection dies.
    let ends = vol
        .inner
        .note_error(RanOn::Transfer(generation), &smb2::Error::Disconnected);
    assert!(ends, "a dropped connection ends the session it was on");
    assert_eq!(
        vol.session_state(),
        ConnectionState::Direct,
        "a transfer session's loss never moves the share's state"
    );
    assert_eq!(Volume::connection_generation(&vol), main_generation);
    assert!(vol.inner.transfer.is_down_for_test());

    // The retry rides the main session meanwhile.
    let lease = vol.transfer_lease().await.unwrap();
    assert!(
        matches!(lease.ran_on, RanOn::Main(_)),
        "a down transfer session falls back"
    );
    drop(lease);
    upload(&vol, &format!("{dir}/two.bin"), payload(STREAMED)).await;

    // A late loss from the replaced session moves nothing once it's back.
    wait_until_async(Duration::from_secs(10), "the transfer session reconnects", || {
        vol.inner.transfer.is_up()
    })
    .await;
    let back = vol.inner.transfer.generation_for_test().expect("back up");
    assert_ne!(back, generation);
    vol.inner
        .note_error(RanOn::Transfer(generation), &smb2::Error::Disconnected);
    assert!(
        vol.inner.transfer.is_up(),
        "a replaced session's late loss moves nothing"
    );

    ensure_clean(&vol, &dir).await;
}

#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_refused_transfer_session_leaves_copies_on_main() {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    vol.create_directory(&app_at(vol.root(), &dir)).await.unwrap();
    // Nothing listens on port 1: the open is refused at once, as a server
    // capping sessions per user would.
    vol.inner.params.write().await.port = 1;

    let data = payload(STREAMED);
    upload(&vol, &format!("{dir}/big.bin"), data.clone()).await;
    assert!(
        vol.inner.transfer.is_down_for_test(),
        "the refused open leaves the slot down"
    );
    assert_eq!(vol.session_state(), ConnectionState::Direct);
    let back = drain(
        vol.open_read_stream(&app_at(vol.root(), &format!("{dir}/big.bin")))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(back, data, "the copy never fails over a refused transfer session");

    ensure_clean(&vol, &dir).await;
}

#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_the_watchdog_reads_the_transfer_session_while_it_is_up() {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    vol.create_directory(&app_at(vol.root(), &dir)).await.unwrap();
    let file = format!("{dir}/big.bin");
    vol.create_file(&app_at(vol.root(), &file), &payload(STREAMED))
        .await
        .unwrap();

    assert_eq!(
        vol.connection_bytes_received(),
        Some(main_conn(&vol).inbound().bytes_received),
        "with no transfer session, the main one answers"
    );
    let stream = vol.open_read_stream(&app_at(vol.root(), &file)).await.unwrap();
    drain(stream).await;
    let transfer = vol.inner.transfer.live.current().expect("open after a download");
    assert_eq!(
        vol.connection_bytes_received(),
        Some(transfer.inbound().bytes_received),
        "the receive rate is the transfer session's"
    );
    assert!(vol.connection_bytes_received().unwrap() >= STREAMED as u64);

    ensure_clean(&vol, &dir).await;
}

/// Server-free: a share that isn't `Direct` hands out no lease on either
/// session, so a transfer's outage wait sees the share's own state.
#[tokio::test]
async fn a_disconnected_share_leases_nothing() {
    let vol = make_test_volume();
    let result = vol.transfer_lease().await;
    assert!(matches!(result, Err(VolumeError::DeviceDisconnected(_))));
    assert!(vol.inner.transfer.is_closed_for_test(), "and opens nothing");
}

/// Server-free: an unmounted share's transfer session opens nothing again.
#[tokio::test]
async fn an_unmounted_share_opens_no_transfer_session() {
    let vol = make_test_volume_direct();
    vol.inner.transfer.close();
    assert!(vol.inner.transfer.lease().await.is_none());
    assert!(!vol.inner.transfer.is_up());
}
