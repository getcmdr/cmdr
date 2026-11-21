//! A cached handle that no longer names the object at its path.
//!
//! A device that frees a handle and hands it to another object within one
//! session (non-compliant firmware: MTP 1.1 § 3.4.1 forbids it, and TinyUSB's
//! example does it, hathach/tinyusb#4045) leaves Cmdr's path cache pointing a
//! still-shown row at someone else's file. The virtual device never reuses a
//! handle, so each cell plants the stale entry itself. ❗ Regression anchors for
//! #524: delete, rename, and move used to act on whatever held the handle.

use super::DeviceWatch;
use super::MtpDeleteScope;
use super::testing::test_connection_manager as connection_manager;
use crate::virtual_device::{
    VirtualDeviceFixture, setup_virtual_mtp_device, unregister_virtual_mtp_device, virtual_device_test_lock,
};
use std::path::PathBuf;

const REPORT: &str = "Quarterly report: Q4 2025 placeholder content.\n";

struct Device {
    id: String,
    storage_id: u32,
    fixture: VirtualDeviceFixture,
}

impl Device {
    fn on_disk(&self, rel: &str) -> PathBuf {
        self.fixture.root().join("internal").join(rel)
    }
}

/// A connected virtual device whose cache says `/Documents/notes.txt` is the
/// object that is really `/Documents/report.txt`: the handle the device reused.
async fn device_with_a_reused_handle() -> Device {
    let fixture = setup_virtual_mtp_device();
    let id = crate::list_mtp_devices()
        .into_iter()
        .find(|d| d.location_id == fixture.location_id)
        .map(|d| d.id)
        .expect("the virtual device must appear in discovery");
    let info = connection_manager()
        .connect(&id, DeviceWatch::Off)
        .await
        .expect("virtual-mtp connect should succeed");
    let storage_id = info.storages.first().expect("a storage").id;
    connection_manager()
        .list_directory(&id, storage_id, "/Documents")
        .await
        .expect("list Documents should succeed");
    let report = connection_manager()
        .cached_handle_for_path(&id, storage_id, "/Documents/report.txt")
        .await
        .expect("the listing cached report.txt");
    connection_manager()
        .plant_cached_handle(&id, storage_id, "/Documents/notes.txt", report)
        .await;
    Device {
        id,
        storage_id,
        fixture,
    }
}

async fn teardown(device: Device) {
    connection_manager()
        .disconnect(&device.id, super::MtpDisconnectReason::User)
        .await
        .ok();
    unregister_virtual_mtp_device(device.fixture.location_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_through_a_reused_handle_leaves_the_object_now_holding_it() {
    let _guard = virtual_device_test_lock().lock().await;
    let device = device_with_a_reused_handle().await;

    let outcome = connection_manager()
        .delete_object(
            &device.id,
            device.storage_id,
            "/Documents/notes.txt",
            MtpDeleteScope::SingleNode,
        )
        .await;

    assert_eq!(
        std::fs::read_to_string(device.on_disk("Documents/report.txt"))
            .ok()
            .as_deref(),
        Some(REPORT),
        "the object that inherited the handle is untouched"
    );
    assert!(outcome.is_ok(), "a fresh resolve finds notes.txt, got {outcome:?}");
    assert!(
        !device.on_disk("Documents/notes.txt").exists(),
        "the file the user picked is the one that goes"
    );
    teardown(device).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_through_a_reused_handle_leaves_the_object_now_holding_it() {
    let _guard = virtual_device_test_lock().lock().await;
    let device = device_with_a_reused_handle().await;

    let outcome = connection_manager()
        .rename_object(&device.id, device.storage_id, "/Documents/notes.txt", "renamed.txt")
        .await;

    assert_eq!(
        std::fs::read_to_string(device.on_disk("Documents/report.txt"))
            .ok()
            .as_deref(),
        Some(REPORT),
        "the object that inherited the handle keeps its name"
    );
    assert!(outcome.is_ok(), "a fresh resolve finds notes.txt, got {outcome:?}");
    assert!(device.on_disk("Documents/renamed.txt").exists());
    teardown(device).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_through_a_reused_handle_leaves_the_object_now_holding_it() {
    let _guard = virtual_device_test_lock().lock().await;
    let device = device_with_a_reused_handle().await;

    let outcome = connection_manager()
        .move_object(&device.id, device.storage_id, "/Documents/notes.txt", "/Music")
        .await;

    assert_eq!(
        std::fs::read_to_string(device.on_disk("Documents/report.txt"))
            .ok()
            .as_deref(),
        Some(REPORT),
        "the object that inherited the handle stays where it was"
    );
    assert!(outcome.is_ok(), "a fresh resolve finds notes.txt, got {outcome:?}");
    assert!(device.on_disk("Music/notes.txt").exists());
    teardown(device).await;
}
