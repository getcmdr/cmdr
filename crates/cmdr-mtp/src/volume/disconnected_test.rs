//! What an `MtpVolume` answers once its phone's session is gone.
//!
//! A volume outlives its session in whoever still holds it: a conflict check, a
//! copy, a pane re-read that started before the cable came out. Every one of
//! those used to hear `VolumeError::NotFound`, because the session layer's
//! `NotConnected` mapped there, and "not found" is an ANSWER to those callers.
//! The conflict scan reads it as "the destination doesn't exist yet, so nothing
//! clashes" and the paste went ahead with no conflict prompt.

use cmdr_fs::app_path;
use cmdr_fs::volume::{SourceItemInfo, Volume, VolumeError};

use crate::connection::MtpDisconnectReason;
use crate::testing::{connect_virtual_device, device_lock, test_connection_manager, volume_for};

/// A conflict check against a phone that went away is a failure the dialog can
/// report, never an empty "no conflicts".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_scan_on_a_disconnected_phone_fails_instead_of_finding_nothing() {
    let _guard = device_lock().await;
    let device = connect_virtual_device(test_connection_manager()).await;
    let volume = volume_for(test_connection_manager(), &device, None).await;
    test_connection_manager()
        .disconnect(&device.id, MtpDisconnectReason::User)
        .await
        .expect("disconnecting the virtual device");

    let conflicts = volume
        .scan_for_conflicts(
            &[SourceItemInfo {
                name: "report.txt".to_string(),
                size: 1,
                modified: None,
                is_directory: false,
            }],
            app_path!("/Documents"),
        )
        .await;
    assert!(
        matches!(conflicts, Err(VolumeError::DeviceDisconnected(_))),
        "a gone phone must not read as a destination with nothing in it, got {conflicts:?}"
    );

    let listing = volume.list_directory(app_path!("/Documents"), None).await;
    assert!(
        matches!(listing, Err(VolumeError::DeviceDisconnected(_))),
        "a listing on a gone phone is a disconnect, not a missing folder, got {listing:?}"
    );

    device.teardown(test_connection_manager()).await;
}
