//! Which volumes an eject takes down together (task 39b): the device tree from a volume up to
//! what the request removes.
//!
//! - A volume names its disk with `IOCTL_STORAGE_GET_DEVICE_NUMBER` (device type and number).
//! - Every present disk and CD-ROM interface (`CM_Get_Device_Interface_ListW`) is opened once
//!   with no access rights and asked the same, which maps each number to its device node
//!   (`DEVPKEY_Device_InstanceId`, `CM_Locate_DevNodeW`).
//! - Up the tree (`CM_Get_Parent`) to the first node with `DN_REMOVABLE` (`CM_Get_DevNode_Status`):
//!   the USB mass-storage device, not the disk under it. That's what `CM_Request_Device_EjectW`
//!   removes, as the tray's "Safely remove" does (the walk Uwe Sieber's RemoveDrive documents,
//!   uwe-sieber.de/drivetools_e.html, read 2026-11-25), so every volume under it goes: each
//!   partition of a stick, each slot of a multi-slot card reader.
//!
//! A disc's eject and an image's detach take one disk, so their unit is the volumes on that disk
//! number (a VHD's partitions). Everything here opens devices: ❌ never from a listing path, only
//! from the eject's own blocking thread under its deadline.

use std::collections::HashMap;
use std::os::windows::io::OwnedHandle;

use cmdr_fs::volume::windows_eject::EjectRoute;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_DevNode_Status, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_PropertyW, CM_Get_Parent, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CR_SUCCESS, DN_REMOVABLE,
};
use windows_sys::Win32::Devices::Properties::DEVPKEY_Device_InstanceId;
use windows_sys::Win32::System::Ioctl::{
    GUID_DEVINTERFACE_CDROM, GUID_DEVINTERFACE_DISK, IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER,
};

use super::storage::{ioctl, open_volume};

/// A disk, as `IOCTL_STORAGE_GET_DEVICE_NUMBER` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DiskNumber {
    device_type: u32,
    number: u32,
}

/// What one eject request takes down: the volumes sharing a key go together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitKey {
    /// The removable device node `CM_Request_Device_EjectW` is aimed at.
    Device(u32),
    /// One disk: a disc's media, or a mounted image.
    Disk { device_type: u32, number: u32 },
}

impl UnitKey {
    /// One number per unit, for the eject's in-flight claim: a device node and a disk number
    /// can't collide, since each carries its own tag in the top bits.
    pub(crate) fn as_u64(self) -> u64 {
        match self {
            Self::Device(node) => (1 << 62) | u64::from(node),
            Self::Disk { device_type, number } => (2 << 62) | (u64::from(device_type) << 32) | u64::from(number),
        }
    }
}

/// Every present disk and CD-ROM by number, read once per eject.
pub(super) struct DeviceTree {
    nodes: HashMap<DiskNumber, u32>,
}

impl DeviceTree {
    pub(super) fn read() -> Self {
        let mut nodes = HashMap::new();
        for class in [GUID_DEVINTERFACE_DISK, GUID_DEVINTERFACE_CDROM] {
            for path in interfaces(&class) {
                let Ok(interface) = open_volume(&path, 0) else {
                    continue;
                };
                let Ok(number) = disk_number_of(&interface) else {
                    continue;
                };
                drop(interface);
                if let Some(node) = instance_node(&path) {
                    nodes.insert(number, node);
                }
            }
        }
        Self { nodes }
    }

    /// The unit the volume at `device` (a `\\?\Volume{…}`, NUL-terminated) belongs to for `route`.
    /// `None` when the volume names no disk, or (for `RemoveDevice`) nothing above it is
    /// removable.
    pub(super) fn unit_key(&self, route: EjectRoute, device: &[u16]) -> Option<UnitKey> {
        let number = {
            // ❗ Closed before anything asks Windows to eject: a handle of Cmdr's own vetoes it.
            let volume = open_volume(device, 0).ok()?;
            disk_number_of(&volume).ok()?
        };
        match route {
            EjectRoute::RemoveDevice => removable_ancestor(*self.nodes.get(&number)?).map(UnitKey::Device),
            EjectRoute::EjectMedia | EjectRoute::DetachImage => Some(UnitKey::Disk {
                device_type: number.device_type,
                number: number.number,
            }),
        }
    }
}

fn disk_number_of(handle: &OwnedHandle) -> Result<DiskNumber, u32> {
    let mut number = STORAGE_DEVICE_NUMBER::default();
    // SAFETY: `IOCTL_STORAGE_GET_DEVICE_NUMBER` takes no input and fills a `STORAGE_DEVICE_NUMBER`.
    unsafe { ioctl::<(), _>(handle, IOCTL_STORAGE_GET_DEVICE_NUMBER, None, Some(&mut number)) }?;
    Ok(DiskNumber {
        device_type: number.DeviceType,
        number: number.DeviceNumber,
    })
}

/// Every present interface of `class`, each NUL-terminated.
fn interfaces(class: &windows_sys::core::GUID) -> Vec<Vec<u16>> {
    let mut len = 0u32;
    // SAFETY: `class` outlives the call; no device id filter.
    let configret = unsafe {
        CM_Get_Device_Interface_List_SizeW(&mut len, class, std::ptr::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
    };
    if configret != CR_SUCCESS || len == 0 {
        return Vec::new();
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
        return Vec::new();
    }
    list.split(|&unit| unit == 0)
        .filter(|path| !path.is_empty())
        .map(|path| path.iter().copied().chain(Some(0)).collect())
        .collect()
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
