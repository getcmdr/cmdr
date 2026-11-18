//! The symlink seam's bodies (`read_link_target`, `create_symlink`), which the
//! trait methods in `local_posix.rs` delegate to.

use super::LocalPosixVolume;
use cmdr_fs::volume::VolumeError;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

impl LocalPosixVolume {
    pub(super) async fn read_link_target_impl(&self, path: &Path) -> Result<Option<PathBuf>, VolumeError> {
        let abs_path = self.resolve(path);
        spawn_blocking(move || match std::fs::read_link(&abs_path) {
            Ok(target) => Ok(Some(target)),
            // `EINVAL` is `readlink`'s own "that's no link".
            Err(e) if e.raw_os_error() == Some(libc::EINVAL) => Ok(None),
            Err(e) => Err(VolumeError::from_io_at(&e, &abs_path)),
        })
        .await
        .expect("spawn_blocking read_link closure doesn't panic and the task is uncancelable")
    }

    /// `symlink(2)`: refuses an occupied name by itself, and stores the target
    /// as given. A filesystem with no links (exFAT, FAT) answers `EPERM`,
    /// `ENOTSUP`, or `EOPNOTSUPP`, all of which mean "can't hold it". The last two are one
    /// number on Linux and two on macOS, so this compares values: a `matches!` pattern
    /// naming both doesn't compile on Linux.
    pub(super) async fn create_symlink_impl(&self, path: &Path, target: &Path) -> Result<(), VolumeError> {
        let abs_path = self.resolve(path);
        let target = target.to_path_buf();
        spawn_blocking(move || match std::os::unix::fs::symlink(&target, &abs_path) {
            Ok(()) => Ok(()),
            Err(e)
                if [libc::EPERM, libc::ENOTSUP, libc::EOPNOTSUPP]
                    .map(Some)
                    .contains(&e.raw_os_error()) =>
            {
                Err(VolumeError::NotSupported)
            }
            Err(e) => Err(VolumeError::from_io_at(&e, &abs_path)),
        })
        .await
        .expect("spawn_blocking symlink closure doesn't panic and the task is uncancelable")
    }
}
