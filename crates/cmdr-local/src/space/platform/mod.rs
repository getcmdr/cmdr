//! How much room the filesystem holding a path has, asked of the filesystem itself.

use std::path::Path;

use cmdr_fs::volume::{SpaceInfo, VolumeError};

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(&Path) -> Result<SpaceInfo, VolumeError> = filesystem_space;

/// The filesystem's own total, available-to-this-user, and used figures for `path`, the
/// volume holding it.
pub(super) use imp::filesystem_space;
