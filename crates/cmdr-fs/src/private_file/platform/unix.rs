//! Unix: mode `0600` passed to `open(2)` with `O_CREAT | O_EXCL`, so the file is owner-only from
//! the call that makes it (the umask can only take bits away). `O_EXCL` also refuses a link at the
//! name, dangling or not, so nothing redirects the create.

use std::fs::File;
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

/// Read and write for the owner, nothing for anyone else.
const OWNER_ONLY: u32 = 0o600;

/// An `O_EXCL` create at mode `0600`.
pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(OWNER_ONLY)
        .open(path)
}

/// A regular file this user owns, with no group or other bits.
pub(crate) fn is_private_to_current_user(path: &Path) -> io::Result<bool> {
    let meta = std::fs::symlink_metadata(path)?;
    // SAFETY: `geteuid` takes no arguments, shares no memory, and can't fail.
    let me = unsafe { libc::geteuid() };
    Ok(meta.is_file() && meta.uid() == me && meta.mode() & 0o077 == 0)
}
