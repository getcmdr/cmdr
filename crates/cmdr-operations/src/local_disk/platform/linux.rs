//! Linux: the POSIX answers. No lock flags Cmdr reads and no purgeable space.

use std::path::Path;

pub(crate) use super::unix::{
    create_dir_with_mode, folder_volume, free_bytes, mode_of, set_mode, symlink, with_mode, write_probe,
};

/// Linux's immutable attribute (`chattr +i`) isn't read yet, so nothing counts as locked.
pub(crate) fn is_locked_item(_path: &Path) -> bool {
    false
}

/// Linux refuses its own locks (`chattr +i`), so nothing needs refusing up front.
pub(crate) fn locked_against_removal(_path: &Path) -> bool {
    false
}

pub(crate) const ASKS_ABOUT_LOCKS: bool = false;

/// Linux has no purgeable space, so `statvfs` is the whole answer.
pub(crate) fn important_free_bytes(_path: &Path) -> Option<u64> {
    None
}
