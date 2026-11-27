//! A native path → its app path's text → the app path again names the same file, for any name,
//! with escapes off and as they'll behave once on (#1524).

use std::path::PathBuf;

use super::super::*;
use super::{from_text_in, text_in};

/// An absolute native path to `name`, on this host's disk.
fn native_with(name: &str) -> PathBuf {
    let folder = if PathFlavor::HOST == PathFlavor::Windows {
        "C:\\scratch"
    } else {
        "/scratch"
    };
    PathBuf::from(folder).join(name)
}

fn escape_range(c: u32) -> char {
    char::from_u32(c).expect("plane 16 private use is a scalar value")
}

/// Names a native API may hold as valid text: plain, non-ASCII, a literal code point of each
/// escape alphabet, and the four code points a literal U+10FEFF's app spelling is made of (the
/// name the escaped spelling would turn into if it were read back as literal text).
fn tricky_names() -> Vec<String> {
    vec![
        "plain.txt".to_string(),
        "caf\u{e9}.txt".to_string(),
        format!("caf{}.txt", escape_range(0x10_FEFF)),
        format!("caf{}.txt", escape_range(0x10_FE80)),
        format!("caf{}.txt", escape_range(0x10_E800)),
        [0x10_FEF4, 0x10_FE8F, 0x10_FEBB, 0x10_FEBF]
            .into_iter()
            .map(escape_range)
            .collect(),
    ]
}

#[test]
fn a_native_name_round_trips_through_its_text_whether_escapes_are_live_or_not() {
    for live in [false, true] {
        for name in tricky_names() {
            let native = native_with(&name);
            let app = AppPathBuf::from_native(&native).expect("an absolute native path");

            let text = text_in(&app, live).expect("a Unicode name has text");
            let back = from_text_in(&text, live).expect("its own text reads back");

            assert_eq!(back, app, "{name:?}, escapes live: {live}");
            assert_eq!(back.to_native(), app.to_native(), "{name:?}, escapes live: {live}");
        }
    }
}

/// While escapes aren't live the text is the name's own: what the index, a pane's row, and a
/// store hold for the same file, so a value written out meets the rows it came from.
#[test]
fn with_escapes_off_the_text_is_the_names_own_text() {
    for name in tricky_names() {
        let native = native_with(&name);
        let app = AppPathBuf::from_native(&native).expect("an absolute native path");

        let text = text_in(&app, false).expect("a Unicode name has text");

        assert!(
            text.ends_with(&format!("/{name}")),
            "{text:?} spells {name:?} literally"
        );
    }
}

/// A name that isn't Unicode text has no literal text while escapes are off: refused typed,
/// ❌ never a look-alike that names another file. Live, its escape round-trips.
#[cfg(unix)]
#[test]
fn a_name_that_isnt_unicode_has_text_only_once_escapes_are_live() {
    use std::os::unix::ffi::OsStrExt;
    let native = PathBuf::from(std::ffi::OsStr::from_bytes(b"/scratch/caf\xff.txt"));
    let app = AppPathBuf::from_native(&native).expect("an absolute native path");

    assert_eq!(text_in(&app, false), Err(NotAnAppPath::NotUnicode));
    let text = text_in(&app, true).expect("live, every name has text");
    assert_eq!(from_text_in(&text, true).expect("reads back"), app);
}

#[cfg(unix)]
mod any_bytes {
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    use proptest::prelude::*;

    use super::super::super::*;
    use super::super::{from_text_in, text_in};

    /// Bytes weighted toward what breaks a round trip: escape-range code points' UTF-8 (`F4 8F
    /// BA..BF xx`), lone high bytes, and plain ASCII.
    fn name_bytes() -> impl Strategy<Value = Vec<u8>> {
        let piece = prop_oneof![
            Just(vec![b'a']),
            Just(vec![0xF4, 0x8F, 0xBB, 0xBF]),
            Just(vec![0xF4, 0x8F, 0xBA, 0x80]),
            Just(vec![0xF4, 0x8E, 0xA0, 0x80]),
            Just(vec![0xC3, 0xA9]),
            (0x80u8..=0xFF).prop_map(|b| vec![b]),
        ];
        prop::collection::vec(piece, 1..6).prop_map(|pieces| pieces.concat())
    }

    proptest! {
        #[test]
        fn any_native_name_round_trips_or_is_refused_typed(name in name_bytes(), live in any::<bool>()) {
            let mut bytes = b"/scratch/".to_vec();
            bytes.extend_from_slice(&name);
            let native = PathBuf::from(std::ffi::OsStr::from_bytes(&bytes));
            let app = AppPathBuf::from_native(&native).expect("an absolute native path");

            match text_in(&app, live) {
                Ok(text) => {
                    let back = from_text_in(&text, live).expect("its own text reads back");
                    prop_assert_eq!(back.to_native().expect("local"), native);
                }
                Err(why) => {
                    prop_assert!(!live, "live, every name has text");
                    prop_assert_eq!(why, NotAnAppPath::NotUnicode);
                    prop_assert!(std::str::from_utf8(&name).is_err(), "only a non-Unicode name is refused");
                }
            }
        }
    }
}
