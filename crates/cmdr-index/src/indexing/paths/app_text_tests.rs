use super::*;
use cmdr_fs::app_path::{AppPathBuf, PathFlavor};

#[test]
fn a_root_of_plain_text_is_stored_as_its_own_text() {
    for root in ["/", "/Volumes/NAS share", "mtp://pixel-7a1b/65537", "adb://R58M1234/"] {
        let path = AppPathBuf::parse(root, PathFlavor::Posix).expect("a canonical root");
        assert_eq!(stored_text(&path), path.as_str());
        assert!(
            matches!(stored_text(&path), Cow::Borrowed(_)),
            "the text of {root} is borrowed"
        );
    }
}

#[test]
fn a_root_that_isnt_unicode_text_is_stored_as_the_lossy_spelling_the_index_always_stored() {
    let native = b"/Volumes/caf\xe9";
    let path = AppPathBuf::from_native_posix(native, PathFlavor::Posix).expect("an absolute path");
    assert_ne!(
        path.as_str(),
        "/Volumes/caf\u{FFFD}",
        "the app path spells the byte losslessly"
    );
    assert_eq!(stored_text(&path), String::from_utf8_lossy(native));
}

#[test]
fn a_root_holding_a_literal_escape_range_character_keeps_that_character() {
    let literal = "/Volumes/x\u{10FEE9}";
    let path = AppPathBuf::from_native_posix(literal.as_bytes(), PathFlavor::Posix).expect("an absolute path");
    assert_eq!(stored_text(&path), literal);
}

#[cfg(unix)]
#[test]
fn a_local_root_crosses_to_its_native_bytes_and_a_device_root_to_none() {
    use std::os::unix::ffi::OsStrExt;
    let native = b"/Volumes/caf\xe9";
    let path = AppPathBuf::from_native_posix(native, PathFlavor::Posix).expect("an absolute path");
    assert_eq!(native_root(&path).expect("a local root").as_os_str().as_bytes(), native);
    let phone = AppPathBuf::parse("mtp://pixel-7a1b/65537", PathFlavor::Posix).expect("a canonical root");
    assert_eq!(native_root(&phone), None);
}

#[test]
fn a_windows_root_holding_an_unpaired_surrogate_is_stored_lossily_too() {
    let units: Vec<u16> = "C:\\x".encode_utf16().chain([0xD800]).collect();
    let path = AppPathBuf::from_native_windows(&units, PathFlavor::Windows).expect("a drive path");
    assert_eq!(stored_text(&path), "C:/x\u{FFFD}");
}
