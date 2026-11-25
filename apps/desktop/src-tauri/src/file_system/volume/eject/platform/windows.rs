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
//! - **`RemoveDevice`**: the volume's `IOCTL_STORAGE_GET_DEVICE_NUMBER`, the disk (or CD-ROM)
//!   interface with the same number (`CM_Get_Device_Interface_ListW`), its device node
//!   (`DEVPKEY_Device_InstanceId`, `CM_Locate_DevNodeW`), then up the tree (`CM_Get_Parent`) to
//!   the first node with `DN_REMOVABLE` (`CM_Get_DevNode_Status`): the USB mass-storage device,
//!   not the disk under it, so the whole stick goes, as the tray's "Safely remove" does (the walk
//!   Uwe Sieber's RemoveDrive documents, uwe-sieber.de/drivetools_e.html, read 2026-11-25).
//!   Then `CM_Request_Device_EjectW` with a veto buffer, which tells Windows not to show its own
//!   message (learn.microsoft.com `CM_Request_Device_EjectW`, read 2026-11-25): Cmdr words the
//!   outcome itself.
//! - **`EjectMedia`** (a disc): `FSCTL_LOCK_VOLUME` (refused while anyone holds a file there,
//!   retried over `REFUSAL_RETRY_BACKOFF`, which is what keeps it honest), `FSCTL_DISMOUNT_VOLUME`,
//!   `IOCTL_STORAGE_MEDIA_REMOVAL` off, `IOCTL_STORAGE_EJECT_MEDIA` (KB 165721's sequence).
//! - **`DetachImage`** (a mounted VHD, VHDX, or ISO): the same lock and dismount, then the backing
//!   file from `GetStorageDependencyInformation` and `DetachVirtualDisk` on it. The lock handle
//!   closes before the detach: an open handle on the volume would veto the disk's own removal.

use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::sync::Arc;

use cmdr_fs::volume::windows_eject::{EjectRoute, RequestVerdict, request_verdict};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_DevNode_Status, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_PropertyW, CM_Get_Parent, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CM_Request_Device_EjectW, CR_SUCCESS, DN_REMOVABLE,
};
use windows_sys::Win32::Devices::Properties::DEVPKEY_Device_InstanceId;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, HANDLE, MAX_PATH,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_DEVICE_CD_ROM, FILE_DEVICE_DISK};
use windows_sys::Win32::Storage::Vhd::{
    DETACH_VIRTUAL_DISK_FLAG_NONE, DetachVirtualDisk, GET_STORAGE_DEPENDENCY_FLAG_NONE,
    GetStorageDependencyInformation, OPEN_VIRTUAL_DISK_FLAG_NONE, OpenVirtualDisk, STORAGE_DEPENDENCY_INFO,
    STORAGE_DEPENDENCY_INFO_VERSION_2, VIRTUAL_DISK_ACCESS_DETACH,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, GUID_DEVINTERFACE_CDROM, GUID_DEVINTERFACE_DISK,
    IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_GET_DEVICE_NUMBER, IOCTL_STORAGE_MEDIA_REMOVAL, PREVENT_MEDIA_REMOVAL,
    STORAGE_DEVICE_NUMBER,
};

use super::super::unmount_tool::{ToolOutcome, UnmountVerb};
use super::super::{EjectError, EjectOutcome};
use super::drive_flight::{DriveSide, RequestAnswer};
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

/// An ejectable drive's eject: its drive flight, on the blocking pool, since the flight blocks up
/// to its index stop's and its request's deadlines together.
pub(in crate::file_system::volume::eject) async fn eject_ejectable(
    volume_id: &str,
    mount_path: &str,
) -> Result<EjectOutcome, EjectError> {
    let Some(target) = crate::volumes::eject_target(mount_path) else {
        return Err(EjectError::NotEjectable {
            volume_id: volume_id.to_string(),
        });
    };
    let side = Arc::new(WindowsDriveSide {
        volume_id: volume_id.to_string(),
        root: target.root,
        route: target.route,
    });
    let id = volume_id.to_string();
    let ended = tokio::task::spawn_blocking(move || {
        super::drive_flight::eject_drive(
            crate::file_system::volume::drive_release::gate(),
            &id,
            side,
            || super::super::is_busy(&id),
            super::super::stop_removable_index,
            super::drive_flight::REQUEST_DEADLINE,
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

/// One drive Cmdr's eject takes away, as the flight sees it.
struct WindowsDriveSide {
    volume_id: String,
    /// Its app root (`E:/`).
    root: String,
    route: EjectRoute,
}

impl DriveSide for WindowsDriveSide {
    fn let_go(&self) {
        crate::volumes::let_go_for_eject(&self.volume_id, &self.root);
    }

    fn request(&self) -> RequestAnswer {
        let Some(device) = volume_device(&self.root) else {
            return RequestAnswer::NotPossible {
                detail: "the drive has no volume device any more".to_owned(),
            };
        };
        log::info!(target: "eject", "Asking Windows to eject {} ({:?})", self.volume_id, self.route);
        match self.route {
            EjectRoute::RemoveDevice => remove_device(&device),
            EjectRoute::EjectMedia => eject_media(&device),
            EjectRoute::DetachImage => detach_image(&device),
        }
    }

    fn take_back(&self) {
        crate::volumes::take_back_after_eject(&self.volume_id, &self.root);
    }

    fn is_listed(&self) -> bool {
        super::super::is_still_mounted(&self.root)
    }
}

fn not_possible(detail: impl Into<String>) -> RequestAnswer {
    RequestAnswer::NotPossible { detail: detail.into() }
}

// ── RemoveDevice ──────────────────────────────────────────────────────

fn remove_device(device: &[u16]) -> RequestAnswer {
    let number = {
        let volume = match open_volume(device, 0) {
            Ok(volume) => volume,
            Err(code) => return not_possible(format!("couldn't open the volume (error {code})")),
        };
        match device_number(&volume) {
            Ok(number) => number,
            Err(code) => return not_possible(format!("the volume named no disk (error {code})")),
        }
        // ❗ The volume handle closes here: a handle of Cmdr's own would veto the eject.
    };
    let class = match number.DeviceType {
        FILE_DEVICE_DISK => GUID_DEVINTERFACE_DISK,
        FILE_DEVICE_CD_ROM => GUID_DEVINTERFACE_CDROM,
        other => return not_possible(format!("device type {other} has no disk interface")),
    };
    let Some(disk) = devnode_of(&class, number.DeviceNumber) else {
        return not_possible(format!("no device node for disk {}", number.DeviceNumber));
    };
    let Some(removable) = removable_ancestor(disk) else {
        return not_possible("no removable device above the disk");
    };
    let mut veto_type = 0;
    let mut veto_name = [0u16; MAX_PATH as usize];
    // SAFETY: `removable` is a device node `CM_Locate_DevNodeW` / `CM_Get_Parent` gave; the veto
    // outputs are valid for the call, the name holding `MAX_PATH` units as the docs ask.
    let configret = unsafe { CM_Request_Device_EjectW(removable, &mut veto_type, veto_name.as_mut_ptr(), MAX_PATH, 0) };
    let name = from_wide(&veto_name);
    match request_verdict(configret, veto_type) {
        RequestVerdict::Ejected => RequestAnswer::Ejected,
        RequestVerdict::Vetoed => RequestAnswer::Vetoed {
            detail: format!("CONFIGRET {configret}, veto type {veto_type} by {name}"),
        },
        RequestVerdict::NotPossible => not_possible(format!("CONFIGRET {configret}, veto type {veto_type} {name}")),
    }
}

fn device_number(handle: &OwnedHandle) -> Result<STORAGE_DEVICE_NUMBER, u32> {
    let mut number = STORAGE_DEVICE_NUMBER::default();
    // SAFETY: `IOCTL_STORAGE_GET_DEVICE_NUMBER` takes no input and fills a `STORAGE_DEVICE_NUMBER`.
    unsafe { ioctl::<(), _>(handle, IOCTL_STORAGE_GET_DEVICE_NUMBER, None, Some(&mut number)) }?;
    Ok(number)
}

/// The device node of the `class` interface whose device number is `number`.
fn devnode_of(class: &windows_sys::core::GUID, number: u32) -> Option<u32> {
    let mut len = 0u32;
    // SAFETY: `class` outlives the call; no device id filter.
    let configret = unsafe {
        CM_Get_Device_Interface_List_SizeW(&mut len, class, std::ptr::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
    };
    if configret != CR_SUCCESS || len == 0 {
        return None;
    }
    let mut list = vec![0u16; len as usize];
    // SAFETY: `list` holds the `len` units the size call asked for.
    let configret = unsafe {
        CM_Get_Device_Interface_ListW(
            class,
            std::ptr::null(),
            list.as_mut_ptr(),
            len,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if configret != CR_SUCCESS {
        return None;
    }
    list.split(|&unit| unit == 0)
        .filter(|path| !path.is_empty())
        .find_map(|path| {
            let path: Vec<u16> = path.iter().copied().chain(Some(0)).collect();
            let interface = open_volume(&path, 0).ok()?;
            let found = device_number(&interface).ok()?;
            drop(interface);
            (found.DeviceNumber == number).then(|| instance_node(&path)).flatten()
        })
}

/// The device node behind interface `path` (NUL-terminated).
fn instance_node(path: &[u16]) -> Option<u32> {
    let mut kind = 0u32;
    let mut id = [0u16; 512];
    let mut size = u32::try_from(size_of_val(&id)).ok()?;
    // SAFETY: `path` is NUL-terminated; `id` holds `size` bytes, which the call reads and
    // overwrites with what it wrote.
    let configret = unsafe {
        CM_Get_Device_Interface_PropertyW(
            path.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut kind,
            id.as_mut_ptr().cast(),
            &mut size,
            0,
        )
    };
    if configret != CR_SUCCESS {
        return None;
    }
    let mut node = 0u32;
    // SAFETY: `id` is the NUL-terminated instance id the property gave.
    let configret = unsafe { CM_Locate_DevNodeW(&mut node, id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
    (configret == CR_SUCCESS).then_some(node)
}

/// `node` or the nearest node above it Windows calls removable. Eight levels at most: a stick's
/// removable node sits one or two above its disk.
fn removable_ancestor(mut node: u32) -> Option<u32> {
    for _ in 0..8 {
        let (mut status, mut problem) = (0u32, 0u32);
        // SAFETY: `node` is a device node the configuration manager gave; both outputs are valid.
        let configret = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, node, 0) };
        if configret == CR_SUCCESS && status & DN_REMOVABLE != 0 {
            return Some(node);
        }
        let mut parent = 0u32;
        // SAFETY: as above.
        if unsafe { CM_Get_Parent(&mut parent, node, 0) } != CR_SUCCESS {
            return None;
        }
        node = parent;
    }
    None
}

// ── EjectMedia and DetachImage ────────────────────────────────────────

/// A volume Cmdr locked and dismounted: nobody holds a file there, and nothing reads it until
/// the handle closes.
struct Dismounted(OwnedHandle);

/// Opens the volume, locks it (retrying a transient holder), and dismounts it. A refused lock is
/// a holder's veto: ❌ never forced.
fn lock_and_dismount(device: &[u16], access: u32) -> Result<Dismounted, RequestAnswer> {
    let volume =
        open_volume(device, access).map_err(|code| not_possible(format!("couldn't open the volume (error {code})")))?;
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

fn detach_image(device: &[u16]) -> RequestAnswer {
    // A VHD's volume locks for writing; an ISO's is read-only and opens for reading only.
    let dismounted =
        lock_and_dismount(device, GENERIC_READ | GENERIC_WRITE).or_else(|_| lock_and_dismount(device, GENERIC_READ));
    let Dismounted(volume) = match dismounted {
        Ok(dismounted) => dismounted,
        Err(answer) => return answer,
    };
    let Some((storage_type, backing)) = backing_file(&volume) else {
        return not_possible("Windows named no image file behind the volume");
    };
    // ❗ Closed before the detach: Cmdr's own open handle would veto the disk's removal.
    drop(volume);
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
    if opened != 0 {
        return not_possible(format!("couldn't open the image file (error {opened})"));
    }
    // SAFETY: `image` is the handle `OpenVirtualDisk` just gave, owned here.
    let image = unsafe { <OwnedHandle as std::os::windows::io::FromRawHandle>::from_raw_handle(image) };
    // SAFETY: a virtual disk handle opened for detaching.
    let detached = unsafe { DetachVirtualDisk(image.as_raw_handle() as HANDLE, DETACH_VIRTUAL_DISK_FLAG_NONE, 0) };
    match detached {
        0 => RequestAnswer::Ejected,
        ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION => RequestAnswer::Vetoed {
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
