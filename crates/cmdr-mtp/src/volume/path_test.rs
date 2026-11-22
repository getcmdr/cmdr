//! `MtpVolume`'s identity and its path conversions.
//!
//! Crate-side because every cell reaches the backend's own internals: the
//! `device_id` / `storage_id` fields, and `to_mtp_path`, which is the one place
//! the three shapes a caller can hand a path in (an `mtp://` URL, an absolute
//! path, a relative one) are reduced to the storage-relative string the PTP
//! layer takes. A wrong reduction is a read from the wrong object, which is why
//! this is asserted here rather than through the pipeline above it.

use std::sync::Arc;

use cmdr_fs::app_path;
use cmdr_fs::volume::Volume;

use super::{MtpVolume, mtp_path_of_text};
use crate::connection::testing::test_connection_manager as connection_manager;

#[test]
fn test_new_creates_volume() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-20-5", 65537, "Internal storage");
    assert_eq!(vol.name(), "Internal storage");
    assert_eq!(vol.device_id, "mtp-20-5");
    assert_eq!(vol.storage_id, 65537);
}

#[test]
fn test_root_path() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-20-5", 65537, "Internal storage");
    assert_eq!(vol.root().as_str(), "mtp://mtp-20-5/65537");
}

#[test]
fn test_to_mtp_path_empty() {
    // The empty and `.` spellings a `std::path` caller still sends, as text: no app
    // path holds one.
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-20-5", 65537, "Test");
    assert_eq!(mtp_path_of_text(""), "");
    assert_eq!(vol.to_mtp_path(app_path!("/")).unwrap(), "");
    assert_eq!(mtp_path_of_text("."), "");
}

#[test]
fn test_to_mtp_path_relative() {
    // Relative text, which no app path holds: the reduction it still gets.
    assert_eq!(mtp_path_of_text("DCIM"), "DCIM");
    assert_eq!(mtp_path_of_text("DCIM/Camera"), "DCIM/Camera");
}

#[test]
fn test_to_mtp_path_absolute() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-20-5", 65537, "Test");
    assert_eq!(vol.to_mtp_path(app_path!("/DCIM")).unwrap(), "DCIM");
    assert_eq!(vol.to_mtp_path(app_path!("/DCIM/Camera")).unwrap(), "DCIM/Camera");
}

#[test]
fn test_to_mtp_path_mtp_url_root() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-0-1", 65537, "Test");
    // MTP URL for storage root
    assert_eq!(vol.to_mtp_path(app_path!("mtp://mtp-0-1/65537")).unwrap(), "");
}

#[test]
fn test_to_mtp_path_mtp_url_with_path() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-0-1", 65537, "Test");
    // MTP URL with nested path
    assert_eq!(vol.to_mtp_path(app_path!("mtp://mtp-0-1/65537/DCIM")).unwrap(), "DCIM");
    assert_eq!(
        vol.to_mtp_path(app_path!("mtp://mtp-0-1/65537/DCIM/Camera")).unwrap(),
        "DCIM/Camera"
    );
}

/// A path whose text no device lists (a live escape: a Linux name that isn't text, joined
/// onto a phone path) addresses NOTHING, ❌ never its lossy spelling: `caf\u{FFFD}.txt` is
/// a name a device can really hold, so a lossy delete, listing, or read would reach that
/// sibling. A name listed with an escape-range code point, in the app's spelling of its
/// row, reaches the device as listed (#1310).
#[test]
fn a_path_no_device_lists_addresses_nothing() {
    use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
    use cmdr_fs::volume::VolumeError;
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-0-1", 65537, "Test");
    let live = app_path::escape_bytes(b"caf\xe9.txt").into_owned();
    for text in [
        format!("mtp://mtp-0-1/65537/DCIM/{live}"),
        format!("mtp://mtp-0-1/65537/{live}/a"),
    ] {
        let path = AppPathBuf::parse(&text, PathFlavor::Posix).expect("an app path");
        assert!(
            matches!(vol.to_mtp_path(&path), Err(VolumeError::NotFound(_))),
            "{text:?}"
        );
        assert_eq!(vol.storage_path_of(&path), None, "{text:?}");
    }
    let row = "mtp://mtp-0-1/65537/DCIM/x\u{10FEFF}";
    let sent = AppPathBuf::from_listed_text(row, PathFlavor::Posix).expect("an app path");
    assert_eq!(vol.to_mtp_path(&sent).unwrap(), "DCIM/x\u{10FEFF}");
}
