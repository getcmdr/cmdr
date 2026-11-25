//! What a local drive's storage device says about itself, and which route its eject takes
//! (task 39b; the decision is `cmdr_fs::volume::windows_eject`, pure and tested on every OS).
//!
//! The one read is `IOCTL_STORAGE_QUERY_PROPERTY` (`StorageDeviceProperty`) on the volume's
//! device (`\\?\Volume{…}`), opened with no access rights: the storage class driver answers from
//! the descriptor it read at arrival, so no media I/O happens and a sleeping disk stays asleep
//! (learn.microsoft.com `IOCTL_STORAGE_QUERY_PROPERTY` and "Opening a volume", read
//! 2026-11-25). A volume spanning disks, or a device that refuses the query, answers nothing,
//! which the decision reads as "not on a hot-plug bus". ❗ Each volume is probed ONCE, off-thread
//! (the drive watcher's baseline and arrivals, [`probe_soon`]), and both outcomes are cached by
//! volume id: the bus doesn't change while it's mounted, and a device that refuses (Storage
//! Spaces, a half-dead stick) or stalls its open must never cost a listing path anything. The
//! listing paths read the cache alone ([`route_of`]); only the eject probes inline ([`route_now`]).

use std::collections::{HashMap, HashSet};
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

/// Each local volume's probe, by volume id: its storage facts, or `None` when the device wouldn't
/// say. ❗ Both outcomes are kept, so a device that refuses the query (Storage Spaces, a
/// half-dead stick) is asked once, ❌ never on every row read.
static FACTS: LazyLock<Mutex<HashMap<String, Option<StorageFacts>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Volume ids with a probe running, so a burst of row reads starts one.
static PROBING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Whether Cmdr reads `drive`'s storage device at all: a local disk that isn't the system drive.
fn is_probed_kind(drive: &WindowsDrive) -> bool {
    matches!(
        drive.kind,
        WindowsDriveKind::Fixed | WindowsDriveKind::Removable | WindowsDriveKind::Optical
    ) && !drive.is_alias
        && drive.id != crate::volumes::DEFAULT_VOLUME_ID
}

/// How `drive` ejects, or `None` when it doesn't, from the cached probe only: it runs on listing
/// paths (`drive_row`, `resolve_path_volume_fast`), so ❌ it never opens a device. A drive not
/// probed yet starts its probe off-thread and answers from its kind alone meanwhile (a stick
/// already offers Eject; a USB disk does once the probe lands and the list refreshes).
pub(super) fn route_of(drive: &WindowsDrive) -> Option<EjectRoute> {
    if !is_probed_kind(drive) {
        return eject_route(drive, None);
    }
    let cached = FACTS.lock_ignore_poison().get(&drive.id).copied();
    match cached {
        Some(facts) => eject_route(drive, facts),
        None => {
            // A listing path has nobody to tell: the watcher's own probes refresh the list.
            probe_soon(std::slice::from_ref(drive), || {});
            eject_route(drive, None)
        }
    }
}

/// [`route_of`], probing first when nothing's cached. Blocking: only for the eject's own
/// blocking thread, under its deadline.
pub(super) fn route_now(drive: &WindowsDrive) -> Option<EjectRoute> {
    if !is_probed_kind(drive) {
        return eject_route(drive, None);
    }
    let cached = FACTS.lock_ignore_poison().get(&drive.id).copied();
    let facts = match cached {
        Some(facts) => facts,
        None => probe(drive),
    };
    eject_route(drive, facts)
}

/// Probes each of `drives` not cached or already being probed, each on a thread of its own, so a
/// wedged device stalls nobody. The drive watcher calls it for its baseline and each arrival.
/// `changed` runs after a probe whose answer changes the drive's route from its kind's alone (a
/// USB disk turning ejectable), so the caller can refresh the list.
pub(crate) fn probe_soon(drives: &[WindowsDrive], changed: impl Fn() + Clone + Send + 'static) {
    for drive in drives.iter().filter(|drive| is_probed_kind(drive)) {
        if FACTS.lock_ignore_poison().contains_key(&drive.id) || !PROBING.lock_ignore_poison().insert(drive.id.clone())
        {
            continue;
        }
        let drive = drive.clone();
        let changed = changed.clone();
        let spawned = std::thread::Builder::new()
            .name("drive-probe".to_owned())
            .spawn(move || {
                let facts = probe(&drive);
                PROBING.lock_ignore_poison().remove(&drive.id);
                if eject_route(&drive, facts) != eject_route(&drive, None) {
                    changed();
                }
            });
        if let Err(e) = spawned {
            log::debug!(target: "volumes::windows", "Couldn't start a drive probe: {e}");
        }
    }
}

/// Reads `drive`'s storage facts and caches the outcome, whichever it is.
fn probe(drive: &WindowsDrive) -> Option<StorageFacts> {
    let facts = volume_device(&drive.root).and_then(|device| read_facts(&device));
    FACTS.lock_ignore_poison().insert(drive.id.clone(), facts);
    facts
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
