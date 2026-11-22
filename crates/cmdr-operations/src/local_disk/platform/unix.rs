//! The POSIX answers macOS and Linux share: modes, `symlink(2)`, `access(W_OK)`,
//! `statvfs`, and the local folder volume.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

use super::super::WriteProbe;

pub(crate) fn mode_of(permissions: &std::fs::Permissions) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    permissions.mode()
}

pub(crate) fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

pub(crate) fn create_dir_with_mode(dir: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(mode).create(dir)
}

pub(crate) fn with_mode(options: &mut std::fs::OpenOptions, mode: u32) -> &mut std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(mode)
}

pub(crate) fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

pub(crate) fn remove_leaf(path: &Path) -> std::io::Result<()> {
    cmdr_local::remove_leaf(path)
}

/// `access(path, W_OK)`, which evaluates ACLs as well as the mode bits on macOS.
pub(crate) fn write_probe(path: &Path) -> WriteProbe {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return WriteProbe::Unrepresentable;
    };
    // SAFETY: `c_path` is a valid NUL-terminated C string that outlives the call.
    if unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } == 0 {
        WriteProbe::Allowed
    } else {
        WriteProbe::Refused(std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EACCES))
    }
}

/// `statvfs`: free blocks available to an unprivileged writer.
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: c_path is a valid null-terminated C string, stat is a valid pointer
    let result = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: statvfs succeeded, stat is initialized
    let stat = unsafe { stat.assume_init() };
    #[allow(
        clippy::unnecessary_cast,
        reason = "Required for macOS where statvfs fields are not u64"
    )]
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

pub(crate) fn folder_volume(name: &str, root: PathBuf, host: VolumeHost) -> Arc<dyn Volume> {
    Arc::new(cmdr_local::LocalVolume::local_folder(name, root, host))
}
