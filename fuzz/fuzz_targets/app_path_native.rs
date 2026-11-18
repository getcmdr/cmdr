//! The app path's native boundary: the lossless escape round-trips every byte and every
//! UTF-16 unit, and whatever app path `from_native_windows` (and `from_native_posix`) builds
//! is canonical and survives `to_native` and back unchanged. The input is both a POSIX path's
//! bytes and, read as UTF-16LE, a Windows path's units: the hand-parsed `\\?\`, `UNC`,
//! `Volume{`, `GLOBALROOT`, drive, and plain-path prefix logic is what this reaches.
#![no_main]

use cmdr_fs::app_path::{
    AppPath, AppPathBuf, PathFlavor, escape_bytes, escape_units, unescape_bytes, unescape_units,
};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();

    let spelled = escape_units(&units);
    assert_eq!(
        unescape_units(&spelled).ok().as_deref(),
        Some(&units[..]),
        "the unit escape round-trips"
    );
    let spelled = escape_bytes(data);
    assert_eq!(
        unescape_bytes(&spelled).ok().as_deref(),
        Some(data),
        "the byte escape round-trips"
    );

    if let Ok(path) = AppPathBuf::from_native_windows(&units, PathFlavor::Windows) {
        assert!(
            AppPath::from_normalized_as(path.as_str(), PathFlavor::Windows).is_ok(),
            "from_native_windows built a non-canonical path: {path:?}"
        );
        // A reported path names its file literally: its last name, as the OS stored it
        // (trailing dots and spaces included), is the app path's last name.
        let last = units.rsplit(|&u| u == u16::from(b'\\') || u == u16::from(b'/')).next().unwrap_or(&[]);
        let plain_name = !last.is_empty() && last != [u16::from(b'.')] && last != [u16::from(b'.'); 2];
        if plain_name && !path.is_root() {
            assert_eq!(
                path.file_name(),
                Some(escape_units(last).as_ref()),
                "from_native_windows respelled its last name: {path:?}"
            );
        }
        if let Ok(native) = path.to_native_windows() {
            // Text, not `Eq`: a round trip that respells without changing components moved too.
            assert_eq!(
                AppPathBuf::from_native_windows(&native, PathFlavor::Windows)
                    .as_ref()
                    .map(|back| back.as_str()),
                Ok(path.as_str()),
                "the native round trip moved {path:?}"
            );
        }
    }

    if let Ok(path) = AppPathBuf::from_native_posix(data, PathFlavor::Posix) {
        assert!(
            AppPath::from_normalized_as(path.as_str(), PathFlavor::Posix).is_ok(),
            "from_native_posix built a non-canonical path: {path:?}"
        );
        if let Ok(native) = path.to_native_posix() {
            assert_eq!(
                AppPathBuf::from_native_posix(&native, PathFlavor::Posix)
                    .as_ref()
                    .map(|back| back.as_str()),
                Ok(path.as_str()),
                "the native round trip moved {path:?}"
            );
        }
    }
});
