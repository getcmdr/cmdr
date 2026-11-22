//! How much of a physical disk one eject takes down, and which switcher rows share a disk.
//!
//! Finder's model: Eject takes one volume down and leaves the disk attached while others stay
//! mounted, and the last one takes the disk with it. "Eject all" is the separate request for the
//! whole disk.

use std::path::Path;

use super::EjectScope;
use super::in_flight::DiskKey;
use crate::volumes::LocationInfo;
use crate::volumes::disk_units::MountedVolume;

/// What this eject takes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiskReach {
    /// One volume, with `diskutil unmount`. The disk stays attached.
    ThisVolume,
    /// Everything on the disk, with `diskutil eject`, which powers it down.
    WholeDisk,
}

/// What an eject of the volume at `clicked` takes down, given what's mounted on its disk. Pure.
///
/// `has_a_row` says whether a mount earns a switcher row. ❗ Only those keep the disk up: a mount
/// nobody can see has no row to eject it from, so counting one would leave the person's last
/// visible volume unable to power the disk down. `diskutil eject` takes it along, and if it's held,
/// the eject refuses honestly.
pub(super) fn reach_of(
    scope: EjectScope,
    clicked: &Path,
    mounted: &[MountedVolume],
    has_a_row: impl Fn(&Path) -> bool,
) -> DiskReach {
    let has_another = mounted
        .iter()
        .any(|volume| volume.path != clicked && has_a_row(&volume.path));
    match scope {
        EjectScope::Volume if has_another => DiskReach::ThisVolume,
        EjectScope::Volume | EjectScope::Disk => DiskReach::WholeDisk,
    }
}

/// Sets `shares_disk_with_others` on every row whose disk another row also sits on. `disk_of`
/// answers `None` for a row it can't place, which then shares with nobody: its Eject still decides
/// for itself from a fresh read, so all this costs is the "Eject all" item.
///
/// ❗ Asks only when two rows could share, so the usual listing (one external drive, or none) does
/// no IOKit walk at all. A listing runs on every `volumes-changed`.
pub(crate) fn mark_shared_disks(rows: &mut [LocationInfo], disk_of: impl Fn(&str) -> Option<u64>) {
    let candidates: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.is_ejectable && matches!(row.category, crate::volumes::LocationCategory::AttachedVolume))
        .map(|(index, _)| index)
        .collect();
    if candidates.len() < 2 {
        return;
    }
    let disks: Vec<Option<u64>> = candidates.iter().map(|&index| disk_of(&rows[index].path)).collect();
    for (position, &index) in candidates.iter().enumerate() {
        let Some(disk) = disks[position] else { continue };
        rows[index].shares_disk_with_others = disks
            .iter()
            .enumerate()
            .any(|(other, other_disk)| other != position && *other_disk == Some(disk));
    }
}

/// The disk under a row's mount, for [`mark_shared_disks`].
pub(crate) fn disk_at(mount_path: &str) -> Option<u64> {
    match super::disk_target::resolve(mount_path) {
        super::disk_target::Resolution::Disk(target) => {
            let DiskKey(key) = target.key;
            Some(key)
        }
        super::disk_target::Resolution::Gone | super::disk_target::Resolution::NoDisk => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumes::{LocationCategory, LocationInfo};

    fn mounted(path: &str) -> MountedVolume {
        MountedVolume {
            bsd_name: "disk7s1".to_string(),
            whole_unit: 7,
            volume_uuid: None,
            path: crate::volumes::os_mount_path(path).to_path_buf(),
        }
    }

    const A: &str = "/Volumes/A";

    #[test]
    fn eject_takes_one_volume_while_the_disk_keeps_another() {
        let disk = [mounted(A), mounted("/Volumes/B")];
        assert_eq!(
            reach_of(EjectScope::Volume, Path::new(A), &disk, |_| true),
            DiskReach::ThisVolume
        );
    }

    #[test]
    fn eject_of_the_last_volume_takes_the_disk() {
        assert_eq!(
            reach_of(EjectScope::Volume, Path::new(A), &[mounted(A)], |_| true),
            DiskReach::WholeDisk
        );
    }

    #[test]
    fn eject_all_takes_the_disk_however_many_volumes_are_on_it() {
        let disk = [mounted(A), mounted("/Volumes/B")];
        assert_eq!(
            reach_of(EjectScope::Disk, Path::new(A), &disk, |_| true),
            DiskReach::WholeDisk
        );
        assert_eq!(
            reach_of(EjectScope::Disk, Path::new(A), &[mounted(A)], |_| true),
            DiskReach::WholeDisk
        );
    }

    #[test]
    fn a_mount_with_no_row_doesnt_keep_the_disk_up() {
        // The person's last visible volume has to be able to power the disk down: nothing else in
        // the app can reach a hidden mount.
        let disk = [mounted(A), mounted("/Volumes/.hidden")];
        assert_eq!(
            reach_of(EjectScope::Volume, Path::new(A), &disk, |path| path == Path::new(A)),
            DiskReach::WholeDisk
        );
    }

    fn row(path: &str, is_ejectable: bool) -> LocationInfo {
        LocationInfo {
            is_ejectable,
            path: path.to_string(),
            category: LocationCategory::AttachedVolume,
            ..crate::volumes::location_for_test()
        }
    }

    #[test]
    fn only_rows_on_one_disk_share_it() {
        let mut rows = [
            row("/Volumes/A", true),
            row("/Volumes/B", true),
            row("/Volumes/Elsewhere", true),
            row("/Volumes/Unplaced", true),
            row("/Volumes/Fixed", false),
        ];
        mark_shared_disks(&mut rows, |path| match path {
            "/Volumes/A" | "/Volumes/B" | "/Volumes/Fixed" => Some(1),
            "/Volumes/Elsewhere" => Some(2),
            _ => None,
        });
        let shares: Vec<bool> = rows.iter().map(|row| row.shares_disk_with_others).collect();
        assert_eq!(shares, [true, true, false, false, false]);
    }

    #[test]
    fn a_lone_ejectable_row_asks_nothing() {
        let mut rows = [row("/Volumes/A", true), row("/Volumes/Fixed", false)];
        mark_shared_disks(&mut rows, |_| panic!("one candidate can't share a disk"));
        assert!(!rows[0].shares_disk_with_others);
    }
}
