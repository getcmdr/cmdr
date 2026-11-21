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
    assert_eq!(vol.to_mtp_path(app_path!("/")), "");
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
    assert_eq!(vol.to_mtp_path(app_path!("/DCIM")), "DCIM");
    assert_eq!(vol.to_mtp_path(app_path!("/DCIM/Camera")), "DCIM/Camera");
}

#[test]
fn test_to_mtp_path_mtp_url_root() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-0-1", 65537, "Test");
    // MTP URL for storage root
    assert_eq!(vol.to_mtp_path(app_path!("mtp://mtp-0-1/65537")), "");
}

#[test]
fn test_to_mtp_path_mtp_url_with_path() {
    let vol = MtpVolume::new(Arc::clone(connection_manager()), "mtp-0-1", 65537, "Test");
    // MTP URL with nested path
    assert_eq!(vol.to_mtp_path(app_path!("mtp://mtp-0-1/65537/DCIM")), "DCIM");
    assert_eq!(
        vol.to_mtp_path(app_path!("mtp://mtp-0-1/65537/DCIM/Camera")),
        "DCIM/Camera"
    );
}
