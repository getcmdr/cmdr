//! Windows: a drive ejects through `drive_flight` (task 39b), by the route
//! `cmdr_fs::volume::windows_eject` picks for it; this file holds the calls each route makes and
//! the [`DriveSide`] the flight drives. No unmount tool exists here: a tool run (an SMB share's
//! unmount, which no Windows volume reaches yet) answers that none could start, which the eject
//! reports as `Unexpected`. Device providers (MTP, ADB) and remote sessions (SFTP, WebDAV) never
//! reach this seam, so their ejects work as on every OS.
//!
//! Every call here runs on the flight's own `drive-eject` thread, ❌ never the main thread, and
//! the flight bounds it (`drive_flight::REQUEST_DEADLINE`).
//!
//! - **`RemoveDevice`**: `CM_Request_Device_EjectW` on the unit's removable device node
//!   (`volumes/platform/windows/device_tree.rs` finds it and every volume under it), with a veto
//!   buffer, which tells Windows not to show its own message (learn.microsoft.com
//!   `CM_Request_Device_EjectW`, read 2026-11-25): Cmdr words the outcome itself.
//! - **`EjectMedia`** (a disc): `FSCTL_LOCK_VOLUME` (refused while anyone holds a file there,
//!   retried over `REFUSAL_RETRY_BACKOFF`, which is what keeps it honest), `FSCTL_DISMOUNT_VOLUME`,
//!   `IOCTL_STORAGE_MEDIA_REMOVAL` off, `IOCTL_STORAGE_EJECT_MEDIA` (KB 165721's sequence).
//! - **`DetachImage`** (a mounted VHD, VHDX, or ISO): the same lock and dismount, then the backing
//!   file from `GetStorageDependencyInformation` and `DetachVirtualDisk` on it, with the lock
//!   held until the detach returns. A denied open, lock, or detach is `NeedsRights` (a VHD on a
//!   fixed volume needs an administrator), ❌ never a holder's veto.

use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::sync::Arc;

use cmdr_fs::volume::windows_eject::{EjectRoute, RequestVerdict, request_verdict};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::CM_Request_Device_EjectW;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, HANDLE, MAX_PATH,
};
use windows_sys::Win32::Storage::Vhd::{
    DETACH_VIRTUAL_DISK_FLAG_NONE, DetachVirtualDisk, GET_STORAGE_DEPENDENCY_FLAG_NONE,
    GetStorageDependencyInformation, OPEN_VIRTUAL_DISK_FLAG_NONE, OpenVirtualDisk, STORAGE_DEPENDENCY_INFO,
    STORAGE_DEPENDENCY_INFO_VERSION_2, VIRTUAL_DISK_ACCESS_DETACH,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_MEDIA_REMOVAL,
    PREVENT_MEDIA_REMOVAL,
};

use super::super::unmount_tool::{ToolOutcome, UnmountVerb};
use super::super::{EjectError, EjectOutcome};
use super::drive_flight::{DriveSide, RequestAnswer};
use crate::volumes::UnitKey;
use crate::volumes::storage::{ioctl, open_volume, volume_device};

/// Cmdr makes no OS SMB mount on Windows yet (`WNetAddConnection2W` is the SMB mounts task), so a
/// disconnect drops the direct session, as on Linux.
pub(crate) const SMB_DISCONNECT_UNMOUNTS: bool = false;

pub(crate) fn tool_command(_verb: UnmountVerb) -> &'static str {
    "unmount (no tool on Windows)"
}

pub(crate) fn run_tool_blocking(_verb: UnmountVerb, _mount_path: &str) -> ToolOutcome {
    ToolOutcome::of(Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows has no unmount tool; a drive ejects through its drive flight",
    )))
}

/// The switcher row's own answer (`volumes::eject_target` behind both), so the eject never
/// refuses a drive its row offered to eject.
pub(crate) fn is_ejectable(mount_path: &str) -> bool {
    crate::volumes::resolve_path_volume_fast(mount_path).is_some_and(|v| v.is_ejectable)
}

/// Always `false`: Windows has no unmount tool, and a mapped share is an SMB volume, which goes
/// by its backend kind.
pub(crate) fn is_network_mount(_mount_path: &str) -> bool {
    false
}

/// An ejectable drive's eject: its unit, then its drive flight, both on the blocking pool.
///
/// 1. The unit (`volumes::eject_target`: every volume the request takes down) is read under
///    `DISK_RESOLVE_DEADLINE`: it opens devices, and a wedged one answers `NotResponding`.
/// 2. The unit claims its device like a macOS disk flight claims its disk
///    (`in_flight::disk::join_or_own_disk`): its other volumes join this flight in the ejecting
///    set, so their own ejects join it and no transfer starts on them, and a unit another flight
///    already owns is joined instead.
/// 3. The flight runs; a timeout keeps the ownership (and the asked volume, `hold_past_landing`)
///    until the request truly ends.
pub(in crate::file_system::volume::eject) async fn eject_ejectable(
    volume_id: &str,
    mount_path: &str,
) -> Result<EjectOutcome, EjectError> {
    use super::super::in_flight::{DiskKey, disk};
    use super::super::{EjectStep, deadlines};

    let path = mount_path.to_string();
    let target = deadlines::within_deadline(
        EjectStep::DiskResolve,
        volume_id,
        deadlines::DISK_RESOLVE_DEADLINE,
        async move { tokio::task::spawn_blocking(move || crate::volumes::eject_target(&path)).await },
    )
    .await?
    .map_err(|join_err| EjectError::Unexpected {
        detail: format!("{}: {join_err}", EjectStep::DiskResolve),
    })?;
    let Some(target) = target else {
        return Err(EjectError::NotEjectable {
            volume_id: volume_id.to_string(),
        });
    };
    // The registry's id leads, whatever the table spelled: it's the one in the ejecting set.
    let mut unit: Vec<(String, String)> = target.unit;
    if let Some(first) = unit.first_mut() {
        first.0 = volume_id.to_string();
    }
    let ids: Vec<String> = unit.iter().map(|(id, _)| id.clone()).collect();
    let Some(key) = target.key else {
        return Err(EjectError::Unexpected {
            detail: "no removable device above the disk".to_owned(),
        });
    };
    let mut ownership = match disk::join_or_own_disk(DiskKey(key.as_u64()), volume_id, &ids) {
        disk::DiskFlight::Owner(ownership) => ownership,
        // Another flight is taking this unit down: its answer is this one's.
        disk::DiskFlight::Joined(flight) | disk::DiskFlight::DecideAgainAfter(flight) => return flight.await,
    };
    if !ownership.adopted().is_empty() {
        log::info!(target: "eject", "Ejecting {volume_id} also takes down: {:?}", ownership.adopted());
    }
    let side = Arc::new(WindowsDriveSide {
        unit,
        route: target.route,
        key,
        locks_for_writing: target.locks_for_writing,
    });
    let asked = volume_id.to_string();
    let ended = tokio::task::spawn_blocking(move || {
        super::drive_flight::eject_drive(
            crate::file_system::volume::drive_release::gate(),
            &ids,
            side,
            super::super::is_busy,
            super::super::stop_removable_index,
            super::drive_flight::REQUEST_DEADLINE,
            // The ownership lives as long as the flight, or, past a timeout, as long as the request.
            Box::new(move || {
                ownership.hold_past_landing(&asked);
                Box::new(move || drop(ownership))
            }),
        )
        .answer
    })
    .await;
    ended.unwrap_or_else(|join_err| {
        Err(EjectError::Unexpected {
            detail: format!("the drive eject ended without an answer: {join_err}"),
        })
    })
}

/// One unit Cmdr's eject takes away, as the flight sees it.
struct WindowsDriveSide {
    /// `(volume id, app root)` of every volume the request takes down, the one asked about first.
    unit: Vec<(String, String)>,
    route: EjectRoute,
    key: UnitKey,
    locks_for_writing: bool,
}

impl DriveSide for WindowsDriveSide {
    fn let_go(&self) {
        for (volume_id, root) in &self.unit {
            crate::volumes::let_go_for_eject(volume_id, root);
        }
    }

    fn request(&self) -> RequestAnswer {
        let Some((volume_id, root)) = self.unit.first() else {
            return not_possible("an eject with no volume");
        };
        log::info!(target: "eject", "Asking Windows to eject {volume_id} ({:?})", self.route);
        match self.route {
            EjectRoute::RemoveDevice => match self.key {
                UnitKey::Device(node) => remove_device(node),
                UnitKey::Disk { .. } => not_possible("no removable device above the disk"),
            },
            EjectRoute::EjectMedia | EjectRoute::DetachImage => {
                let Some(device) = volume_device(root) else {
                    return not_possible("the drive has no volume device any more");
                };
                if self.route == EjectRoute::EjectMedia {
                    eject_media(&device)
                } else {
                    detach_image(&device, self.locks_for_writing)
                }
            }
        }
    }

    fn take_back(&self) {
        for (volume_id, root) in &self.unit {
            crate::volumes::take_back_after_eject(volume_id, root);
        }
    }

    fn is_listed(&self, volume_id: &str) -> bool {
        self.unit
            .iter()
            .find(|(id, _)| id == volume_id)
            .is_some_and(|(_, root)| super::super::is_still_mounted(root))
    }
}

fn not_possible(detail: impl Into<String>) -> RequestAnswer {
    RequestAnswer::NotPossible { detail: detail.into() }
}

// ── RemoveDevice ──────────────────────────────────────────────────────

/// Asks Windows to remove the device at `node` (the unit's removable ancestor,
/// `volumes/platform/windows/device_tree.rs`).
fn remove_device(node: u32) -> RequestAnswer {
    let mut veto_type = 0;
    let mut veto_name = [0u16; MAX_PATH as usize];
    // SAFETY: `node` is a device node the configuration manager gave for a device still present;
    // the veto outputs are valid for the call, the name holding `MAX_PATH` units as the docs ask.
    let configret = unsafe { CM_Request_Device_EjectW(node, &mut veto_type, veto_name.as_mut_ptr(), MAX_PATH, 0) };
    let name = from_wide(&veto_name);
    let detail = format!("CONFIGRET {configret}, veto type {veto_type} {name}");
    match request_verdict(configret, veto_type) {
        RequestVerdict::Ejected => RequestAnswer::Ejected,
        RequestVerdict::Vetoed => RequestAnswer::Vetoed { detail },
        RequestVerdict::NeedsRights => RequestAnswer::NeedsRights { detail },
        RequestVerdict::NotPossible => not_possible(detail),
    }
}

// ── EjectMedia and DetachImage ────────────────────────────────────────

/// A volume Cmdr locked and dismounted: nobody holds a file there, and nothing reads it until
/// the handle closes.
struct Dismounted(OwnedHandle);

/// Opens the volume, locks it (retrying a transient holder), and dismounts it. A refused lock is
/// a holder's veto: ❌ never forced.
fn lock_and_dismount(device: &[u16], access: u32) -> Result<Dismounted, RequestAnswer> {
    let volume = open_volume(device, access).map_err(|code| match code {
        // The open asks no holder anything: a denial is about rights (a fixed volume opened for
        // writing needs an administrator), ❌ never a veto.
        ERROR_ACCESS_DENIED => RequestAnswer::NeedsRights {
            detail: format!("opening the volume for its lock was denied (error {code})"),
        },
        _ => not_possible(format!("couldn't open the volume (error {code})")),
    })?;
    let mut pauses = super::super::unmount_tool::REFUSAL_RETRY_BACKOFF.iter();
    loop {
        // SAFETY: `FSCTL_LOCK_VOLUME` takes no input and no output.
        match unsafe { ioctl::<(), ()>(&volume, FSCTL_LOCK_VOLUME, None, None) } {
            Ok(_) => break,
            Err(code @ (ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)) => match pauses.next() {
                Some(pause) => std::thread::sleep(*pause),
                None => {
                    return Err(RequestAnswer::Vetoed {
                        detail: format!("the volume lock was refused (error {code}): a file there is open"),
                    });
                }
            },
            Err(code) => return Err(not_possible(format!("the volume lock wasn't possible (error {code})"))),
        }
    }
    // SAFETY: `FSCTL_DISMOUNT_VOLUME` takes no input and no output.
    unsafe { ioctl::<(), ()>(&volume, FSCTL_DISMOUNT_VOLUME, None, None) }
        .map_err(|code| not_possible(format!("the dismount wasn't possible (error {code})")))?;
    Ok(Dismounted(volume))
}

fn eject_media(device: &[u16]) -> RequestAnswer {
    // KB 165721 opens a CD-ROM volume for reading: a disc has nothing to write.
    let Dismounted(volume) = match lock_and_dismount(device, GENERIC_READ) {
        Ok(dismounted) => dismounted,
        Err(answer) => return answer,
    };
    let allow = PREVENT_MEDIA_REMOVAL {
        PreventMediaRemoval: false,
    };
    // SAFETY: `IOCTL_STORAGE_MEDIA_REMOVAL` takes a `PREVENT_MEDIA_REMOVAL`, no output.
    if let Err(code) = unsafe { ioctl::<_, ()>(&volume, IOCTL_STORAGE_MEDIA_REMOVAL, Some(&allow), None) } {
        return not_possible(format!("the drive kept its media locked (error {code})"));
    }
    // SAFETY: `IOCTL_STORAGE_EJECT_MEDIA` takes no input and no output.
    match unsafe { ioctl::<(), ()>(&volume, IOCTL_STORAGE_EJECT_MEDIA, None, None) } {
        Ok(_) => RequestAnswer::Ejected,
        Err(code) => not_possible(format!("the drive wouldn't eject its media (error {code})")),
    }
}

fn detach_image(device: &[u16], locks_for_writing: bool) -> RequestAnswer {
    // A VHD's volume locks for writing (an administrator's right on a fixed volume); an ISO's is
    // read-only. ❌ No second try with less access: after a veto it would rerun the whole backoff,
    // and after a denial it would lock nothing a VHD's writers respect.
    let access = if locks_for_writing {
        GENERIC_READ | GENERIC_WRITE
    } else {
        GENERIC_READ
    };
    // ❗ Held until the detach returns: closed earlier, another app could reopen the volume and
    // write between the dismount and the detach.
    let Dismounted(volume) = match lock_and_dismount(device, access) {
        Ok(dismounted) => dismounted,
        Err(answer) => return answer,
    };
    let Some((storage_type, backing)) = backing_file(&volume) else {
        return not_possible("Windows named no image file behind the volume");
    };
    let mut image: HANDLE = std::ptr::null_mut();
    // SAFETY: `storage_type` and `backing` (NUL-terminated) outlive the call; no parameters is
    // the documented default for a detach.
    let opened = unsafe {
        OpenVirtualDisk(
            &storage_type,
            backing.as_ptr(),
            VIRTUAL_DISK_ACCESS_DETACH,
            OPEN_VIRTUAL_DISK_FLAG_NONE,
            std::ptr::null(),
            &mut image,
        )
    };
    match opened {
        0 => {}
        ERROR_ACCESS_DENIED => {
            return RequestAnswer::NeedsRights {
                detail: format!("opening the image file for its detach was denied (error {opened})"),
            };
        }
        other => return not_possible(format!("couldn't open the image file (error {other})")),
    }
    // SAFETY: `image` is the handle `OpenVirtualDisk` just gave, owned here.
    let image = unsafe { <OwnedHandle as std::os::windows::io::FromRawHandle>::from_raw_handle(image) };
    // SAFETY: a virtual disk handle opened for detaching.
    let detached = unsafe { DetachVirtualDisk(image.as_raw_handle() as HANDLE, DETACH_VIRTUAL_DISK_FLAG_NONE, 0) };
    drop(volume);
    match detached {
        0 => RequestAnswer::Ejected,
        ERROR_ACCESS_DENIED => RequestAnswer::NeedsRights {
            detail: format!("the image's detach was denied (error {detached})"),
        },
        ERROR_SHARING_VIOLATION => RequestAnswer::Vetoed {
            detail: format!("the image's detach was refused (error {detached})"),
        },
        other => not_possible(format!("the image wouldn't detach (error {other})")),
    }
}

/// The image file a virtual disk's volume is backed by, and its storage type.
fn backing_file(volume: &OwnedHandle) -> Option<(windows_sys::Win32::Storage::Vhd::VIRTUAL_STORAGE_TYPE, Vec<u16>)> {
    // The structure is followed by the strings its entries point into.
    let mut buffer = vec![0u64; 512];
    let size = u32::try_from(buffer.len() * size_of::<u64>()).ok()?;
    let info = buffer.as_mut_ptr().cast::<STORAGE_DEPENDENCY_INFO>();
    // SAFETY: `buffer` is 8-byte aligned and larger than a `STORAGE_DEPENDENCY_INFO`; its
    // version is set before the call reads it.
    unsafe { (*info).Version = STORAGE_DEPENDENCY_INFO_VERSION_2 };
    let mut used = 0u32;
    // SAFETY: `volume` is an open volume handle; `info` is valid for `size` bytes.
    let status = unsafe {
        GetStorageDependencyInformation(
            volume.as_raw_handle() as HANDLE,
            GET_STORAGE_DEPENDENCY_FLAG_NONE,
            size,
            info,
            &mut used,
        )
    };
    // SAFETY: the call filled the header; an entry is read only when it says there is one.
    let entries = unsafe { (*info).NumberEntries };
    if status != 0 || entries == 0 {
        return None;
    }
    // SAFETY: entry 0 exists (above), in the version-2 layout asked for; its strings point into
    // `buffer`, which is alive.
    let entry = unsafe { (*info).Anonymous.Version2Entries[0] };
    // SAFETY: both are NUL-terminated strings inside `buffer`, or null.
    let host = unsafe { wide_at(entry.HostVolumeName) }?;
    // SAFETY: as above.
    let relative = unsafe { wide_at(entry.DependentVolumeRelativePath) }?;
    let mut path = host;
    if !path.ends_with('\\') {
        path.push('\\');
    }
    path.push_str(relative.trim_start_matches('\\'));
    Some((entry.VirtualStorageType, path.encode_utf16().chain(Some(0)).collect()))
}

/// The NUL-terminated wide string at `ptr`, or `None` for null.
///
/// # Safety
///
/// `ptr` is null or points at a NUL-terminated UTF-16 string that stays alive for the call.
unsafe fn wide_at(ptr: windows_sys::core::PWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: the caller promises a NUL-terminated string; reading stops at its NUL.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` units before the NUL are readable (above).
    let units = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(std::ffi::OsString::from_wide(units).to_string_lossy().into_owned())
}

fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
