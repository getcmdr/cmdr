//! What one Windows `WM_DEVICECHANGE` message says about Cmdr's drives (task 39). Pure, so
//! it's tested on every OS; the window that receives the message and the work it starts live
//! in the app's `volumes/platform/windows/watcher/`.
//!
//! The message's `wParam` names the event; for the events this reads, `lParam` points at a
//! `DEV_BROADCAST_HDR` whose `dbch_size` covers the whole structure, so the caller hands over
//! exactly those bytes and nothing here dereferences a pointer. Also the drive table's diff
//! ([`diff_drives`]), which every notification ends in.
//!
//! - **`DBT_DEVTYP_VOLUME`** (`DEV_BROADCAST_VOLUME`), broadcast to every top-level window:
//!   a drive letter arrived or left. `dbcv_unitmask` is one bit per letter (bit 0 is `A:`),
//!   `DBTF_MEDIA` marks media in a drive that stays (a card reader, an optical drive), and
//!   `DBTF_NET` a network volume.
//! - **`DBT_DEVTYP_HANDLE`** (`DEV_BROADCAST_HANDLE`), sent only to whoever registered a handle
//!   on the device: the query, refusal, and completion of a removal, plus `DBT_CUSTOMEVENT`s
//!   such as a volume lock (`chkdsk`, format, BitLocker). `dbch_hdevnotify` says which
//!   registration it is.
//!
//! Layouts from `dbt.h` (Windows SDK 10.0.26100); the app's Windows tests pin the offsets
//! against `windows-sys`' own structs.

use super::DEFAULT_VOLUME_ID;
use super::windows_volume_ids::{WindowsDrive, WindowsDriveKind};

/// `DBT_DEVICEARRIVAL`: a device or media arrived and is ready.
pub const DBT_DEVICEARRIVAL: usize = 0x8000;
/// `DBT_DEVICEQUERYREMOVE`: something asks to remove the device; a handle holder lets go.
pub const DBT_DEVICEQUERYREMOVE: usize = 0x8001;
/// `DBT_DEVICEQUERYREMOVEFAILED`: that removal was refused, so the device stays.
pub const DBT_DEVICEQUERYREMOVEFAILED: usize = 0x8002;
/// `DBT_DEVICEREMOVEPENDING`: the device is about to go, no asking.
pub const DBT_DEVICEREMOVEPENDING: usize = 0x8003;
/// `DBT_DEVICEREMOVECOMPLETE`: the device or media is gone (a safe removal or a pull).
pub const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;
/// `DBT_CUSTOMEVENT`: a driver-defined event, named by a GUID.
pub const DBT_CUSTOMEVENT: usize = 0x8006;

/// `DBT_DEVTYP_VOLUME`.
pub const DBT_DEVTYP_VOLUME: u32 = 2;
/// `DBT_DEVTYP_HANDLE`.
pub const DBT_DEVTYP_HANDLE: u32 = 6;

/// `DBTF_MEDIA`: the change is media in a drive, not the drive itself.
pub const DBTF_MEDIA: u16 = 0x0001;
/// `DBTF_NET`: a network volume.
pub const DBTF_NET: u16 = 0x0002;

/// `GUID_IO_VOLUME_LOCK` (`ioevent.h`): someone is locking the volume (format, `chkdsk /f`).
pub const GUID_IO_VOLUME_LOCK: u128 = 0x5070_8874_c9af_11d1_8fef_00a0_c9a0_6d32;
/// `GUID_IO_VOLUME_LOCK_FAILED`: the lock didn't take.
pub const GUID_IO_VOLUME_LOCK_FAILED: u128 = 0xae2e_ed10_0ba8_11d2_8ffb_00a0_c9a0_6d32;
/// `GUID_IO_VOLUME_UNLOCK`: the lock ended.
pub const GUID_IO_VOLUME_UNLOCK: u128 = 0x9a8c_3d68_d0cb_11d1_8fef_00a0_c9a0_6d32;

/// What happened to the drive letters a volume message names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeEvent {
    /// They arrived (`DBT_DEVICEARRIVAL`).
    Arrived,
    /// They left (`DBT_DEVICEREMOVECOMPLETE`).
    Removed,
}

/// Why a handle registration is asked to let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetGoCause {
    /// A removal (`DBT_DEVICEQUERYREMOVE`).
    Removal,
    /// A volume lock (`GUID_IO_VOLUME_LOCK`): a format, `chkdsk /f`, or anything else that may
    /// rewrite the volume before it unlocks.
    Lock,
}

/// What a handle registration hears about its device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleEvent {
    /// Let go: a removal is being asked for (`DBT_DEVICEQUERYREMOVE`), or a lock
    /// (`GUID_IO_VOLUME_LOCK`). Both wait for every open handle to close.
    LetGo(LetGoCause),
    /// Take it back: the removal was refused, the lock failed, or the lock ended.
    TakeBack,
    /// The device is going or gone (`DBT_DEVICEREMOVEPENDING`, `DBT_DEVICEREMOVECOMPLETE`):
    /// close what's left and unregister.
    Gone,
}

/// One `WM_DEVICECHANGE`, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChange {
    /// Drive letters arrived or left.
    Volumes {
        /// Which way.
        event: VolumeEvent,
        /// The letters, uppercase, `A` first.
        letters: Vec<char>,
        /// Media in a drive that stays (`DBTF_MEDIA`).
        media: bool,
        /// A network volume (`DBTF_NET`).
        network: bool,
    },
    /// A handle registration's device.
    Handle {
        /// What it heard.
        event: HandleEvent,
        /// The registration (`dbch_hdevnotify`), as an integer.
        registration: usize,
    },
    /// Anything else: another device type, an event this doesn't act on, or a structure too
    /// short for what its header claims.
    Other,
}

/// The letters a `dbcv_unitmask` names, `A` first.
pub fn drive_letters(unitmask: u32) -> Vec<char> {
    (0..26u8)
        .filter(|bit| unitmask & (1 << bit) != 0)
        .map(|bit| char::from(b'A' + bit))
        .collect()
}

/// Where `DEV_BROADCAST_HANDLE`'s pointer-sized fields and its GUID sit: after the 12-byte
/// header, aligned to a pointer.
const fn handle_offsets() -> (usize, usize, usize) {
    let pointer = size_of::<usize>();
    let handle = 12usize.next_multiple_of(pointer);
    let registration = handle + pointer;
    let event_guid = registration + pointer;
    (handle, registration, event_guid)
}

/// `dbch_hdevnotify`'s offset in `DEV_BROADCAST_HANDLE`.
pub const HANDLE_REGISTRATION_OFFSET: usize = handle_offsets().1;
/// `dbch_eventguid`'s offset in `DEV_BROADCAST_HANDLE`.
pub const HANDLE_EVENT_GUID_OFFSET: usize = handle_offsets().2;
/// `dbcv_unitmask`'s offset in `DEV_BROADCAST_VOLUME`.
pub const VOLUME_UNITMASK_OFFSET: usize = 12;
/// `dbcv_flags`'s offset in `DEV_BROADCAST_VOLUME`.
pub const VOLUME_FLAGS_OFFSET: usize = 16;

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn usize_at(bytes: &[u8], at: usize) -> Option<usize> {
    let width = size_of::<usize>();
    Some(usize::from_ne_bytes(bytes.get(at..at + width)?.try_into().ok()?))
}

/// A `GUID` as `windows-sys`' `GUID::from_u128` builds it: `data1` to `data3` in native
/// order, `data4` as bytes.
fn guid_at(bytes: &[u8], at: usize) -> Option<u128> {
    let data1 = u32_at(bytes, at)?;
    let data2 = u16_at(bytes, at + 4)?;
    let data3 = u16_at(bytes, at + 6)?;
    let data4: [u8; 8] = bytes.get(at + 8..at + 16)?.try_into().ok()?;
    Some(
        (u128::from(data1) << 96)
            | (u128::from(data2) << 80)
            | (u128::from(data3) << 64)
            | u128::from(u64::from_be_bytes(data4)),
    )
}

/// The structure `wm_devicechange`'s `lParam` points at, as `dbch_size` bytes. `None` for an
/// event that carries none (a `DBT_DEVNODES_CHANGED`) is read as [`DeviceChange::Other`].
pub fn decode(event: usize, broadcast: Option<&[u8]>) -> DeviceChange {
    let Some(bytes) = broadcast else {
        return DeviceChange::Other;
    };
    let Some(device_type) = u32_at(bytes, 4) else {
        return DeviceChange::Other;
    };
    match device_type {
        DBT_DEVTYP_VOLUME => decode_volume(event, bytes).unwrap_or(DeviceChange::Other),
        DBT_DEVTYP_HANDLE => decode_handle(event, bytes).unwrap_or(DeviceChange::Other),
        _ => DeviceChange::Other,
    }
}

fn decode_volume(event: usize, bytes: &[u8]) -> Option<DeviceChange> {
    let event = match event {
        DBT_DEVICEARRIVAL => VolumeEvent::Arrived,
        DBT_DEVICEREMOVECOMPLETE => VolumeEvent::Removed,
        _ => return None,
    };
    let unitmask = u32_at(bytes, VOLUME_UNITMASK_OFFSET)?;
    let flags = u16_at(bytes, VOLUME_FLAGS_OFFSET)?;
    Some(DeviceChange::Volumes {
        event,
        letters: drive_letters(unitmask),
        media: flags & DBTF_MEDIA != 0,
        network: flags & DBTF_NET != 0,
    })
}

fn decode_handle(event: usize, bytes: &[u8]) -> Option<DeviceChange> {
    let registration = usize_at(bytes, HANDLE_REGISTRATION_OFFSET)?;
    let event = match event {
        DBT_DEVICEQUERYREMOVE => HandleEvent::LetGo(LetGoCause::Removal),
        DBT_DEVICEQUERYREMOVEFAILED => HandleEvent::TakeBack,
        DBT_DEVICEREMOVEPENDING | DBT_DEVICEREMOVECOMPLETE => HandleEvent::Gone,
        DBT_CUSTOMEVENT => match guid_at(bytes, HANDLE_EVENT_GUID_OFFSET)? {
            GUID_IO_VOLUME_LOCK => HandleEvent::LetGo(LetGoCause::Lock),
            GUID_IO_VOLUME_LOCK_FAILED | GUID_IO_VOLUME_UNLOCK => HandleEvent::TakeBack,
            _ => return None,
        },
        _ => return None,
    };
    Some(DeviceChange::Handle { event, registration })
}

/// What a fresh read of the drive table changed against the last one.
// DEFAULT-OK: empty is "nothing changed", the truthful answer for two equal reads.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DriveTableChanges {
    /// Roots that left, or whose volume changed under the same root (another stick on the
    /// letter), as the last read had them.
    pub left: Vec<WindowsDrive>,
    /// Roots that arrived, or a new volume on a root that was there.
    pub arrived: Vec<WindowsDrive>,
}

impl DriveTableChanges {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.arrived.is_empty()
    }
}

/// The roots that left and arrived between two reads, each keyed by `(root, id)`: the same
/// letter on another disk is the old one leaving and the new one arriving. A label or
/// read-only flag that moved is no change. The system drive (`root`) never leaves, so it's
/// in neither list.
pub fn diff_drives(known: &[WindowsDrive], now: &[WindowsDrive]) -> DriveTableChanges {
    let same = |a: &WindowsDrive, b: &WindowsDrive| a.root == b.root && a.id == b.id;
    let counted = |drive: &&WindowsDrive| drive.id != DEFAULT_VOLUME_ID;
    DriveTableChanges {
        left: known
            .iter()
            .filter(counted)
            .filter(|was| !now.iter().any(|is| same(was, is)))
            .cloned()
            .collect(),
        arrived: now
            .iter()
            .filter(counted)
            .filter(|is| !known.iter().any(|was| same(was, is)))
            .cloned()
            .collect(),
    }
}

/// The roots a re-read found gone, sorted by what each one leaving means.
// DEFAULT-OK: empty is "nothing left", the truthful answer for no leaving roots.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Leaving {
    /// Roots that really left: announce each one's leave.
    pub gone: Vec<WindowsDrive>,
    /// A whole-share mapping's UNC alias whose mapped letter went while the share still answers:
    /// no leave, since a pane on `//server/share/…` is still on a live share. Its root is adopted
    /// again as the share's own volume (the same id, `unc_drive`'s), so the letter's leave
    /// promotes it rather than taking the volume down (#1492).
    pub kept_shares: Vec<WindowsDrive>,
}

/// Sorts `left` ([`diff_drives`]): an alias root that `still_answers` stays as its share, the
/// rest are gone. Only an alias is asked, so a dead server costs no probe of a letter.
pub fn sort_leaving(left: &[WindowsDrive], still_answers: impl Fn(&WindowsDrive) -> bool) -> Leaving {
    let (kept_shares, gone) = left
        .iter()
        .cloned()
        .partition(|drive| drive.is_alias && still_answers(drive));
    Leaving { gone, kept_shares }
}

/// Whether Cmdr keeps a handle on `drive` to hear its removal query (`DBT_DEVTYP_HANDLE`): a
/// local disk, stick, card, or disc that isn't the system drive. A share, a `subst` folder,
/// and a RAM disk have no device to remove, and an alias root is its drive's second address.
pub fn hears_removal_queries(drive: &WindowsDrive) -> bool {
    matches!(
        drive.kind,
        WindowsDriveKind::Fixed | WindowsDriveKind::Removable | WindowsDriveKind::Optical
    ) && !drive.is_alias
        && drive.id != DEFAULT_VOLUME_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(root: &str, id: &str, kind: WindowsDriveKind) -> WindowsDrive {
        WindowsDrive {
            root: root.to_string(),
            id: id.to_string(),
            kind,
            label: None,
            share_root: None,
            remote: None,
            is_alias: false,
            fs_type: None,
            read_only: false,
        }
    }

    #[test]
    fn a_stick_plugged_in_arrives_and_pulled_leaves() {
        let system = drive("C:/", DEFAULT_VOLUME_ID, WindowsDriveKind::Fixed);
        let stick = drive("E:/", "local-stick", WindowsDriveKind::Removable);
        let before = vec![system.clone()];
        let after = vec![system.clone(), stick.clone()];
        assert_eq!(
            diff_drives(&before, &after),
            DriveTableChanges {
                left: vec![],
                arrived: vec![stick.clone()],
            }
        );
        assert_eq!(
            diff_drives(&after, &before),
            DriveTableChanges {
                left: vec![stick],
                arrived: vec![],
            }
        );
        assert!(diff_drives(&after, &after).is_empty());
    }

    #[test]
    fn another_disk_on_the_same_letter_is_one_leaving_and_one_arriving() {
        let first = drive("E:/", "local-first", WindowsDriveKind::Removable);
        let second = drive("E:/", "local-second", WindowsDriveKind::Removable);
        let changes = diff_drives(std::slice::from_ref(&first), std::slice::from_ref(&second));
        assert_eq!(changes.left, [first]);
        assert_eq!(changes.arrived, [second]);
    }

    #[test]
    fn a_relabeled_drive_or_the_system_drive_is_no_change() {
        let before = drive("D:/", "local-data", WindowsDriveKind::Fixed);
        let relabeled = WindowsDrive {
            label: Some("Photos".to_string()),
            read_only: true,
            ..before.clone()
        };
        assert!(diff_drives(&[before], &[relabeled]).is_empty());
        let system = drive("C:/", DEFAULT_VOLUME_ID, WindowsDriveKind::Fixed);
        assert!(diff_drives(&[system], &[]).is_empty(), "the system drive never leaves");
    }

    #[test]
    fn a_mapped_drive_leaves_with_its_unc_alias() {
        let mapped = WindowsDrive {
            share_root: Some("//testnas.local/media/".to_string()),
            ..drive("Z:/", "smb-media", WindowsDriveKind::Network)
        };
        let alias = WindowsDrive {
            root: "//testnas.local/media/".to_string(),
            is_alias: true,
            share_root: None,
            ..mapped.clone()
        };
        let changes = diff_drives(&[alias.clone(), mapped.clone()], &[]);
        assert_eq!(changes.left, [alias, mapped]);
    }

    /// #1492: unmapping a whole-share letter keeps the share's UNC root while the share answers,
    /// so a pane on it stays; a share that doesn't answer leaves with its letter.
    #[test]
    fn a_whole_share_letter_leaving_keeps_its_share_while_the_share_answers() {
        let mapped = crate::volume::windows_volume_ids::mapped_drive("Z:/", Some(r"\\testnas.local\media"), "C:/");
        let alias = WindowsDrive {
            root: "//testnas.local/media/".to_string(),
            is_alias: true,
            share_root: None,
            ..mapped.clone()
        };
        let left = diff_drives(&[alias.clone(), mapped.clone()], &[]).left;

        let answering = sort_leaving(&left, |_| true);
        assert_eq!(answering.gone, std::slice::from_ref(&mapped));
        assert_eq!(answering.kept_shares, std::slice::from_ref(&alias));

        let dead = sort_leaving(&left, |_| false);
        assert_eq!(dead.gone, [alias, mapped.clone()]);
        assert!(dead.kept_shares.is_empty());

        let asked = std::cell::Cell::new(0);
        let letter_only = sort_leaving(std::slice::from_ref(&mapped), |_| {
            asked.set(asked.get() + 1);
            true
        });
        assert_eq!(letter_only.gone, [mapped]);
        assert_eq!(asked.get(), 0, "a letter is never probed");
    }

    /// #1481: a drive mapped to a folder below a share arrives as its own volume at its letter,
    /// and a pane on the share's root (the share's own volume) is no change.
    #[test]
    fn a_mapped_folder_arrives_beside_its_share_without_touching_it() {
        let share = crate::volume::windows_volume_ids::mapped_drive("Z:/", Some(r"\\testnas.local\media"), "C:/");
        let folder =
            crate::volume::windows_volume_ids::mapped_drive("W:/", Some(r"\\testnas.local\media\photos"), "C:/");
        assert_ne!(folder.id, share.id);
        let changes = diff_drives(std::slice::from_ref(&share), &[share.clone(), folder.clone()]);
        assert_eq!(
            changes.arrived,
            std::slice::from_ref(&folder),
            "the folder alone arrives, at W:/"
        );
        assert!(changes.left.is_empty());
        let changes = diff_drives(&[share.clone(), folder], std::slice::from_ref(&share));
        assert_eq!(changes.left.len(), 1);
        assert_eq!(changes.left[0].root, "W:/");
    }

    #[test]
    fn only_a_local_device_other_than_the_system_drive_hears_removal_queries() {
        assert!(hears_removal_queries(&drive("E:/", "s", WindowsDriveKind::Removable)));
        assert!(hears_removal_queries(&drive("D:/", "s", WindowsDriveKind::Fixed)));
        assert!(hears_removal_queries(&drive("F:/", "s", WindowsDriveKind::Optical)));
        assert!(!hears_removal_queries(&drive(
            "C:/",
            DEFAULT_VOLUME_ID,
            WindowsDriveKind::Fixed
        )));
        for kind in [
            WindowsDriveKind::Network,
            WindowsDriveKind::Subst,
            WindowsDriveKind::Ram,
        ] {
            assert!(!hears_removal_queries(&drive("X:/", "s", kind)), "{kind:?}");
        }
        let alias = WindowsDrive {
            is_alias: true,
            ..drive("//testnas.local/x/", "s", WindowsDriveKind::Fixed)
        };
        assert!(!hears_removal_queries(&alias));
    }

    fn volume(unitmask: u32, flags: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&20u32.to_ne_bytes());
        bytes.extend_from_slice(&DBT_DEVTYP_VOLUME.to_ne_bytes());
        bytes.extend_from_slice(&0u32.to_ne_bytes());
        bytes.extend_from_slice(&unitmask.to_ne_bytes());
        bytes.extend_from_slice(&flags.to_ne_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    /// A `DEV_BROADCAST_HANDLE` laid out as `dbt.h` does, with `guid` in its event slot.
    fn handle(registration: usize, guid: u128) -> Vec<u8> {
        let mut bytes = vec![0u8; HANDLE_EVENT_GUID_OFFSET + 16 + 8];
        let size = u32::try_from(bytes.len()).expect("a test structure is a few dozen bytes");
        bytes[0..4].copy_from_slice(&size.to_ne_bytes());
        bytes[4..8].copy_from_slice(&DBT_DEVTYP_HANDLE.to_ne_bytes());
        let width = size_of::<usize>();
        bytes[HANDLE_REGISTRATION_OFFSET..HANDLE_REGISTRATION_OFFSET + width]
            .copy_from_slice(&registration.to_ne_bytes());
        let at = HANDLE_EVENT_GUID_OFFSET;
        bytes[at..at + 4].copy_from_slice(&((guid >> 96) as u32).to_ne_bytes());
        bytes[at + 4..at + 6].copy_from_slice(&((guid >> 80) as u16).to_ne_bytes());
        bytes[at + 6..at + 8].copy_from_slice(&((guid >> 64) as u16).to_ne_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&(guid as u64).to_be_bytes());
        bytes
    }

    #[test]
    fn a_unit_mask_names_its_letters() {
        assert_eq!(drive_letters(0), Vec::<char>::new());
        assert_eq!(drive_letters(1), ['A']);
        assert_eq!(drive_letters(1 << 4), ['E']);
        assert_eq!(drive_letters((1 << 3) | (1 << 25)), ['D', 'Z']);
        assert_eq!(drive_letters(u32::MAX).len(), 26, "the six bits past Z name nothing");
    }

    #[test]
    fn a_stick_arriving_and_leaving_reads_as_its_letter() {
        assert_eq!(
            decode(DBT_DEVICEARRIVAL, Some(&volume(1 << 4, 0))),
            DeviceChange::Volumes {
                event: VolumeEvent::Arrived,
                letters: vec!['E'],
                media: false,
                network: false,
            }
        );
        assert_eq!(
            decode(DBT_DEVICEREMOVECOMPLETE, Some(&volume(1 << 4, DBTF_MEDIA | DBTF_NET))),
            DeviceChange::Volumes {
                event: VolumeEvent::Removed,
                letters: vec!['E'],
                media: true,
                network: true,
            }
        );
    }

    #[test]
    fn a_volume_query_or_a_short_structure_is_nothing_to_act_on() {
        assert_eq!(decode(DBT_DEVICEQUERYREMOVE, Some(&volume(1, 0))), DeviceChange::Other);
        assert_eq!(
            decode(DBT_DEVICEARRIVAL, Some(&volume(1, 0)[..14])),
            DeviceChange::Other
        );
        assert_eq!(decode(DBT_DEVICEARRIVAL, Some(&[0, 0])), DeviceChange::Other);
        assert_eq!(decode(DBT_DEVICEARRIVAL, None), DeviceChange::Other);
    }

    #[test]
    fn a_handle_hears_let_go_take_back_and_gone() {
        let at = |event| decode(event, Some(&handle(0xbeef, 0)));
        let read = |event| DeviceChange::Handle {
            event,
            registration: 0xbeef,
        };
        assert_eq!(at(DBT_DEVICEQUERYREMOVE), read(HandleEvent::LetGo(LetGoCause::Removal)));
        assert_eq!(at(DBT_DEVICEQUERYREMOVEFAILED), read(HandleEvent::TakeBack));
        assert_eq!(at(DBT_DEVICEREMOVEPENDING), read(HandleEvent::Gone));
        assert_eq!(at(DBT_DEVICEREMOVECOMPLETE), read(HandleEvent::Gone));
        assert_eq!(at(DBT_DEVICEARRIVAL), DeviceChange::Other);
    }

    #[test]
    fn a_volume_lock_lets_go_and_its_end_takes_back() {
        let custom = |guid| decode(DBT_CUSTOMEVENT, Some(&handle(7, guid)));
        let read = |event| DeviceChange::Handle { event, registration: 7 };
        assert_eq!(custom(GUID_IO_VOLUME_LOCK), read(HandleEvent::LetGo(LetGoCause::Lock)));
        assert_eq!(custom(GUID_IO_VOLUME_LOCK_FAILED), read(HandleEvent::TakeBack));
        assert_eq!(custom(GUID_IO_VOLUME_UNLOCK), read(HandleEvent::TakeBack));
        assert_eq!(custom(0x1234), DeviceChange::Other, "another driver's event");
    }
}
