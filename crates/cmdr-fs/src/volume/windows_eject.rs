//! Which Windows drives Cmdr can eject, and how (task 39b). Pure, so it's tested on every OS;
//! the device reads that feed it and the calls that carry an eject out live in the app
//! (`volumes/platform/windows/storage.rs`, `file_system/volume/eject/platform/windows/`).
//!
//! The decision reads two facts: the drive table's kind (`GetDriveTypeW`) and the storage bus
//! the volume's device reports (`IOCTL_STORAGE_QUERY_PROPERTY`, `StorageDeviceProperty`,
//! `STORAGE_DEVICE_DESCRIPTOR::BusType`). Both are answered from what Windows already holds:
//! neither touches the media, so neither can wake or hang a disk.
//!
//! - **The whole device goes** ([`EjectRoute::RemoveDevice`]) for a disk on a hot-plug bus: USB,
//!   FireWire, SD, MMC, and any `DRIVE_REMOVABLE` drive. That's `CM_Request_Device_EjectW` on the
//!   disk's removable ancestor, what the tray's "Safely remove hardware" does, so every volume
//!   of the stick goes with it, as on Windows itself.
//! - **The disc comes out** ([`EjectRoute::EjectMedia`]) for an optical drive: lock, dismount,
//!   `IOCTL_STORAGE_EJECT_MEDIA`, which opens the tray and leaves the drive, as Explorer's Eject
//!   does (a USB DVD drive included: a person ejecting a disc means the disc).
//! - **The image detaches** ([`EjectRoute::DetachImage`]) for a mounted VHD, VHDX, or ISO
//!   (`BusTypeFileBackedVirtual`): lock, dismount, `DetachVirtualDisk`, the parity of a macOS
//!   disk image's eject. ❗ `CM_Request_Device_EjectW` doesn't detach one (Uwe Sieber's
//!   RemoveDrive needs a separate `-vhd` switch for it, uwe-sieber.de/drivetools_e.html, read
//!   2026-11-25).
//! - **Nothing** for the system drive, an internal disk (SATA, NVMe, SAS, RAID, Storage
//!   Spaces), a share, a `subst` drive, a RAM disk, or an alias root. ❗ An internal disk on a
//!   hot-plug SATA port also shows in the tray's list, and stays non-ejectable here: Explorer
//!   offers no Eject for one either, and a misclick there takes a working disk offline.

use super::DEFAULT_VOLUME_ID;
use super::windows_volume_ids::{WindowsDrive, WindowsDriveKind};

/// `STORAGE_BUS_TYPE` values from `winioctl.h` (Windows SDK 10.0.26100) that the decision reads.
pub mod bus {
    /// `BusType1394`.
    pub const IEEE_1394: i32 = 4;
    /// `BusTypeUsb`.
    pub const USB: i32 = 7;
    /// `BusTypeFileBackedVirtual`: a mounted VHD, VHDX, or ISO.
    pub const FILE_BACKED_VIRTUAL: i32 = 15;
    /// `BusTypeSd`.
    pub const SD: i32 = 12;
    /// `BusTypeMmc`.
    pub const MMC: i32 = 13;
}

/// What the volume's storage device says about itself, or nothing when it wouldn't answer (a
/// volume spanning disks, a device that refuses the query).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageFacts {
    /// `STORAGE_DEVICE_DESCRIPTOR::BusType`.
    pub bus_type: i32,
}

/// How an eject of a drive goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EjectRoute {
    /// `CM_Request_Device_EjectW` on the disk's removable ancestor: the whole device.
    RemoveDevice,
    /// Lock, dismount, and `IOCTL_STORAGE_EJECT_MEDIA`: the disc, the drive stays.
    EjectMedia,
    /// Lock, dismount, and `DetachVirtualDisk`: a mounted VHD, VHDX, or ISO.
    DetachImage,
}

/// How `drive` ejects, or `None` when it doesn't (module docs).
pub fn eject_route(drive: &WindowsDrive, facts: Option<StorageFacts>) -> Option<EjectRoute> {
    if drive.is_alias || drive.id == DEFAULT_VOLUME_ID {
        return None;
    }
    let bus_type = facts.map(|facts| facts.bus_type);
    if bus_type == Some(bus::FILE_BACKED_VIRTUAL)
        && matches!(drive.kind, WindowsDriveKind::Fixed | WindowsDriveKind::Optical)
    {
        return Some(EjectRoute::DetachImage);
    }
    match drive.kind {
        WindowsDriveKind::Optical => Some(EjectRoute::EjectMedia),
        WindowsDriveKind::Removable => Some(EjectRoute::RemoveDevice),
        WindowsDriveKind::Fixed => bus_type
            .filter(|bus| is_hot_plug(*bus))
            .map(|_| EjectRoute::RemoveDevice),
        WindowsDriveKind::Ram | WindowsDriveKind::Network | WindowsDriveKind::Subst => None,
    }
}

fn is_hot_plug(bus_type: i32) -> bool {
    matches!(bus_type, bus::USB | bus::IEEE_1394 | bus::SD | bus::MMC)
}

/// How Windows answered an eject request, decided from its return code and veto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestVerdict {
    /// The device or media went.
    Ejected,
    /// Something holds the drive: an open file, an app, a service. ❌ Never forced.
    Vetoed,
    /// The request couldn't be made or Windows turned it down for another reason (no rights,
    /// a device that can't be ejected).
    NotPossible,
}

/// `PNP_VETO_TYPE` values from `cfg.h` that mean someone holds the device.
pub mod veto {
    /// `PNP_VetoTypeUnknown`: no veto was given.
    pub const UNKNOWN: i32 = 0;
    /// `PNP_VetoPendingClose`.
    pub const PENDING_CLOSE: i32 = 2;
    /// `PNP_VetoWindowsApp`: an app answered the query with a refusal.
    pub const WINDOWS_APP: i32 = 3;
    /// `PNP_VetoWindowsService`.
    pub const WINDOWS_SERVICE: i32 = 4;
    /// `PNP_VetoOutstandingOpen`: a handle is open on the device (a file on the stick).
    pub const OUTSTANDING_OPEN: i32 = 5;
    /// `PNP_VetoDevice`: a driver of the device (the file system with files open) refused.
    pub const DEVICE: i32 = 6;
    /// `PNP_VetoDriver`.
    pub const DRIVER: i32 = 7;
    /// `PNP_VetoAlreadyRemoved`.
    pub const ALREADY_REMOVED: i32 = 13;
}

/// `CR_SUCCESS` and `CR_REMOVE_VETOED` from `cfgmgr32.h`.
pub mod configret {
    /// `CR_SUCCESS`.
    pub const SUCCESS: u32 = 0;
    /// `CR_REMOVE_VETOED`.
    pub const REMOVE_VETOED: u32 = 23;
}

/// What `CM_Request_Device_EjectW` answering `configret` with `veto_type` means.
///
/// ❗ The veto is read before the return code: the docs fill it "if the removal request fails",
/// and a success that still names a veto must never read as ejected (the stick would stay
/// mounted under a "safe to unplug"). A device already removed is the goal met.
pub fn request_verdict(configret: u32, veto_type: i32) -> RequestVerdict {
    match veto_type {
        veto::ALREADY_REMOVED => RequestVerdict::Ejected,
        veto::PENDING_CLOSE
        | veto::WINDOWS_APP
        | veto::WINDOWS_SERVICE
        | veto::OUTSTANDING_OPEN
        | veto::DEVICE
        | veto::DRIVER => RequestVerdict::Vetoed,
        veto::UNKNOWN if configret == configret::SUCCESS => RequestVerdict::Ejected,
        _ if configret == configret::REMOVE_VETOED => RequestVerdict::Vetoed,
        _ => RequestVerdict::NotPossible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(id: &str, kind: WindowsDriveKind) -> WindowsDrive {
        WindowsDrive {
            root: "E:/".to_string(),
            id: id.to_string(),
            kind,
            label: None,
            share_root: None,
            is_alias: false,
            fs_type: None,
            read_only: false,
        }
    }

    fn on(bus_type: i32) -> Option<StorageFacts> {
        Some(StorageFacts { bus_type })
    }

    /// `BusTypeSata` and `BusTypeNvme`, an internal disk's.
    const SATA: i32 = 11;
    const NVME: i32 = 17;

    #[test]
    fn a_stick_or_card_takes_its_whole_device_whatever_its_bus_says() {
        let stick = drive("local-stick", WindowsDriveKind::Removable);
        assert_eq!(eject_route(&stick, on(bus::USB)), Some(EjectRoute::RemoveDevice));
        assert_eq!(eject_route(&stick, on(bus::SD)), Some(EjectRoute::RemoveDevice));
        assert_eq!(
            eject_route(&stick, None),
            Some(EjectRoute::RemoveDevice),
            "a DRIVE_REMOVABLE drive is removable even when its device wouldn't say more"
        );
    }

    #[test]
    fn a_fixed_disk_ejects_only_on_a_hot_plug_bus() {
        let disk = drive("local-ssd", WindowsDriveKind::Fixed);
        for bus_type in [bus::USB, bus::IEEE_1394, bus::SD, bus::MMC] {
            assert_eq!(
                eject_route(&disk, on(bus_type)),
                Some(EjectRoute::RemoveDevice),
                "bus {bus_type}"
            );
        }
        for bus_type in [SATA, NVME] {
            assert_eq!(
                eject_route(&disk, on(bus_type)),
                None,
                "an internal disk, bus {bus_type}"
            );
        }
        assert_eq!(
            eject_route(&disk, None),
            None,
            "a fixed disk whose bus nobody could read stays put"
        );
    }

    #[test]
    fn a_disc_comes_out_and_its_drive_stays() {
        let dvd = drive("local-dvd", WindowsDriveKind::Optical);
        assert_eq!(eject_route(&dvd, on(SATA)), Some(EjectRoute::EjectMedia));
        assert_eq!(
            eject_route(&dvd, on(bus::USB)),
            Some(EjectRoute::EjectMedia),
            "a USB DVD drive ejects its disc, not itself"
        );
        assert_eq!(eject_route(&dvd, None), Some(EjectRoute::EjectMedia));
    }

    #[test]
    fn a_mounted_vhd_or_iso_detaches() {
        assert_eq!(
            eject_route(
                &drive("local-vhd", WindowsDriveKind::Fixed),
                on(bus::FILE_BACKED_VIRTUAL)
            ),
            Some(EjectRoute::DetachImage)
        );
        assert_eq!(
            eject_route(
                &drive("local-iso", WindowsDriveKind::Optical),
                on(bus::FILE_BACKED_VIRTUAL)
            ),
            Some(EjectRoute::DetachImage)
        );
    }

    #[test]
    fn the_system_drive_an_alias_and_every_non_disk_never_eject() {
        assert_eq!(
            eject_route(&drive(DEFAULT_VOLUME_ID, WindowsDriveKind::Fixed), on(bus::USB)),
            None,
            "the system drive, even booted from USB"
        );
        let mut alias = drive("local-stick", WindowsDriveKind::Removable);
        alias.is_alias = true;
        assert_eq!(eject_route(&alias, on(bus::USB)), None);
        for kind in [
            WindowsDriveKind::Ram,
            WindowsDriveKind::Network,
            WindowsDriveKind::Subst,
        ] {
            assert_eq!(eject_route(&drive("other", kind), on(bus::USB)), None, "{kind:?}");
        }
    }

    #[test]
    fn a_veto_is_a_refusal_even_when_the_call_reports_success() {
        assert_eq!(
            request_verdict(configret::SUCCESS, veto::UNKNOWN),
            RequestVerdict::Ejected
        );
        for veto_type in [
            veto::OUTSTANDING_OPEN,
            veto::WINDOWS_APP,
            veto::WINDOWS_SERVICE,
            veto::DEVICE,
            veto::DRIVER,
            veto::PENDING_CLOSE,
        ] {
            assert_eq!(
                request_verdict(configret::REMOVE_VETOED, veto_type),
                RequestVerdict::Vetoed,
                "veto {veto_type}"
            );
            assert_eq!(
                request_verdict(configret::SUCCESS, veto_type),
                RequestVerdict::Vetoed,
                "veto {veto_type} with a success code"
            );
        }
        assert_eq!(
            request_verdict(configret::REMOVE_VETOED, veto::UNKNOWN),
            RequestVerdict::Vetoed
        );
        assert_eq!(
            request_verdict(configret::REMOVE_VETOED, veto::ALREADY_REMOVED),
            RequestVerdict::Ejected,
            "a device already gone is the goal met"
        );
        // `PNP_VetoInsufficientRights` and `CR_ACCESS_DENIED`: no rights, nobody holds it.
        assert_eq!(request_verdict(51, 12), RequestVerdict::NotPossible);
    }
}
