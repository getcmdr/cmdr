//! Fixtures for the ADB volume suites, on both sides of the crate boundary.
//!
//! Gated behind the `testing` feature, so it exists in dev targets and in no
//! shipped build. The fake server itself is `crate::testing::FakeAdbServer`.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, PathFlavor};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::listings::RecordingListings;
use cmdr_fs::volume::{adb_app_root, adb_volume_id};
use tokio_util::sync::CancellationToken;

use super::{AdbVolume, AdbVolumeInner, ConnectionState, app_root_for, connect_adb_volume};
use crate::features::DeviceFeatures;
use crate::params::AdbConnectionParams;
use crate::server::AdbEndpoint;
use crate::testing::FakeAdbServer;

/// The serial every fixture device answers to: the fake server's own.
pub const FIXTURE_SERIAL: &str = crate::testing::FAKE_SERIAL;

/// The app path a pane holds for `device` on the fixture phone:
/// `adb://<FIXTURE_SERIAL><device>`. Suites spell paths through this so they
/// address the volume the way a pane does.
pub fn fixture_path(device: &str) -> PathBuf {
    PathBuf::from(format!("{}{device}", adb_app_root(FIXTURE_SERIAL)))
}

/// [`fixture_path`] as a typed app path, `adb://<FIXTURE_SERIAL><device>` parsed:
/// the device's `/` (or `""`) is the canonical root, `adb://<FIXTURE_SERIAL>/`.
///
/// A typed path holds no `..` and no relative text; what the volume does with such
/// TEXT, which only a `std::path` caller can still send, is `paths_test`'s.
pub fn fixture_app_path(device: &str) -> AppPathBuf {
    let text = format!("{}{device}", adb_app_root(FIXTURE_SERIAL));
    AppPathBuf::parse(&text, PathFlavor::Posix).expect("a fixture device path is an app path")
}

/// `name`, a listed `FileEntry.name`, in `folder`.
pub fn child(folder: &AppPath, name: &str) -> AppPathBuf {
    let name = AppName::from_app_spelling(name).expect("a listed name is one plain name");
    folder.join(name).expect("a listed name joins its folder")
}

/// A volume with no server behind it, for the pure paths (path translation,
/// capability answers). Every wire-touching call on it fails.
pub fn detached_volume() -> AdbVolume {
    let host = VolumeHost::detached();
    let (root, paths) = app_root_for(FIXTURE_SERIAL);
    AdbVolume {
        name: "Fixture phone".to_string(),
        root,
        paths,
        inner: Arc::new(AdbVolumeInner {
            connection: cmdr_fs::volume::ConnectionReporter::new(
                &adb_volume_id(FIXTURE_SERIAL),
                &host,
                ConnectionState::Connected,
            ),
            volume_id: adb_volume_id(FIXTURE_SERIAL),
            serial: FIXTURE_SERIAL.to_string(),
            endpoint: AdbEndpoint::at(std::net::SocketAddr::from(([127, 0, 0, 1], 1))),
            features: DeviceFeatures::all(),
            unmounted: AtomicBool::new(false),
            reconnect_lock: tokio::sync::Mutex::new(()),
            host,
        }),
    }
}

/// Connects to the fake server's device `serial` through a host that records
/// what the volume tells its panes.
pub async fn connect_fake(server: &FakeAdbServer, serial: &str) -> (Arc<AdbVolume>, Arc<RecordingListings>) {
    let listings = Arc::new(RecordingListings::new());
    let host = VolumeHost::builder()
        .listings(Arc::clone(&listings) as Arc<dyn cmdr_fs::volume::host::listings::ListingHost>)
        .build();
    let volume = connect_adb_volume(
        AdbConnectionParams::at(serial, server.endpoint()),
        host,
        CancellationToken::new(),
    )
    .await
    .unwrap_or_else(|e| panic!("connecting to the fake server's {serial} must work, got {e:?}"));
    (Arc::new(volume), listings)
}
