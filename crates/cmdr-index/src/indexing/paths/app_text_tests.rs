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

/// A volume's root is the bridge's unnormalized text of the path its registration got
/// (`LocalPosixVolume::on_mount`), so a mount reported as `/Volumes/X/` must still open
/// where the OS opens it: the local walker ran there before the root was typed.
#[cfg(unix)]
#[test]
fn a_bridge_root_the_os_reads_as_one_place_crosses_to_that_place() {
    use cmdr_fs::volume::{InMemoryVolume, Volume as _};
    use std::path::Path;
    // `with_root` keeps its text as the bridge does, the way `LocalPosixVolume::on_mount` does.
    let root_of = |text: &str| InMemoryVolume::new("Drive").with_root(text);
    for (text, native) in [
        ("/Volumes/X/", "/Volumes/X"),
        ("/Volumes/./X", "/Volumes/X"),
        ("/", "/"),
    ] {
        assert_eq!(
            native_root(root_of(text).root()).as_deref(),
            Some(Path::new(native)),
            "{text}"
        );
    }
    assert_eq!(
        native_root(root_of("/Volumes/X/../Y").root()),
        None,
        "a `..` is a volume's question, never the text's"
    );
}

#[test]
fn stored_text_reads_back_as_its_own_place_or_none_never_a_look_alike() {
    assert_eq!(app_path_of("/Volumes/s/a").expect("canonical").as_str(), "/Volumes/s/a");
    assert_eq!(
        app_path_of("adb://R58").expect("a bare scheme root").as_str(),
        "adb://R58/"
    );
    assert_eq!(app_path_of("/Volumes/s//a/").expect("folded").as_str(), "/Volumes/s/a");
    // A canonical escape stays the row's own text.
    let lone = "/Volumes/s/x\u{10FEFF}";
    assert_eq!(app_path_of(lone).expect("canonical").as_str(), lone);
    // Non-canonical escapes would respell to `/Volumes/s/xé`, another folder.
    assert!(app_path_of("/Volumes/s/x\u{10FEC3}\u{10FEA9}").is_none());
    assert!(app_path_of("/Volumes/s/../x").is_none());
}
