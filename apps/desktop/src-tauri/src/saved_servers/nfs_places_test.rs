//! This Mac's groups, as the NFS identity guess offers them.

use super::*;

#[test]
fn this_macs_other_groups_leave_out_the_primary_and_group_zero_each_once() {
    assert_eq!(other_groups(20, &[20, 12, 0, 80, 12]), vec![12, 80]);
}

#[test]
#[cfg(unix)]
fn this_macs_identity_is_the_processs_own() {
    // SAFETY: `getuid` takes nothing, can't fail, and touches no memory of ours.
    let uid = unsafe { libc::getuid() };
    let identity = this_mac_identity();
    assert_eq!(identity.uid, uid);
    assert!(!identity.other_gids.contains(&identity.gid));
    assert!(!identity.other_gids.contains(&0));
}

#[test]
#[cfg(target_os = "windows")]
fn on_windows_this_machine_asks_as_nobody() {
    // Windows has no uid: `AUTH_SYS`'s anonymous identity, until the place sets its own.
    let identity = this_mac_identity();
    assert_eq!((identity.uid, identity.gid), (65_534, 65_534));
    assert!(identity.other_gids.is_empty());
}
