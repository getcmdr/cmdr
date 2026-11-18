//! What a test drives a virtual device with, on either side of the boundary.
//!
//! Every device cell in this crate and every MTP cell in the app needs the same
//! four steps: register a fixture-backed phone, find the id discovery gave it,
//! connect, and prime the root listing (so a cell's first op reads the path
//! cache instead of paying the resolver's heal listing). Written out per file
//! that's forty lines each and six chances to get the teardown wrong.
//!
//! The one thing that differs across the boundary is WHICH manager: a cell here
//! wants a detached host that answers nothing, and an app cell wants the real
//! wiring, so the listing cache, the index, and the volume registry see what the
//! device reports. So every entry point takes the manager, and the app's
//! `mtp/test_support.rs` shadows them with no-argument versions over its parked
//! one. Same shape as `cmdr_smb::volume::testing` and the app's
//! `smb_test_support.rs`.
//!
//! ❌ Never enable `testing` in a production build, and ❌ don't grow this into a
//! way to reach the session layer's state: it hands out a connected device and
//! takes it away again. The two instruments that read the backend are
//! `volume::testing`, and they stay two numbers.

use cmdr_fs::app_path::AppPath;
use std::path::Path;
use std::sync::Arc;

use cmdr_fs::volume::Volume;

use crate::connection::{DeviceWatch, MtpConnectionManager, MtpDisconnectReason};
use crate::virtual_device::{
    VirtualDeviceFixture, setup_virtual_mtp_device, unregister_virtual_mtp_device, virtual_device_test_lock,
};
use crate::volume::MtpVolume;

#[cfg(test)]
pub(crate) use crate::connection::testing::{is_attached, recording_registrar, test_connection_manager};

/// A virtual device that's registered, connected, and primed.
///
/// Holds its fixture, so the backing `TempDir` outlives every cell that reads
/// through it. ❗ Call [`ConnectedDevice::teardown`] rather than dropping it: a
/// device left registered survives into the next test in the same binary, which
/// then connects to a stale storage handle over a directory that's already gone
/// and fails on its first write with a bare protocol error.
pub struct ConnectedDevice {
    /// The device id discovery reports, which is serial-derived rather than
    /// `format!("mtp-{location_id}")`. Every session call takes this.
    pub id: String,
    /// The first storage the device reported: the writable internal one.
    pub storage_id: u32,
    /// Every storage the connect reported, in the order it reported them. The
    /// virtual device has two, so a cell asserting on the per-storage loop has
    /// more than one to count.
    pub storage_ids: Vec<u32>,
    /// What [`unregister_virtual_mtp_device`] takes.
    pub location_id: u64,
    fixture: VirtualDeviceFixture,
}

impl ConnectedDevice {
    /// The fixture's backing directory. `root().join("internal")` is what the
    /// writable storage serves, so a cell can seed a file there (plus a
    /// [`crate::virtual_device::rescan_virtual_device`]) or assert one is gone.
    pub fn root(&self) -> &Path {
        self.fixture.root()
    }

    /// Disconnects AND unregisters. Both halves are load-bearing; see the type's
    /// note. Tolerant of an already-disconnected device, so a cell that tested a
    /// disconnect path can still finish through here.
    pub async fn teardown(self, manager: &Arc<MtpConnectionManager>) {
        let _ = manager.disconnect(&self.id, MtpDisconnectReason::User).await;
        unregister_virtual_mtp_device(self.location_id);
    }
}

/// The lock every virtual-device cell holds for its whole span.
///
/// All virtual devices register under one serial, hence one Cmdr device id, so
/// two cells in the same process would fight over it. Re-exported here so a
/// suite needs one `use`.
pub async fn device_lock() -> tokio::sync::MutexGuard<'static, ()> {
    virtual_device_test_lock().lock().await
}

/// Registers a virtual device, connects `manager` to it with the watch off, and
/// primes the root listing.
///
/// Caller holds [`device_lock`] first. `DeviceWatch::Off` because a cell that
/// wants the event loop running says so itself; the default here is the quiet
/// one, so nothing arrives that the cell didn't ask for.
pub async fn connect_virtual_device(manager: &Arc<MtpConnectionManager>) -> ConnectedDevice {
    let fixture = setup_virtual_mtp_device();
    connect_fixture(manager, fixture).await
}

/// [`connect_virtual_device`] over a fixture the caller already seeded.
///
/// Seeding wants the backing directory before the device is connected (write the
/// file, then [`crate::virtual_device::rescan_virtual_device`]), which is why
/// this half is separate.
pub async fn connect_fixture(manager: &Arc<MtpConnectionManager>, fixture: VirtualDeviceFixture) -> ConnectedDevice {
    let location_id = fixture.location_id;
    let id = crate::list_mtp_devices()
        .into_iter()
        .find(|device| device.location_id == location_id)
        .map(|device| device.id)
        .expect("the virtual device must appear in discovery");
    let info = manager
        .connect(&id, DeviceWatch::Off)
        .await
        .expect("virtual-mtp connect should succeed");
    let storage_ids: Vec<u32> = info.storages.iter().map(|storage| storage.id).collect();
    let storage_id = *storage_ids.first().expect("the virtual device reports storages");
    manager
        .list_directory(&id, storage_id, "/")
        .await
        .expect("priming the root listing");
    ConnectedDevice {
        id,
        storage_id,
        storage_ids,
        location_id,
        fixture,
    }
}

/// An `MtpVolume` over the device's writable storage, with `path` primed too.
///
/// [`connect_virtual_device`] stops at the root. Priming `path` too puts its
/// entries in the path cache, for a cell that counts listings or wants its first
/// op to skip the resolver's heal. Pass `None` when the root is enough.
pub async fn volume_for(
    manager: &Arc<MtpConnectionManager>,
    device: &ConnectedDevice,
    primed: Option<&str>,
) -> MtpVolume {
    let volume = MtpVolume::new(Arc::clone(manager), &device.id, device.storage_id, "Test");
    if let Some(path) = primed {
        volume
            .list_directory(&AppPath::legacy_unchecked(Path::new(path)), None)
            .await
            .unwrap_or_else(|error| panic!("priming the {path} listing: {error:?}"));
    }
    volume
}

/// Gives `folder` two items named `name`: from now on the device reports the
/// object at `folder/other` under `name` too, the way a camera or phone holding
/// two same-named files lists them. Returns `(name's handle, other's)`, the
/// `inode` each row carries, so a cell can tell the twins apart.
///
/// The virtual device is backed by a real folder, which can't hold two files
/// with one name, so the second one keeps its own name on disk: a cell tells
/// which object an op reached by which backing file changed.
pub async fn make_twins(
    manager: &Arc<MtpConnectionManager>,
    device: &ConnectedDevice,
    folder: &str,
    name: &str,
    other: &str,
) -> (u64, u64) {
    let entries = manager
        .list_directory(&device.id, device.storage_id, folder)
        .await
        .unwrap_or_else(|error| panic!("listing {folder}: {error:?}"));
    let handle_of = |wanted: &str| {
        entries
            .iter()
            .find(|entry| entry.name == wanted)
            .and_then(|entry| entry.inode)
            .unwrap_or_else(|| panic!("no {wanted} in {folder}"))
    };
    let (first, second) = (handle_of(name), handle_of(other));
    manager
        .report_object_as_for_test(&device.id, mtp_rs::ObjectHandle(second), name)
        .await;
    (first, second)
}
