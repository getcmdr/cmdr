//! Windows: `GetDiskFreeSpaceExW`, through the path's `\\?\` form. "Available" is what this
//! user may take (quotas included), as `statvfs`'s `f_bavail` is.

use std::path::Path;

use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::{SpaceInfo, VolumeError};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

use crate::platform::local_windows::paths::{app_of, native_of};
use crate::platform::local_windows::rename::wide;
use crate::reading::Named;

/// A native path in either spelling (`C:\x`, `C:/x`), as `get_space_info_for_path` takes it.
pub(crate) fn filesystem_space(path: &Path) -> Result<SpaceInfo, VolumeError> {
    let app = app_of(path).map_err(|e| VolumeError::from_io_at(&e, path))?;
    filesystem_space_at(&app)
}

/// The space of the filesystem holding the app path `path`, an error naming it.
fn filesystem_space_at(path: &AppPath) -> Result<SpaceInfo, VolumeError> {
    let named = Named::new(path.as_str().to_owned());
    let native = native_of(path).map_err(|e| named.error(&e))?;
    let wide = wide(&native);
    let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call, and the three
    // out-pointers are live, aligned `u64`s the call fills.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    if ok == 0 {
        return Err(named.error(&std::io::Error::last_os_error()));
    }
    Ok(SpaceInfo::Bounded {
        total_bytes: total,
        available_bytes: available,
        used_bytes: total.saturating_sub(free),
    })
}
