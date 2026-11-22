//! The two resolution questions every transfer asks before it starts.
//!
//! Both answers are load-bearing and neither is obvious from its inputs: an
//! unanchored destination reaches a share as a path outside its mount, and a
//! `.zip` FILE routed to its own `ArchiveVolume` would scan the archive's contents
//! instead of copying the file.
//!
//! The routing these cells pin is the app registry's (an archive, the git
//! portal, an id nothing knows), which `FakeVolumes` doesn't route through, so
//! each cell builds an engine of its own over the app's answers
//! (`operations_host::test_engine`).

use super::resolve_dest_path;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use std::path::Path;
use std::sync::Arc;

/// A non-local volume (no `local_path`) rooted where a mounted share is: stands in
/// for `SmbVolume` without a network.
fn share_at(root: &str) -> Arc<dyn Volume> {
    Arc::new(InMemoryVolume::new("TestShare").with_root(root))
}

#[test]
fn a_remote_destination_is_anchored_at_its_mount_root() {
    // The dialog's box is volume-relative. Unanchored, `/test_photos` reaches
    // `SmbVolume` as an absolute path outside the mount and comes back
    // `NotFound` before any I/O, which is what killed a 360 GB move
    // (ERR-XCP5Q) and reported the DESTINATION as a missing source.
    let volume = share_at("/Volumes/testshare");
    assert_eq!(
        resolve_dest_path(&volume, "/test_photos/test footage".to_string()),
        Path::new("/Volumes/testshare/test_photos/test footage")
    );
}

#[test]
fn a_remote_destination_already_under_its_root_is_untouched() {
    let volume = share_at("/Volumes/testshare");
    assert_eq!(
        resolve_dest_path(&volume, "/Volumes/testshare/test_photos".to_string()),
        Path::new("/Volumes/testshare/test_photos")
    );
}

#[test]
fn a_remote_destination_at_the_share_root_stays_the_root() {
    // The one shape that worked before anchoring existed; it must keep
    // working, and must not become `/Volumes/testshare/`.
    let volume = share_at("/Volumes/testshare");
    assert_eq!(
        resolve_dest_path(&volume, "/".to_string()),
        Path::new("/Volumes/testshare")
    );
}

#[test]
fn a_local_destination_still_expands_the_home_shortcut() {
    let volume: Arc<dyn Volume> = Arc::new(cmdr_local::LocalVolume::local_folder(
        "Root",
        "/",
        crate::host::testing::pane_order_host(),
    ));
    let home = std::env::var("HOME").expect("HOME");
    assert_eq!(
        resolve_dest_path(&volume, "~/Downloads".to_string()),
        Path::new(&home).join("Downloads")
    );
}

/// A `NotConnected` a backend answers mid-transfer names the half that asked, the
/// same way an unregistered volume does before the transfer starts. ❌ Never
/// `DeviceDisconnected`: nothing dropped.
#[test]
fn a_volume_that_isnt_connected_yet_maps_to_the_side_that_asked() {
    use crate::WriteOperationError;
    use crate::transfer::volume::{PathRole, map_volume_error};
    use cmdr_fs::volume::VolumeError;

    let not_connected = || VolumeError::NotConnected("adb://R58M/sdcard".to_string());
    assert!(matches!(
        map_volume_error("adb://R58M/sdcard", PathRole::Destination, not_connected()),
        WriteOperationError::DestinationNotConnected { .. }
    ));
    assert!(matches!(
        map_volume_error("adb://R58M/sdcard", PathRole::Source, not_connected()),
        WriteOperationError::SourceNotConnected { .. }
    ));
}

/// A copy and a move into a `.zip` on a volume rooted BELOW `/` route to the archive
/// driver when the destination arrives volume-relative, as the transfer dialog's box
/// sends it: the routes read the anchored path (#1369). Before, `/box.zip/sub` on a
/// drive rooted at `C:/` confirmed no zip and fell through to the local engine,
/// which refused the zip as `destination_not_a_folder`. The per-source leaf refusal
/// is the probe: only the archive route says `RouteCannotName`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_relative_destination_still_routes_into_its_zip() {
    use crate::event_sinks::{CollectorEventSink, OperationEventSink};
    use crate::testing::TestOperations;
    use crate::types::{NamedLeavesRefusal, TransferLeaves, VolumeCopyConfig, WriteOperationError};
    use cmdr_fs::testing::TestDir;
    use cmdr_fs::volume::RoutedKind;
    use cmdr_operation_log::types::Initiator;

    let tmp = TestDir::new("routing_relative_zip");
    let source = tmp.join("src").join("a.txt");
    std::fs::create_dir_all(tmp.join("src")).expect("src");
    std::fs::write(&source, b"a").expect("source");
    let engine = TestOperations::new();
    let stick = crate::local_disk::folder_volume("Stick", tmp.to_path_buf(), crate::host::testing::pane_order_host());
    engine.volumes().register("stick", stick);
    let zip: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("box.zip"));
    engine.volumes().route(
        &crate::testing::app_path_of(&tmp.join("box.zip")),
        RoutedKind::Archive,
        zip,
    );

    for moves in [false, true] {
        let config = VolumeCopyConfig {
            leaves: TransferLeaves::per_source([(source.clone(), "renamed.txt".to_string())]),
            ..VolumeCopyConfig::default()
        };
        let events: Arc<dyn OperationEventSink> = Arc::new(CollectorEventSink::new());
        let (from, to, dest) = ("stick".to_string(), "stick".to_string(), "/box.zip/sub".to_string());
        let result = if moves {
            engine
                .start_volume_move(
                    events,
                    from,
                    vec![crate::testing::app_path_of(&source)],
                    to,
                    dest,
                    config,
                    Initiator::User,
                    None,
                )
                .await
        } else {
            engine
                .start_volume_copy(
                    events,
                    from,
                    vec![crate::testing::app_path_of(&source)],
                    to,
                    dest,
                    config,
                    Initiator::User,
                    None,
                )
                .await
        };
        assert!(
            matches!(
                &result,
                Err(WriteOperationError::NamedLeavesRefused {
                    reason: NamedLeavesRefusal::RouteCannotName,
                    ..
                })
            ),
            "moves={moves}: the zip route answers, got {result:?}"
        );
    }
    assert!(source.exists(), "nothing moved");
    assert!(
        !tmp.join("box.zip").exists(),
        "nothing landed on the drive at the zip's name"
    );
}
