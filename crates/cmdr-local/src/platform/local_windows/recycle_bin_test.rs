//! The one Recycle Bin answer (#1332): the volume over a folder answers `has_trash` from it, a
//! share has none, and both spellings of a path read the same drive.

use std::path::Path;

use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{MountClass, Volume};

use super::drive_bin;
use crate::LocalWindowsVolume;
use crate::local_windows_conformance_test::volume_at;

fn has_recycle_bin(path: &Path) -> bool {
    drive_bin(path).is_some()
}

/// The runner's temp folder is on a fixed disk with its bin on (the Recycle Bin arm's cells
/// recycle there), so its volume has a trash, and says so from the same answer.
#[test]
fn a_volume_on_a_fixed_drive_has_the_drives_recycle_bin() {
    let dir = TestDir::new("win_recycle_bin_fixed");

    assert!(has_recycle_bin(&dir), "a fixed disk with its bin on keeps one");
    assert!(volume_at(&dir).has_trash(), "and its volume says so");
    assert!(
        drive_bin(&dir)
            .and_then(|bin| bin.limit_bytes())
            .is_some_and(|limit| limit > 0)
    );
}

#[test]
fn both_spellings_read_the_same_drive() {
    let dir = TestDir::new("win_recycle_bin_spellings");
    let forward = dir.to_str().expect("a Unicode temp path").replace('\\', "/");

    assert_eq!(has_recycle_bin(Path::new(&forward)), has_recycle_bin(&dir));
}

/// The admin share reaches `C:` over SMB, which keeps no Recycle Bin: the Shell would delete
/// through it for good. A share's volume says no without asking the drive.
#[test]
fn a_share_has_no_recycle_bin() {
    assert!(!has_recycle_bin(Path::new(r"\\localhost\C$\Windows")));
    assert!(drive_bin(Path::new("//localhost/C$/Windows")).is_none());
    let share = LocalWindowsVolume::on_mount(
        "share",
        r"\\localhost\C$\",
        MountClass::SmbShare,
        VolumeHost::detached(),
    );
    assert!(!share.has_trash());
}
