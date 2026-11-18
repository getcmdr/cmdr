//! Windows: `GetDiskFreeSpaceExW`, through the path's `\\?\` form (an app path in, plan D1).
//! "Available" is what this user may take (quotas included), as `statvfs`'s `f_bavail` is.

use std::path::Path;

use cmdr_fs::volume::{SpaceInfo, VolumeError};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

use crate::platform::local_windows::paths::native;
use crate::platform::local_windows::rename::wide;

pub(crate) fn filesystem_space(path: &Path) -> Result<SpaceInfo, VolumeError> {
    let native = native(path).map_err(|e| VolumeError::from_io_at(&e, path))?;
    let wide = wide(&native);
    let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call, and the three
    // out-pointers are live, aligned `u64`s the call fills.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    if ok == 0 {
        return Err(VolumeError::from_io_at(&std::io::Error::last_os_error(), path));
    }
    Ok(SpaceInfo::Bounded {
        total_bytes: total,
        available_bytes: available,
        used_bytes: total.saturating_sub(free),
    })
}
