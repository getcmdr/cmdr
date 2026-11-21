//! Whether a Windows drive keeps a Recycle Bin: the ONE answer the volume's `has_trash`, the
//! volume list's `supports_trash`, and the Recycle Bin arm's refusal all read
//! (`cmdr-operations`' `delete/trash/platform/windows/bin.rs`), so the three can't disagree
//! (#1332).
//!
//! A drive keeps one when it's a fixed disk Windows names a volume for (a `subst` letter has
//! none, and the Shell deletes through it for good) and neither Explorer's per-drive "Don't
//! move files to the Recycle Bin" (`NukeOnDelete`) nor the `NoRecycleFiles` policy turned it
//! off. Read live each time: a few calls and registry reads, no disk I/O, so a setting the user
//! changes counts at the next question. What a bin takes, its size limit, is per item:
//! [`DriveBin::limit_bytes`]. ❗ A public module of a capped crate (`DETAILS.md` § "The public
//! surface is capped"): its two items are all a caller gets.

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use cmdr_fs::app_path::AppPath;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
};

/// `GetDriveTypeW`'s `DRIVE_FIXED` (`WindowsProgramming`'s constant, a feature this crate
/// doesn't otherwise need).
const DRIVE_FIXED: u32 = 3;

/// The Recycle Bin of one drive, found by [`drive_bin`].
pub struct DriveBin {
    /// The NUL-terminated volume root (`C:\`).
    root: Vec<u16>,
    /// Explorer's per-drive bin settings key.
    settings: String,
}

/// Whether the drive holding the app path `path` (`C:/x`) keeps a Recycle Bin: what the
/// volume's `has_trash` asks.
pub(crate) fn has_recycle_bin_at(path: &AppPath) -> bool {
    bin_of(path.as_str().encode_utf16()).is_some()
}

/// The Recycle Bin of the drive holding `path`, a plain Win32 path in either spelling (`C:\x`
/// or `C:/x`, `\\server\share\x`; ❌ not the verbatim `\\?\` form), or `None`
/// when no bin would keep anything there: not a fixed disk, a drive Windows names no volume for
/// (a `subst` letter), a bin turned off, or a path Windows can't place on a volume at all.
pub fn drive_bin(path: &Path) -> Option<DriveBin> {
    bin_of(path.as_os_str().encode_wide())
}

fn bin_of(path: impl Iterator<Item = u16>) -> Option<DriveBin> {
    let root = volume_root(path)?;
    // SAFETY: `root` is the NUL-terminated root `GetVolumePathNameW` wrote.
    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return None;
    }
    let guid = volume_guid(&root)?;
    let settings = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{guid}");
    if dword(HKEY_CURRENT_USER, &settings, "NukeOnDelete") == Some(1) || policy("NoRecycleFiles") == Some(1) {
        return None;
    }
    Some(DriveBin { root, settings })
}

impl DriveBin {
    /// The bin's size limit: Explorer's per-drive `MaxCapacity` (MB), a policy's percentage of
    /// the drive, whichever is smaller; with neither written yet, 5% of the drive, at or below
    /// what Windows starts a drive's bin at. `None` when the drive's size can't be read.
    pub fn limit_bytes(&self) -> Option<u64> {
        const MIB: u64 = 1 << 20;
        let total = drive_bytes(&self.root)?;
        let from_setting = dword(HKEY_CURRENT_USER, &self.settings, "MaxCapacity").map(|mb| u64::from(mb) * MIB);
        let from_policy = policy("RecycleBinSize").map(|percent| total / 100 * u64::from(percent.min(100)));
        match (from_setting, from_policy) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => Some(total / 20),
        }
    }
}

/// The NUL-terminated root of the volume holding the path spelled `path` (`C:\`,
/// `\\server\share\`).
fn volume_root(path: impl Iterator<Item = u16>) -> Option<Vec<u16>> {
    // The app spelling's `/` as `\`, so a share's `//server/share/` reads as the UNC root too.
    let wide: Vec<u16> = path
        .map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        })
        .chain(Some(0))
        .collect();
    // The volume root is never longer than the path itself (plus its trailing `\`).
    let mut root = vec![0u16; wide.len().max(261) + 1];
    let root_len = u32::try_from(root.len()).ok()?;
    // SAFETY: `wide` is NUL-terminated and `root` holds `root_len` units.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root_len) } == 0 {
        return None;
    }
    Some(root)
}

/// A Recycle Bin policy value (`NoRecycleFiles`, `RecycleBinSize`), the user's first, then
/// the machine's.
fn policy(value: &str) -> Option<u32> {
    const POLICIES: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";
    dword(HKEY_CURRENT_USER, POLICIES, value).or_else(|| dword(HKEY_LOCAL_MACHINE, POLICIES, value))
}

/// The `{GUID}` of the volume at `root`, as the bin's settings key names it.
fn volume_guid(root: &[u16]) -> Option<String> {
    let mut name = [0u16; 64];
    let name_len = u32::try_from(name.len()).ok()?;
    // SAFETY: `root` is NUL-terminated (it ends in `\`, as the call wants) and `name` holds
    // `name_len` units.
    if unsafe { GetVolumeNameForVolumeMountPointW(root.as_ptr(), name.as_mut_ptr(), name_len) } == 0 {
        return None;
    }
    let end = name.iter().position(|&unit| unit == 0)?;
    // `\\?\Volume{7ee27290-…}\`
    let text = String::from_utf16(&name[..end]).ok()?;
    let start = text.find('{')?;
    let close = text.find('}')?;
    (start < close).then(|| text[start..=close].to_string())
}

/// The drive's size in bytes.
fn drive_bytes(root: &[u16]) -> Option<u64> {
    let mut total = 0u64;
    // SAFETY: `root` is NUL-terminated; the two out pointers we don't want are null, which
    // the call allows, and `total` outlives it.
    let ok = unsafe { GetDiskFreeSpaceExW(root.as_ptr(), std::ptr::null_mut(), &mut total, std::ptr::null_mut()) };
    (ok != 0 && total > 0).then_some(total)
}

/// A `REG_DWORD` value, or `None` when it isn't there.
fn dword(hive: HKEY, key: &str, value: &str) -> Option<u32> {
    let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let value: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let mut data = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).ok()?;
    // SAFETY: both names are NUL-terminated; `data` holds `size` bytes, and the type out
    // pointer we don't want is null, which the call allows.
    let status = unsafe {
        RegGetValueW(
            hive,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut data).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

#[cfg(test)]
#[path = "recycle_bin_test.rs"]
mod tests;
