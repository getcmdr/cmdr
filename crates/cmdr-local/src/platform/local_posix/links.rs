//! The symlink seam's bodies (`read_link_target`, `parent_for_dot_dot`, `create_symlink`), which the
//! trait methods in `local_posix.rs` delegate to.

use super::LocalPosixVolume;
use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::VolumeError;
use cmdr_fs::volume::dot_dot::{LinkFuture, LinkReader, parent_across_links};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

impl LocalPosixVolume {
    pub(super) async fn read_link_target_impl(&self, path: &AppPath) -> Result<Option<PathBuf>, VolumeError> {
        read_link_at(self.os_path(path)).await
    }

    /// `<path>/..` as the kernel reads it: the link target's parent when the last
    /// name is a symlink (`cmdr_fs::volume::dot_dot`). Reads links anywhere on this
    /// machine, since a target can leave the volume's root.
    pub(super) async fn parent_for_dot_dot_impl(&self, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
        parent_across_links(&MachineLinks, path).await
    }

    /// `symlink(2)`: refuses an occupied name by itself, and stores the target
    /// as given. A filesystem with no links (exFAT, FAT) answers `EPERM`,
    /// `ENOTSUP`, or `EOPNOTSUPP`, all of which mean "can't hold it". The last two are one
    /// number on Linux and two on macOS, so this compares values: a `matches!` pattern
    /// naming both doesn't compile on Linux.
    pub(super) async fn create_symlink_impl(&self, path: &AppPath, target: &Path) -> Result<(), VolumeError> {
        let abs_path = self.os_path(path);
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

/// `readlink(2)` at `native`, off the async thread: `Ok(None)` for anything but a link.
async fn read_link_at(native: PathBuf) -> Result<Option<PathBuf>, VolumeError> {
    spawn_blocking(move || match std::fs::read_link(&native) {
        Ok(target) => Ok(Some(target)),
        // `EINVAL` is `readlink`'s own "that's no link".
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => Ok(None),
        Err(e) => Err(VolumeError::from_io_at(&e, &native)),
    })
    .await
    .expect("spawn_blocking read_link closure doesn't panic and the task is uncancelable")
}

/// The links of this machine's one tree, for a `..`: `readlink(2)` on any local path.
struct MachineLinks;

impl LinkReader for MachineLinks {
    fn read_link<'a>(&'a self, path: &'a AppPath) -> LinkFuture<'a> {
        Box::pin(async move {
            // A path no local call can spell (a scheme path, an MTP pick) holds no local link.
            let Ok(native) = path.to_native() else {
                return Ok(None);
            };
            read_link_at(native).await
        })
    }

    fn absolute_target(&self, target: &str) -> Option<AppPathBuf> {
        AppPathBuf::from_native_unicode(Path::new(target)).ok()
    }
}
