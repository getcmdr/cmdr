//! Where this OS mounts the volumes a person plugs in or connects, as path prefixes, and
//! where its boot volume starts: pure string knowledge, no syscall, so the index's hot paths
//! and the importance classifier can ask it. The mount TABLE (what's mounted right now) is
//! `cmdr_local::mounts`, which re-exports this.

mod platform;

/// The prefixes under which one more path component names a mounted volume: `/Volumes/` on
/// macOS, `/mnt/` and `/media/` on Linux, none yet on Windows (a drive letter is its own root).
/// `/Volumes/backup` is a volume root, `/Volumes/backup/photos` a folder inside it.
pub fn user_mount_prefixes() -> &'static [&'static str] {
    platform::user_mount_prefixes()
}

/// Where the boot volume (the registry's `root`) starts, as an app path: `/` on macOS and
/// Linux, the system drive on Windows (`C:/`, plan D1), where each drive is its own volume.
/// The root drive index walks from here, ❌ never from a bare `/`, which Windows reads as the
/// process's CURRENT drive.
pub fn boot_volume_root() -> String {
    platform::boot_volume_root()
}

/// The system drive's app root from `%SystemDrive%`'s value (`C:` gives `C:/`, the letter
/// uppercased), else `C:/`. Pure, so it's tested on every OS.
pub fn system_drive_root_from(system_drive: Option<&str>) -> String {
    match system_drive.map(str::as_bytes) {
        Some([letter, b':']) if letter.is_ascii_alphabetic() => format!("{}:/", letter.to_ascii_uppercase() as char),
        _ => "C:/".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_is_an_uppercase_drive_root() {
        assert_eq!(system_drive_root_from(Some("C:")), "C:/");
        assert_eq!(system_drive_root_from(Some("d:")), "D:/");
    }

    #[test]
    fn anything_else_falls_back_to_c() {
        for odd in [None, Some(""), Some("C:\\"), Some("1:"), Some("CD:")] {
            assert_eq!(system_drive_root_from(odd), "C:/", "{odd:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_boot_volume_is_the_whole_tree_on_macos_and_linux() {
        assert_eq!(boot_volume_root(), "/");
    }
}
