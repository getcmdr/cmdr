//! The Win32 reads behind Windows' volume list: every drive letter and folder mount, what
//! each one is, and the facts its id comes from (plan D8). The id ladder itself is pure and
//! lives in `cmdr_fs::volume::windows_volume_ids`, tested on every OS.
//!
//! What Windows 10 22H2 answers (verified on the builder, `ssh mami`, PowerShell P/Invoke of
//! each call, 2026-11-18):
//!
//! - A fixed or removable drive: `GetVolumeNameForVolumeMountPointW` gives its GUID path
//!   (`\\?\Volume{7ee27290-…}\`); `QueryDosDeviceW` names a device
//!   (`\Device\HarddiskVolume4`). A USB SSD reports `DRIVE_FIXED`.
//! - A `subst` drive: `GetVolumeNameForVolumeMountPointW` fails with
//!   `ERROR_INVALID_PARAMETER` (87); `QueryDosDeviceW` gives `\??\C:\Windows`;
//!   `GetVolumeInformationW` reports the TARGET drive's serial, so a serial would merge it
//!   with that drive.
//! - A mapped drive (`net use N: \\localhost\C$`): `DRIVE_REMOTE`, no GUID (87),
//!   `WNetGetConnectionW` gives `\\localhost\C$`, and `GetVolumeInformationW` reports the
//!   SERVER's filesystem and serial. A bare UNC root answers the same, minus the letter.
//! - An empty optical drive: `DRIVE_CDROM` with a GUID, and `GetVolumeInformationW` fails
//!   with `ERROR_NOT_READY` (21); it lists no row until media is in.
//! - `FindFirstVolumeW` also lists the EFI and recovery partitions, with no path names.
//!
//! The letters and the GUID sweep come from the mount table (`cmdr_local::mounts`), the same
//! reads the write engine's mount guards ask, so the drive list and the guards can't disagree
//! on what's mounted where.
//!
//! ❗ `GetVolumeInformationW` is asked only of a local drive: on a mapped drive it's a
//! network round trip, which blocks on a dead server.

use std::os::windows::ffi::OsStrExt;

use cmdr_fs::volume::windows_volume_ids::{
    WindowsDrive, WindowsDriveKind, WindowsVolumeIdentity, subst_target, unc_app_root, unc_server_share,
    windows_volume_id,
};
use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, NO_ERROR};
use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, QueryDosDeviceW,
};

/// `GetDriveTypeW`'s answers (`WindowsProgramming`'s constants, a feature the app doesn't
/// otherwise need).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;

/// `GetVolumeInformationW`'s flag for a volume mounted read-only.
const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

/// A volume GUID path is 49 UTF-16 units plus its NUL.
const GUID_PATH_LEN: usize = 50;

/// Every volume root Windows lists right now: each drive letter, each folder mount, and each
/// mapped drive's UNC spelling as an alias of the same volume. `system_root` is the system
/// drive's app root (`C:/`), which gets `root`.
pub(super) fn enumerate(system_root: &str) -> Vec<WindowsDrive> {
    let mut drives: Vec<WindowsDrive> = Vec::new();
    for letter in drive_letters() {
        if let Some(drive) = letter_drive(letter, system_root) {
            if let Some(share_root) = drive.share_root.clone() {
                drives.push(WindowsDrive {
                    root: share_root,
                    is_alias: true,
                    share_root: None,
                    ..drive.clone()
                });
            }
            drives.push(drive);
        }
    }
    drives.extend(folder_mounts(system_root));
    drives
}

/// The facts for the UNC root `root` (`//server/share/`) nobody mapped: a share of its own,
/// keyed like a mapped one. Pure apart from the system root.
pub(super) fn unc_drive(root: &str, system_root: &str) -> Option<WindowsDrive> {
    let app_root = unc_app_root(root)?;
    let (_, share) = unc_server_share(root)?;
    Some(WindowsDrive {
        id: windows_volume_id(
            &app_root,
            &WindowsVolumeIdentity::Share { unc: app_root.clone() },
            system_root,
        ),
        root: app_root,
        kind: WindowsDriveKind::Network,
        label: Some(share),
        share_root: None,
        is_alias: false,
        fs_type: None,
        read_only: false,
    })
}

/// The letters in use, from the mount table (`cmdr_local::mounts`), none when it can't be read.
fn drive_letters() -> Vec<char> {
    cmdr_local::mounts::platform::drive_letters().unwrap_or_default()
}

/// One drive letter's row, or `None` for one with nothing usable behind it (an empty card
/// reader or optical drive, a letter Windows can't type).
fn letter_drive(letter: char, system_root: &str) -> Option<WindowsDrive> {
    let root = format!("{letter}:/");
    let native_root = wide(&format!("{letter}:\\"));
    // SAFETY: `native_root` is NUL-terminated.
    let drive_type = unsafe { GetDriveTypeW(native_root.as_ptr()) };
    if drive_type == DRIVE_REMOTE {
        let unc = mapped_share(letter);
        let share_root = unc.as_deref().and_then(unc_app_root);
        let label = unc.as_deref().and_then(unc_server_share).map(|(_, share)| share);
        let identity = unc.map_or(WindowsVolumeIdentity::Unnamed, |unc| WindowsVolumeIdentity::Share {
            unc,
        });
        return Some(WindowsDrive {
            id: windows_volume_id(&root, &identity, system_root),
            root,
            kind: WindowsDriveKind::Network,
            label,
            share_root,
            is_alias: false,
            fs_type: None,
            read_only: false,
        });
    }
    let kind = match drive_type {
        DRIVE_FIXED => WindowsDriveKind::Fixed,
        DRIVE_REMOVABLE => WindowsDriveKind::Removable,
        DRIVE_CDROM => WindowsDriveKind::Optical,
        DRIVE_RAMDISK => WindowsDriveKind::Ram,
        _ => return None,
    };
    let info = volume_information(&native_root)?;
    let (kind, identity) = match volume_guid_path(&native_root) {
        Some(guid_path) => (kind, WindowsVolumeIdentity::Local { guid_path }),
        // No GUID: a `subst` drive when its device is a folder, else a drive Windows won't name.
        None if subst_of(letter).is_some() => (WindowsDriveKind::Subst, WindowsVolumeIdentity::Unnamed),
        None => (kind, WindowsVolumeIdentity::Unnamed),
    };
    Some(WindowsDrive {
        id: windows_volume_id(&root, &identity, system_root),
        root,
        kind,
        label: info.label,
        share_root: None,
        is_alias: false,
        fs_type: info.fs_type,
        read_only: info.read_only,
    })
}

/// Every volume mounted in a folder rather than (or as well as) on a letter, from the mount
/// table's GUID sweep (`cmdr_local::mounts`), which also names each volume's letters (skipped
/// here: [`letter_drive`] has them).
fn folder_mounts(system_root: &str) -> Vec<WindowsDrive> {
    let mut out = Vec::new();
    for volume in cmdr_local::mounts::platform::volumes().unwrap_or_default() {
        let guid_path = volume.guid_path;
        for path in volume.mount_paths {
            let app = path.replace('\\', "/");
            let app = app.trim_end_matches('/');
            let is_letter_root = app.len() == 2;
            if is_letter_root || app.is_empty() {
                continue;
            }
            let native_root = wide(&path);
            let Some(info) = volume_information(&native_root) else {
                continue;
            };
            // SAFETY: `native_root` is NUL-terminated.
            let kind = match unsafe { GetDriveTypeW(native_root.as_ptr()) } {
                DRIVE_REMOVABLE => WindowsDriveKind::Removable,
                DRIVE_CDROM => WindowsDriveKind::Optical,
                _ => WindowsDriveKind::Fixed,
            };
            let root = uppercase_drive(app);
            out.push(WindowsDrive {
                id: windows_volume_id(
                    &root,
                    &WindowsVolumeIdentity::Local {
                        guid_path: guid_path.clone(),
                    },
                    system_root,
                ),
                root,
                kind,
                label: info.label,
                share_root: None,
                is_alias: false,
                fs_type: info.fs_type,
                read_only: info.read_only,
            });
        }
    }
    out
}

/// `c:/mnt/data` as `C:/mnt/data`: the app path's drive letter is always uppercase (D1).
fn uppercase_drive(app: &str) -> String {
    let mut chars = app.chars();
    match chars.next() {
        Some(letter) => letter.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// What `GetVolumeInformationW` says about a local volume.
struct VolumeFacts {
    label: Option<String>,
    fs_type: Option<String>,
    read_only: bool,
}

/// The label, filesystem, and read-only flag of the local volume at `native_root`, or `None`
/// when it isn't ready (an empty card reader or optical drive answers `ERROR_NOT_READY`).
fn volume_information(native_root: &[u16]) -> Option<VolumeFacts> {
    let mut label = [0u16; 261];
    let mut fs_name = [0u16; 261];
    let mut flags = 0u32;
    // SAFETY: `native_root` is NUL-terminated; `label` and `fs_name` hold the lengths passed;
    // `flags` is a valid out pointer, and the serial and component length aren't wanted (null).
    let ok = unsafe {
        GetVolumeInformationW(
            native_root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let non_empty = |text: String| Some(text).filter(|t| !t.is_empty());
    Some(VolumeFacts {
        label: non_empty(from_wide(&label)),
        fs_type: non_empty(from_wide(&fs_name)),
        read_only: flags & FILE_READ_ONLY_VOLUME != 0,
    })
}

/// The volume GUID path of the drive at `native_root`, `None` for a `subst` or network drive.
fn volume_guid_path(native_root: &[u16]) -> Option<String> {
    let mut name = [0u16; GUID_PATH_LEN];
    // SAFETY: `native_root` is NUL-terminated; `name` holds `GUID_PATH_LEN` units.
    let ok =
        unsafe { GetVolumeNameForVolumeMountPointW(native_root.as_ptr(), name.as_mut_ptr(), GUID_PATH_LEN as u32) };
    (ok != 0).then(|| from_wide(&name))
}

/// The folder `letter` is a `subst` for, as an app path, or `None` for a real drive.
fn subst_of(letter: char) -> Option<String> {
    let device = wide(&format!("{letter}:"));
    let mut target = vec![0u16; 1024];
    // SAFETY: `device` is NUL-terminated; `target` holds its length in units.
    let written = unsafe {
        QueryDosDeviceW(
            device.as_ptr(),
            target.as_mut_ptr(),
            u32::try_from(target.len()).unwrap_or(u32::MAX),
        )
    } as usize;
    if written == 0 || written > target.len() {
        return None;
    }
    subst_target(&from_wide(&target[..written]))
}

/// The share a mapped drive points at (`\\server\share`), from the redirector's own table:
/// no round trip to the server, so a dead one can't block it. A remembered drive that isn't
/// connected right now answers too.
fn mapped_share(letter: char) -> Option<String> {
    /// `WNetGetConnectionW`'s answer for a remembered drive that isn't connected now.
    const ERROR_CONNECTION_UNAVAIL: u32 = 1201;
    let local = wide(&format!("{letter}:"));
    let mut remote = vec![0u16; 512];
    for _ in 0..2 {
        let mut len = u32::try_from(remote.len()).unwrap_or(u32::MAX);
        // SAFETY: `local` is NUL-terminated; `remote` holds `len` units, the length the call
        // reads and, on `ERROR_MORE_DATA`, overwrites with the size it needs.
        let status = unsafe { WNetGetConnectionW(local.as_ptr(), remote.as_mut_ptr(), &mut len) };
        match status {
            NO_ERROR | ERROR_CONNECTION_UNAVAIL => return Some(from_wide(&remote)).filter(|s| !s.is_empty()),
            ERROR_MORE_DATA if (len as usize) > remote.len() => remote = vec![0u16; len as usize],
            _ => return None,
        }
    }
    None
}

/// `text` as a NUL-terminated wide string.
fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

/// A wide buffer up to its first NUL.
fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
