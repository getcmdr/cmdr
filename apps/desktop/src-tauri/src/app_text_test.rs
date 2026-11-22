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
