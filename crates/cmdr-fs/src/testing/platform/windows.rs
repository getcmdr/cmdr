//! Windows links need a privilege a plain user only has with Developer Mode, and their
//! kind (file or folder) is fixed at creation.

use std::io;
use std::path::Path;
use std::sync::OnceLock;

/// See the re-export in `mod.rs`.
pub fn symlink<T: AsRef<Path>, L: AsRef<Path>>(target: T, link: L) -> io::Result<()> {
    let (target, link) = (target.as_ref(), link.as_ref());
    let resolved = match link.parent() {
        Some(folder) if target.is_relative() => folder.join(target),
        _ => target.to_path_buf(),
    };
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// An elevated Windows process still meets ACL refusals the way a test means it to, so
/// nothing here needs skipping.
pub fn is_root() -> bool {
    false
}

/// See the re-export in `mod.rs`.
pub fn long_path(path: &Path) -> io::Result<std::path::PathBuf> {
    crate::app_path::expand_short_names(path)
}

/// See the re-export in `mod.rs`: `TEMP` spelled long, read once per process.
pub fn scratch_root() -> std::path::PathBuf {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let temp = std::env::temp_dir();
        long_path(&temp).unwrap_or(temp)
    })
    .clone()
}

ctor::declarative::ctor! {
    /// Points `tempfile`'s default folder at [`scratch_root`] before `main`, so every bare
    /// `tempfile::tempdir()` in a test binary spells its folder long. `override_temp_dir`
    /// sets it once per process, and nothing else calls it.
    ///
    /// SAFETY: runs before `main`, where it touches only `std::env`, one Win32 call
    /// (`GetLongPathNameW`), and two `OnceLock`s, none of which needs `main`'s setup.
    #[ctor(unsafe)]
    fn spell_tempfile_scratch_long() {
        let _already_set = tempfile::env::override_temp_dir(&scratch_root());
    }
}

/// Probed once per process: one dangling file link in a fresh temp folder.
pub fn can_create_symlinks() -> bool {
    static CAN: OnceLock<bool> = OnceLock::new();
    *CAN.get_or_init(|| {
        let Ok(dir) = tempfile::tempdir() else { return false };
        std::os::windows::fs::symlink_file("missing", dir.path().join("probe")).is_ok()
    })
}
