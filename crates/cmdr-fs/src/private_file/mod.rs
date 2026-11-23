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
//!   standard users out, but it lets `SYSTEM` and `Administrators` in, and `CMDR_DATA_DIR` or a
//!   redirected `AppData` can put the folder anywhere. Why the DACL holds nothing else:
//!   `DETAILS.md` § "Private files".

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
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
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

/// Whether the file at `path` (not a link's target) is a regular file only the current user can
/// open: on Unix, owned by this user with no group or other bits; on Windows, owned by this user
/// with a protected DACL holding exactly one entry, full access for this user. For tests and for
/// checking a file someone else may have made.
pub fn is_private_to_current_user(path: &Path) -> io::Result<bool> {
    platform::is_private_to_current_user(path)
}
