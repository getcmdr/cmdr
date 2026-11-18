//! macOS and Linux: `statvfs`.

use std::path::Path;

use cmdr_fs::volume::{SpaceInfo, VolumeError};

pub(crate) fn filesystem_space(path: &Path) -> Result<SpaceInfo, VolumeError> {
    use std::ffi::CString;

    let path_c = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| VolumeError::IoError {
        message: e.to_string().into(),
        raw_os_error: None,
    })?;

    // SAFETY: `path_c` is a valid NUL-terminated C string from `path`; `stat` is a zeroed,
    // correctly-typed `libc::statvfs` out-buffer the kernel fills, and its fields are only read on
    // the `== 0` (success) branch where the kernel initialized them.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path_c.as_ptr(), &mut stat) == 0 {
            #[allow(clippy::unnecessary_cast, reason = "statvfs field types vary across platforms")]
            let block_size = stat.f_frsize as u64;
            #[allow(clippy::unnecessary_cast, reason = "statvfs field types vary across platforms")]
            let total_bytes = (stat.f_blocks as u64) * block_size;
            #[allow(clippy::unnecessary_cast, reason = "statvfs field types vary across platforms")]
            let available_bytes = (stat.f_bavail as u64) * block_size;
            #[allow(clippy::unnecessary_cast, reason = "statvfs field types vary across platforms")]
            let used_bytes = total_bytes.saturating_sub((stat.f_bfree as u64) * block_size);

            // ❗ `used` is NOT the complement of `available` here: `statvfs`
            // reserves blocks that are neither free to a normal user nor holding
            // anything, so the two come from different fields and `bounded`
            // (which derives one from the other) would be wrong.
            Ok(SpaceInfo::Bounded {
                total_bytes,
                available_bytes,
                used_bytes,
            })
        } else {
            Err(VolumeError::IoError {
                message: "Failed to get space info".into(),
                raw_os_error: None,
            })
        }
    }
}
