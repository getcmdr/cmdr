//! Whether a drive's Recycle Bin can take an item, asked before the Shell is.
//!
//! ❗ The Shell announces a FILE too big for the bin as a recycle (`PreDeleteItem` with
//! `TSF_DELETE_RECYCLE_IF_POSSIBLE`), then deletes it for good and reports success with no
//! bin item, so the sink's veto never sees it. So a file is weighed here first, against
//! the bin's size limit as Explorer's own setting stores it. (verified on Windows 10 22H2,
//! NTFS, `windows_tests.rs` on the builder, 2026-11-21: a 64 GiB sparse file on `C:`, whose
//! bin holds 5,831 MB, came back `PostDeleteItem` `0x00270008` with no new item and was
//! gone from disk and from the bin; a folder holding it fell back to its children, which
//! the sink vetoed.)

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

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

/// What the drive's Recycle Bin would do with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bin {
    /// It takes it.
    Takes,
    /// There's no bin that would keep it: not a fixed disk, a drive Windows names no
    /// volume for (a `subst` letter), a bin turned off, or an item bigger than its limit.
    WouldDeleteForGood,
}

/// The NUL-terminated root of the volume holding `plain` (`C:\`, `\\server\share\`).
fn volume_root(plain: &Path) -> Option<Vec<u16>> {
    let wide: Vec<u16> = plain.as_os_str().encode_wide().chain(Some(0)).collect();
    // The volume root is never longer than the path itself (plus its trailing `\`).
    let mut root = vec![0u16; wide.len().max(261) + 1];
    let root_len = u32::try_from(root.len()).ok()?;
    // SAFETY: `wide` is NUL-terminated and `root` holds `root_len` units.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root_len) } == 0 {
        return None;
    }
    Some(root)
}

/// What the bin of the drive holding `plain` does with an item of `size` bytes (`None`
/// for a folder, which the Shell weighs itself: it falls back to the folder's children,
/// and the sink vetoes that).
pub(super) fn verdict(plain: &Path, size: Option<u64>) -> Bin {
    let Some(root) = volume_root(plain) else {
        // Windows can't name the volume, so nothing here can be weighed: the sink's veto
        // still guards a share or a stick, and a file is refused rather than guessed at.
        return if size.is_some() {
            Bin::WouldDeleteForGood
        } else {
            Bin::Takes
        };
    };
    // SAFETY: `root` is the NUL-terminated root `GetVolumePathNameW` wrote.
    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return Bin::WouldDeleteForGood;
    }
    let Some(guid) = volume_guid(&root) else {
        return Bin::WouldDeleteForGood;
    };
    let settings = settings_key(&guid);
    if dword(HKEY_CURRENT_USER, &settings, "NukeOnDelete") == Some(1) || policy("NoRecycleFiles") == Some(1) {
        return Bin::WouldDeleteForGood;
    }
    let Some(size) = size else {
        return Bin::Takes;
    };
    // At the limit still recycles, past it doesn't (verified on Windows 10 22H2:
    // a 5,831 MiB file into a 5,831 MB bin landed, one 1 MiB bigger was deleted for good).
    match limit_bytes(&root, &settings) {
        Some(limit) if size <= limit => Bin::Takes,
        _ => Bin::WouldDeleteForGood,
    }
}

/// The bin's limit for the drive holding `plain`, for a test to step past.
#[cfg(test)]
pub(super) fn limit_of(plain: &Path) -> Option<u64> {
    let root = volume_root(plain)?;
    let guid = volume_guid(&root)?;
    limit_bytes(&root, &settings_key(&guid))
}

/// Explorer's per-drive bin settings (`MaxCapacity`, `NukeOnDelete`), under HKCU.
fn settings_key(guid: &str) -> String {
    format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{guid}")
}

/// A Recycle Bin policy value (`NoRecycleFiles`, `RecycleBinSize`), the user's first, then
/// the machine's.
fn policy(value: &str) -> Option<u32> {
    const POLICIES: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";
    dword(HKEY_CURRENT_USER, POLICIES, value).or_else(|| dword(HKEY_LOCAL_MACHINE, POLICIES, value))
}

/// The bin's size limit: Explorer's per-drive `MaxCapacity` (MB), a policy's percentage of
/// the drive, whichever is smaller; with neither written yet, 5% of the drive, at or below
/// what Windows starts a drive's bin at.
fn limit_bytes(root: &[u16], settings: &str) -> Option<u64> {
    const MIB: u64 = 1 << 20;
    let total = drive_bytes(root)?;
    let from_setting = dword(HKEY_CURRENT_USER, settings, "MaxCapacity").map(|mb| u64::from(mb) * MIB);
    let from_policy = policy("RecycleBinSize").map(|percent| total / 100 * u64::from(percent.min(100)));
    match (from_setting, from_policy) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => Some(total / 20),
    }
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
