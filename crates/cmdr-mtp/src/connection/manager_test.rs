//! Unit cells for the manager and the free functions beside it.
//!
//! Nothing here opens a device: path normalization, icon ids, the device-id →
//! `location_id` lookup, the enabled flag, and the wire shape of
//! `MtpDisconnectReason`. Anything that needs a real session goes in
//! `host_seam_test.rs` or one of the `device_tests` modules, which carry the
//! `virtual-mtp` gate.

use std::sync::Arc;

use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::host::VolumeHost;

use super::storage_path::storage_path;
use super::{
    ConnectedDeviceInfo, MtpConnectionManager, MtpDisconnectReason, MtpObjectInfo, MtpVolumeRegistrar, events,
    get_mtp_icon_id, resolve_device_location_id,
};

fn detached_manager() -> Arc<MtpConnectionManager> {
    MtpConnectionManager::new(
        VolumeHost::detached(),
        events::no_device_events(),
        MtpVolumeRegistrar::detached(),
    )
}

/// `mtp_rs::Error::AccessDenied` doesn't say read or write, so the read path says it:
/// a folder the phone keeps private is a refused read, ❌ never "read-only" storage.
#[test]
fn a_read_answered_access_denied_is_a_refused_read() {
    let mapped = detached_manager().map_device_read_error(mtp_rs::Error::AccessDenied, "mtp-1", "Android/data");
    assert!(
        matches!(&mapped, super::MtpConnectionError::ReadRefused { device_id, path }
            if device_id == "mtp-1" && path == "Android/data"),
        "got {mapped:?}"
    );
}

/// Every other answer to a read classifies as it does for anything else.
#[test]
fn a_read_answered_anything_else_classifies_as_usual() {
    let mapped = detached_manager().map_device_read_error(mtp_rs::Error::Disconnected, "mtp-1", "DCIM");
    assert!(
        matches!(mapped, super::MtpConnectionError::Disconnected { .. }),
        "got {mapped:?}"
    );
}

/// A manager nobody has told anything has to be ON. The app pushes the
/// persisted setting in at startup, and a manager that started out refusing
/// would drop a session-reset recovery in the window before that push, which
/// reads to the user as a phone that never came back from a screen lock.
#[test]
fn a_manager_nobody_configured_has_mtp_on() {
    assert!(detached_manager().is_enabled());
}

/// Setting it reports what it was, so the settings toggle can tell a real
/// change from a redundant push: the disable path disconnects every device
/// and clears the known set, and running that for a no-op push would tear
/// down live sessions.
#[test]
fn turning_mtp_off_and_on_again_reports_what_it_was() {
    let manager = detached_manager();

    assert!(manager.set_enabled(false), "it was on");
    assert!(!manager.is_enabled());
    assert!(!manager.set_enabled(false), "still off, and that's not a change");
    assert!(!manager.set_enabled(true), "it was off");
    assert!(manager.is_enabled());
}

#[test]
fn resolve_device_location_id_is_none_when_no_device_matches() {
    // Device-id → location_id resolution now matches the live enumeration
    // rather than numerically decoding the id (a serial id can't be decoded).
    // With no device of this id connected, resolution yields None — the
    // connect path then returns `DeviceNotFound`. (A positive match needs a
    // live/virtual device; the derivation it matches against is unit-tested
    // in `cmdr_fs::volume::mtp_ids`.)
    assert_eq!(resolve_device_location_id("mtp-no-such-device-9999"), None);
}

// ========================================================================
// Path normalization tests
// ========================================================================

#[test]
fn test_normalize_mtp_path_empty() {
    assert_eq!(storage_path("").as_deref().map(AppPath::as_str), Some("/"));
}

#[test]
fn test_normalize_mtp_path_dot() {
    assert_eq!(storage_path(".").as_deref().map(AppPath::as_str), Some("/"));
}

#[test]
fn test_normalize_mtp_path_root() {
    assert_eq!(storage_path("/").as_deref().map(AppPath::as_str), Some("/"));
}

#[test]
fn test_normalize_mtp_path_absolute() {
    assert_eq!(storage_path("/DCIM").as_deref().map(AppPath::as_str), Some("/DCIM"));
    assert_eq!(
        storage_path("/DCIM/Camera").as_deref().map(AppPath::as_str),
        Some("/DCIM/Camera")
    );
}

#[test]
fn test_normalize_mtp_path_relative() {
    assert_eq!(storage_path("DCIM").as_deref().map(AppPath::as_str), Some("/DCIM"));
    assert_eq!(
        storage_path("DCIM/Camera").as_deref().map(AppPath::as_str),
        Some("/DCIM/Camera")
    );
}

#[test]
fn test_normalize_mtp_path_special_characters() {
    // Test paths with spaces and special characters
    assert_eq!(
        storage_path("/My Files").as_deref().map(AppPath::as_str),
        Some("/My Files")
    );
    assert_eq!(
        storage_path("Photos & Videos").as_deref().map(AppPath::as_str),
        Some("/Photos & Videos")
    );
}

// ========================================================================
// Icon ID generation tests
// ========================================================================

#[test]
fn test_get_mtp_icon_id_directory() {
    assert_eq!(get_mtp_icon_id(true, "DCIM"), "dir");
    assert_eq!(get_mtp_icon_id(true, "Camera"), "dir");
    assert_eq!(get_mtp_icon_id(true, ""), "dir");
}

#[test]
fn test_get_mtp_icon_id_file_with_extension() {
    assert_eq!(get_mtp_icon_id(false, "photo.jpg"), "ext:jpg");
    assert_eq!(get_mtp_icon_id(false, "document.PDF"), "ext:pdf");
    assert_eq!(get_mtp_icon_id(false, "video.MP4"), "ext:mp4");
    assert_eq!(get_mtp_icon_id(false, "archive.tar.gz"), "ext:gz");
}

#[test]
fn test_get_mtp_icon_id_file_without_extension() {
    assert_eq!(get_mtp_icon_id(false, "README"), "file");
    assert_eq!(get_mtp_icon_id(false, "Makefile"), "file");
    // Hidden files starting with . have no "real" extension, Path::extension returns None
    assert_eq!(get_mtp_icon_id(false, ".hidden"), "file");
}

// ========================================================================
// Object info tests
// ========================================================================

#[test]
fn test_object_info_serialization() {
    let info = MtpObjectInfo {
        handle: 12345,
        name: "test.jpg".to_string(),
        path: "/DCIM/test.jpg".to_string(),
        is_directory: false,
        size: Some(1024),
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"handle\":12345"));
    assert!(json.contains("\"name\":\"test.jpg\""));
    assert!(json.contains("\"path\":\"/DCIM/test.jpg\""));
    assert!(json.contains("\"isDirectory\":false"));
    assert!(json.contains("\"size\":1024"));
}

#[test]
fn test_object_info_directory() {
    let info = MtpObjectInfo {
        handle: 100,
        name: "Photos".to_string(),
        path: "/Photos".to_string(),
        is_directory: true,
        size: None,
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"isDirectory\":true"));
    assert!(json.contains("\"size\":null"));
}

// ========================================================================
// Connected device info tests
// ========================================================================

#[test]
fn test_connected_device_info_serialization() {
    use super::super::types::{MtpDeviceInfo, MtpStorageInfo};

    let info = ConnectedDeviceInfo {
        device: MtpDeviceInfo {
            id: "mtp-336592896".to_string(),
            location_id: 336592896,
            vendor_id: 0x18d1,
            product_id: 0x4ee1,
            manufacturer: Some("Google".to_string()),
            product: Some("Pixel 8".to_string()),
            serial_number: None,
            usb_speed: None,
        },
        storages: vec![MtpStorageInfo {
            id: 65537,
            name: "Internal shared storage".to_string(),
            total_bytes: 128_000_000_000,
            available_bytes: 64_000_000_000,
            storage_type: Some("FixedRAM".to_string()),
            is_read_only: false,
        }],
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"id\":\"mtp-336592896\""));
    assert!(json.contains("\"locationId\":336592896"));
    assert!(json.contains("\"manufacturer\":\"Google\""));
    assert!(json.contains("\"product\":\"Pixel 8\""));
    assert!(json.contains("\"Internal shared storage\""));
    assert!(json.contains("\"isReadOnly\":false"));
}

// Device-id ↔ location_id derivation and the `:`-robust volume-id parse are
// owned and exhaustively tested by `cmdr_fs::volume::mtp_ids`; the connect path
// here only resolves a live id to a location (tested above as the no-match
// case). No id-string edge cases are re-tested in this module.

// ========================================================================
// MtpDisconnectReason serialization
// ========================================================================
//
// The reason is emitted as a JSON string on the `mtp-device-disconnected`
// event and consumed by hand-written TS in `lib/tauri-commands/mtp.ts`.
// The serialized form is the contract; pin it.

#[test]
fn test_mtp_disconnect_reason_serializes_to_snake_case() {
    assert_eq!(serde_json::to_string(&MtpDisconnectReason::User).unwrap(), "\"user\"");
    assert_eq!(
        serde_json::to_string(&MtpDisconnectReason::Removed).unwrap(),
        "\"removed\""
    );
}

#[test]
fn test_mtp_disconnect_reason_embeds_in_event_payload() {
    // Mirrors the `json!({ "deviceId": ..., "reason": reason })` shape in
    // `disconnect()` / `handle_device_disconnected()`.
    let payload = serde_json::json!({
        "deviceId": "mtp-1",
        "reason": MtpDisconnectReason::Removed,
    });
    assert_eq!(payload["reason"], "removed");
    assert_eq!(payload["deviceId"], "mtp-1");

    let payload_user = serde_json::json!({
        "deviceId": "mtp-1",
        "reason": MtpDisconnectReason::User,
    });
    assert_eq!(payload_user["reason"], "user");
}
