//! The readings of app text, on every OS.

use super::{last_name, parse_on_volume};

#[test]
fn a_phones_or_shares_posix_path_reads_on_any_host() {
    let path = parse_on_volume("/DCIM/Camera/a.jpg").expect("a POSIX path on any volume");
    assert_eq!(path.as_str(), "/DCIM/Camera/a.jpg");
    assert_eq!(last_name("/DCIM/Camera/a.jpg").as_deref(), Some("a.jpg"));
}

#[test]
fn a_scheme_path_reads_and_names_its_last_name() {
    assert_eq!(last_name("mtp://dev-1/1/DCIM/a.jpg").as_deref(), Some("a.jpg"));
}

#[test]
fn text_that_names_no_place_reads_as_nothing() {
    assert!(parse_on_volume("relative/x").is_err());
    assert!(
        parse_on_volume("/a/../b").is_err(),
        "a `..` is the typed-path door's to answer"
    );
    assert_eq!(last_name("/"), None);
}

/// Regression (#1430): Windows' `realpath` (`std::fs::canonicalize`) answers the verbatim
/// `\\?\C:\…`, and its lossy text reached the index as a scope no row matched, so a search of
/// the current folder found nothing. A native answer comes back as the app path it names, in
/// either flavor, on every host.
#[test]
fn a_native_answer_reads_back_as_its_app_path_in_either_flavor() {
    use cmdr_fs::app_path::PathFlavor;
    use std::path::Path;

    let windows = [
        (r"\\?\C:\Users\test_user_1\src", "C:/Users/test_user_1/src"),
        (r"C:\Users\test_user_1\src", "C:/Users/test_user_1/src"),
        (r"\\?\UNC\testnas.local\share\x", "//testnas.local/share/x"),
        (r"\\?\C:\", "C:/"),
    ];
    for (native, app) in windows {
        assert_eq!(
            super::of_native_as(Path::new(native), PathFlavor::Windows).as_deref(),
            Some(app),
            "{native}"
        );
    }
    assert_eq!(
        super::of_native_as(Path::new("/private/tmp/x"), PathFlavor::Posix).as_deref(),
        Some("/private/tmp/x")
    );
    // A device path names no app path: the caller keeps what it had.
    assert_eq!(
        super::of_native_as(Path::new(r"\\?\Volume{1234}\x"), PathFlavor::Windows),
        None
    );
}

/// [`super::realpath`] resolves a real folder to app text this host's [`super::parse`] reads.
#[test]
fn a_real_folders_realpath_is_app_text() {
    let dir = crate::test_support::AppTestDir::new("app-text-realpath");
    let real = super::realpath(dir.app_path().as_str()).expect("an existing folder resolves");
    assert!(super::parse(&real).is_ok(), "{real} reads as an app path");
    assert!(!real.starts_with(r"\\?\"), "{real}");
}
