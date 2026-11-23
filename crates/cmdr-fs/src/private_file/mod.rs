//! Files only the current user can open, private from the moment they exist: the MCP bearer
//! token and its port file, the plain secrets file of dev and E2E builds, the error-report
//! preview, and the Multi-Rename Results file.
//!
//! - **Private at creation, never after.** A file made loose and tightened later is readable for
//!   that window, so the access goes in with the create: mode `0600` on Unix, and on Windows a
//!   protected DACL granting only the current user, passed to `CreateFileW` (`platform/`).
//! - **Only ever a NEW file.** On every OS a create that opens an existing file keeps that file's
//!   access and ignores the one asked for, so [`create_new`] refuses anything already at the name,
//!   a link included. [`write_atomically`] writes a fresh sibling and renames it over the target,
//!   which carries the sibling's access along.
//! - **Not the data folder's ACL.** On Windows the profile's `AppData` already keeps other
//!   standard users out, but `CMDR_DATA_DIR` or a redirected `AppData` can put the folder anywhere,
//!   and a file that inherits is as open as its folder. No barrier to an administrator, who reads
//!   through the backup privilege. `DETAILS.md` § "Private files".

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

mod platform;

#[cfg(test)]
mod tests;

/// A new file at `path`, writable, that only the current user can open. Fails with
/// `AlreadyExists` if anything is at `path`, a link or a dangling link included, so a planted link
/// can't redirect the write and a stale file can't lend it its looser access.
pub fn create_new(path: &Path) -> io::Result<File> {
    platform::create_new(path)
}

/// Writes `contents` to `path` so the file is private to the current user from its first byte
/// and a reader never sees it half-written: a fresh sibling through [`create_new`], synced, then
/// renamed over `path`. A file already at `path`, however loose, is replaced, never reused. On
/// failure the sibling is removed and `path` is left as it was.
///
/// First it removes the temps an earlier write of `path` left when it crashed between create and
/// rename ([`sweep_crashed_temps`]): each holds an older secret, and nothing else would ever
/// delete it. A write of the same file running at the same moment in another thread or process
/// would lose its temp and fail its rename; every caller writes a given file from one place.
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    sweep_crashed_temps(path);
    let temp = crate::staging::temp_sibling_path(path);
    let written = (|| {
        let mut file = create_new(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp); // best effort: the error that matters is the write's
    }
    written
}

/// Removes the regular files beside `path` named like its temps (`<name>.cmdr-tmp-<uuid>`,
/// `staging::temp_sibling_prefix`). A link or a folder with that shape stays. Best effort: a temp
/// that can't be listed or removed is left for the next write.
fn sweep_crashed_temps(path: &Path) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let prefix = crate::staging::temp_sibling_prefix(name);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_its_temp = entry.file_name().to_str().is_some_and(|n| n.starts_with(&prefix));
        // `DirEntry::file_type` doesn't follow a link, so a link reads as one and stays.
        if is_its_temp && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = std::fs::remove_file(entry.path()); // best effort, see above
        }
    }
}

/// Whether the file at `path` (not a link's target) is a regular file only the current user can
/// open: on Unix, owned by this user with no group or other bits; on Windows, owned by this user
/// with a protected DACL holding exactly one entry, full access for this user. For tests and for
/// checking a file someone else may have made.
pub fn is_private_to_current_user(path: &Path) -> io::Result<bool> {
    platform::is_private_to_current_user(path)
}
