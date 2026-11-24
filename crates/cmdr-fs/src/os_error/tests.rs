//! The seam's own promises: each OS's table names a code once, every class it names
//! reads back, and the one Windows-only class never comes out of a Unix code.

use super::OsErrorClass;
use super::platform::TABLES;

#[test]
fn a_code_is_named_once() {
    let mut seen = std::collections::HashSet::new();
    for &(code, class) in TABLES.iter().flat_map(|table| table.iter()) {
        assert!(
            seen.insert(code),
            "code {code} is named twice (second time as {class:?})"
        );
    }
}

#[test]
fn every_named_code_reads_back_as_its_class_and_every_class_has_a_code_of_its_own() {
    for &(code, class) in TABLES.iter().flat_map(|table| table.iter()) {
        assert_eq!(OsErrorClass::of_code(code), class, "code {code}");
        let host = class.host_code().expect("a named class has a code");
        assert_eq!(OsErrorClass::of_code(host), class, "{class:?}'s code {host}");
    }
}

/// On Unix each class has exactly one errno, so a backend that reports a class gets
/// back the very number it would have named.
#[cfg(unix)]
#[test]
fn every_unix_errno_round_trips_through_its_class() {
    for &(code, class) in TABLES.iter().flat_map(|table| table.iter()) {
        assert_eq!(class.host_code(), Some(code), "{class:?}");
    }
}

#[test]
fn a_code_nobody_names_is_other_and_other_has_no_code() {
    assert_eq!(OsErrorClass::of_code(-1), OsErrorClass::Other);
    assert_eq!(OsErrorClass::of_code(999_999), OsErrorClass::Other);
    assert_eq!(OsErrorClass::Other.host_code(), None);
}

#[test]
fn an_io_error_without_a_code_has_no_class() {
    assert_eq!(OsErrorClass::of_io(&std::io::Error::other("a backend's words")), None);
}

/// "In use by another app" is Windows' sharing and lock violations. No Unix errno
/// means it (`EBUSY` is a busy mount or device, `ETXTBSY` a running program), so no
/// Unix code may produce it, and it has no Unix code to report.
#[cfg(unix)]
#[test]
fn no_unix_code_is_in_use_by_another_app() {
    for code in 0..=4096 {
        assert_ne!(
            OsErrorClass::of_code(code),
            OsErrorClass::InUseByAnotherApp,
            "errno {code}"
        );
    }
    assert_eq!(OsErrorClass::InUseByAnotherApp.host_code(), None);
    assert_eq!(OsErrorClass::of_code(libc::EBUSY), OsErrorClass::Busy);
    assert_eq!(OsErrorClass::of_code(libc::ETXTBSY), OsErrorClass::Other);
}

/// The transport set is the one `std` reads as a dropped link, errno for errno:
/// the set this replaced was decided through `ErrorKind`.
#[cfg(unix)]
#[test]
fn the_transport_classes_are_the_errnos_std_reads_as_a_dropped_link() {
    use std::io::ErrorKind;

    for code in 0..=4096 {
        let by_kind = matches!(
            std::io::Error::from_raw_os_error(code).kind(),
            ErrorKind::BrokenPipe
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
                | ErrorKind::TimedOut
                | ErrorKind::NetworkDown
                | ErrorKind::NetworkUnreachable
                | ErrorKind::HostUnreachable
        );
        assert_eq!(OsErrorClass::of_code(code).is_transport(), by_kind, "errno {code}");
    }
}

/// The numbers that differ between the two Unixes land in one class each.
#[cfg(unix)]
#[test]
fn each_unix_names_its_own_numbers() {
    assert_eq!(OsErrorClass::of_code(libc::ENOTEMPTY), OsErrorClass::NotEmpty);
    assert_eq!(OsErrorClass::of_code(libc::EDQUOT), OsErrorClass::QuotaExceeded);
    assert_eq!(OsErrorClass::of_code(libc::ESTALE), OsErrorClass::Stale);
    assert_eq!(OsErrorClass::of_code(libc::ENOSYS), OsErrorClass::NotImplemented);
    #[cfg(target_os = "linux")]
    assert_eq!(OsErrorClass::of_code(libc::EOPNOTSUPP), OsErrorClass::Unsupported);
    #[cfg(target_os = "macos")]
    assert_eq!(
        OsErrorClass::of_code(libc::EOPNOTSUPP),
        OsErrorClass::OperationNotSupported
    );
}

/// A Windows share answers a folder's flush with `ERROR_INVALID_FUNCTION` (verified on
/// Windows 10 22H2 against `\\localhost\C$`, 2026-11-22): a sync the filesystem doesn't
/// offer, so the copy's durability pass counts it as done instead of warning that the
/// copies may not be written.
#[cfg(windows)]
#[test]
fn a_share_refusing_a_folder_flush_is_a_sync_it_does_not_offer() {
    let refused = std::io::Error::from_raw_os_error(1);
    assert_eq!(OsErrorClass::of_io(&refused), Some(OsErrorClass::Unsupported));
    assert!(crate::durable::is_sync_unsupported(&refused));
    assert_eq!(
        OsErrorClass::Unsupported.host_code(),
        Some(50),
        "ERROR_NOT_SUPPORTED stays the class's code"
    );
}

/// A drive pulled under a running copy answers its open handles with one of these, and each
/// must read as the drive going away (task 39), ❌ never a generic I/O error.
#[cfg(windows)]
#[test]
fn a_drive_that_went_away_reads_as_no_device() {
    for (code, class) in [
        (21, OsErrorClass::NoDevice),
        (55, OsErrorClass::NoDevice),
        (1006, OsErrorClass::NoDevice),
        (1112, OsErrorClass::NoDevice),
        (1167, OsErrorClass::DeviceNotConfigured),
        (1617, OsErrorClass::DeviceNotConfigured),
    ] {
        let gone = std::io::Error::from_raw_os_error(code);
        assert_eq!(OsErrorClass::of_io(&gone), Some(class), "error {code}");
    }
    assert_eq!(
        OsErrorClass::NoDevice.host_code(),
        Some(21),
        "ERROR_NOT_READY stays the class's code"
    );
}
