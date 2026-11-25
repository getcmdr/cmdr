//! What a local drive's storage device says about itself, and which route its eject takes
//! (task 39b; the decision is `cmdr_fs::volume::windows_eject`, pure and tested on every OS).
//!
//! The one read is `IOCTL_STORAGE_QUERY_PROPERTY` (`StorageDeviceProperty`) on the volume's
//! device (`\\?\Volume{…}`), opened with no access rights: the storage class driver answers from
//! the descriptor it read at arrival, so no media I/O happens and a sleeping disk stays asleep
//! (learn.microsoft.com `IOCTL_STORAGE_QUERY_PROPERTY` and "Opening a volume", read
//! 2026-11-25). A volume spanning disks, or a device that refuses the query, answers nothing,
//! which the decision reads as "not on a hot-plug bus". Answers are cached by volume id, since
//! the bus a volume sits on doesn't change while it's mounted.

use std::collections::HashMap;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::{LazyLock, Mutex};

use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::windows_eject::{EjectRoute, StorageFacts, eject_route};
use cmdr_fs::volume::windows_volume_ids::{WindowsDrive, WindowsDriveKind};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    StorageDeviceProperty,
};

use crate::ignore_poison::IgnorePoison;

/// Each local volume's storage facts, by volume id. Only answers are kept: a read that failed is
/// asked again next time (a stick's device can still be settling just after it arrives).
static FACTS: LazyLock<Mutex<HashMap<String, StorageFacts>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// How `drive` ejects, or `None` when it doesn't. Reads the device only for a local disk that
/// isn't the system drive.
pub(super) fn route_of(drive: &WindowsDrive) -> Option<EjectRoute> {
    let local = matches!(
        drive.kind,
        WindowsDriveKind::Fixed | WindowsDriveKind::Removable | WindowsDriveKind::Optical
    ) && !drive.is_alias
        && drive.id != crate::volumes::DEFAULT_VOLUME_ID;
    if !local {
        return eject_route(drive, None);
    }
    eject_route(drive, facts_of(drive))
}

fn facts_of(drive: &WindowsDrive) -> Option<StorageFacts> {
    if let Some(facts) = FACTS.lock_ignore_poison().get(&drive.id) {
        return Some(*facts);
    }
    let facts = read_facts(&volume_device(&drive.root)?)?;
    FACTS.lock_ignore_poison().insert(drive.id.clone(), facts);
    Some(facts)
}

/// The volume device behind an app root (`E:/`, a folder mount's `C:/mnt/data`), as a
/// NUL-terminated `\\?\Volume{…}` with no trailing separator, the spelling that opens the
/// volume itself rather than its root folder.
pub(crate) fn volume_device(root: &str) -> Option<Vec<u16>> {
    let native = AppPath::from_normalized(root).ok()?.to_native_plain().ok()?;
    let mut native = native.to_string_lossy().into_owned();
    if !native.ends_with('\\') {
        native.push('\\');
    }
    let guid_path = super::drives::volume_guid_path(&super::drives::wide(&native))?;
    Some(super::drives::wide(guid_path.trim_end_matches('\\')))
}

/// Opens the volume `device` names with `access`, sharing everything. `None` (with the Win32
/// error code) when Windows refuses.
pub(crate) fn open_volume(device: &[u16], access: u32) -> Result<OwnedHandle, u32> {
    // SAFETY: `device` is NUL-terminated and outlives the call; no security attributes, no
    // template.
    let handle = unsafe {
        CreateFileW(
            device.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        return Err(unsafe { windows_sys::Win32::Foundation::GetLastError() });
    }
    // SAFETY: a valid handle `CreateFileW` just opened, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Sends `code` to `handle` with `input`, filling `output`. The Win32 error on failure.
///
/// # Safety
///
/// `I` and `O` must be the input and output structures `code` documents (or `()` for none).
pub(crate) unsafe fn ioctl<I, O>(
    handle: &OwnedHandle,
    code: u32,
    input: Option<&I>,
    output: Option<&mut O>,
) -> Result<u32, u32> {
    use std::os::windows::io::AsRawHandle;
    let (in_ptr, in_len) = input.map_or((std::ptr::null(), 0), |input| {
        ((input as *const I).cast(), size_of::<I>() as u32)
    });
    let (out_ptr, out_len) = output.map_or((std::ptr::null_mut(), 0), |output| {
        ((output as *mut O).cast(), size_of::<O>() as u32)
    });
    let mut returned = 0u32;
    // SAFETY: the caller pairs `code` with its documented structures; both buffers are valid for
    // their lengths for the call, which is synchronous (no OVERLAPPED).
    let ok = unsafe {
        DeviceIoControl(
            handle.as_raw_handle() as HANDLE,
            code,
            in_ptr,
            in_len,
            out_ptr,
            out_len,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        return Err(unsafe { windows_sys::Win32::Foundation::GetLastError() });
    }
    Ok(returned)
}

/// The storage device descriptor's fixed part, with room for the strings that follow it so the
/// driver fills the header whole.
#[repr(C)]
struct DescriptorBuffer {
    descriptor: STORAGE_DEVICE_DESCRIPTOR,
    _strings: [u8; 512],
}

fn read_facts(device: &[u16]) -> Option<StorageFacts> {
    let handle = open_volume(device, 0).ok()?;
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    let mut buffer = DescriptorBuffer {
        descriptor: STORAGE_DEVICE_DESCRIPTOR::default(),
        _strings: [0; 512],
    };
    // SAFETY: `IOCTL_STORAGE_QUERY_PROPERTY` takes a `STORAGE_PROPERTY_QUERY` and fills a
    // `STORAGE_DEVICE_DESCRIPTOR` followed by its strings, which `DescriptorBuffer` lays out.
    let returned = unsafe { ioctl(&handle, IOCTL_STORAGE_QUERY_PROPERTY, Some(&query), Some(&mut buffer)) }.ok()?;
    // The bus type sits past the header's first fields; a short answer didn't reach it.
    let reached = std::mem::offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType) + size_of::<i32>();
    (returned as usize >= reached).then_some(StorageFacts {
        bus_type: buffer.descriptor.BusType,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_reads_its_bus_and_never_ejects() {
        let system = super::super::drive_table()
            .iter()
            .find(|drive| drive.id == crate::volumes::DEFAULT_VOLUME_ID)
            .cloned()
            .expect("Windows lists its system drive");
        let device = volume_device(&system.root).expect("the system drive has a volume device");
        assert!(
            read_facts(&device).is_some(),
            "a volume opened with no access rights answers the storage query"
        );
        assert_eq!(route_of(&system), None, "whatever bus it's on");
    }
}
