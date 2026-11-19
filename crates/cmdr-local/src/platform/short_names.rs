//! When an old-form constructor's native root needs its 8.3 short names spelled long. Pure, so
//! every OS tests it (`short_names_test.rs`); only `LocalWindowsVolume` calls it.
//!
//! ❗ A constructor runs on whatever thread registers the volume, and `GetLongPathNameW`
//! (`cmdr_fs::app_path::expand_short_names`) reads every folder on the path, so on a share
//! that's gone offline it waits out the SMB timeout right there. `Volume::root` is
//! synchronous and must answer the long spelling from the start, so the expansion can't be
//! put off to a later disk call either. So it only runs for a path that can hold a short
//! name: one with a `~`, which every short name Windows generates carries (`LONGNA~1`,
//! `AB12C~1`). The paths Cmdr registers (drive roots, share roots, the profile folders)
//! carry none; the `%TEMP%` under a long account name does.

use std::io;
use std::path::{Path, PathBuf};

/// `root` with its short names spelled long by `expand`, asked only when `root` can hold one;
/// `root` itself when `expand` refuses (a missing path keeps its text, as before).
#[cfg_attr(unix, allow(dead_code, reason = "the Windows volume's; tested on every OS"))]
pub(crate) fn long_root(root: PathBuf, expand: impl FnOnce(&Path) -> io::Result<PathBuf>) -> PathBuf {
    if !may_hold_a_short_name(&root) {
        return root;
    }
    expand(&root).unwrap_or(root)
}

/// Whether a name in `path` can be an 8.3 short name: it holds a `~`.
fn may_hold_a_short_name(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().contains(&b'~')
}

#[cfg(test)]
#[path = "short_names_test.rs"]
mod tests;
