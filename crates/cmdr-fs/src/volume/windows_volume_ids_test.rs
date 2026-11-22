use super::*;
use crate::volume::VolumeScheme;

/// The GUID paths, `subst` target, and share Windows 10 22H2 reported on the builder
/// (`GetVolumeNameForVolumeMountPointW`, `QueryDosDeviceW`, `WNetGetConnectionW`, 2026-11-18).
const C_GUID: &str = r"\\?\Volume{7ee27290-b54a-45d8-bcfd-9f3bd4318a54}\";
const D_GUID: &str = r"\\?\Volume{caa3226d-b12e-447b-acbe-74d20a3f2a83}\";

fn local(guid_path: &str) -> WindowsVolumeIdentity {
    WindowsVolumeIdentity::Local {
        guid_path: guid_path.to_string(),
    }
}

fn share(unc: &str) -> WindowsVolumeIdentity {
    WindowsVolumeIdentity::Share { unc: unc.to_string() }
}

#[test]
fn the_system_drive_is_root_whatever_its_guid() {
    assert_eq!(windows_volume_id("C:/", &local(C_GUID), "C:/"), DEFAULT_VOLUME_ID);
    assert_eq!(
        windows_volume_id("c:/", &WindowsVolumeIdentity::Unnamed, "C:/"),
        DEFAULT_VOLUME_ID
    );
}

#[test]
fn a_local_drive_keys_on_its_guid_so_another_letter_keeps_its_id() {
    let on_d = windows_volume_id("D:/", &local(D_GUID), "C:/");
    let on_f = windows_volume_id("F:/", &local(D_GUID), "C:/");
    assert_eq!(on_d, on_f, "the same disk on another letter is the same volume");
    assert_eq!(VolumeScheme::of(&on_d), VolumeScheme::Local);
    assert_eq!(
        on_d,
        local_volume_id(Some("caa3226d-b12e-447b-acbe-74d20a3f2a83"), "D:/")
    );
    // The GUID's case doesn't split one disk into two ids.
    assert_eq!(on_d, windows_volume_id("D:/", &local(&D_GUID.to_uppercase()), "C:/"));
}

#[test]
fn two_disks_that_swap_letters_keep_their_own_ids() {
    let d_then = windows_volume_id("D:/", &local(D_GUID), "C:/");
    let e_then = windows_volume_id("E:/", &local(C_GUID), "C:/");
    let d_now = windows_volume_id("D:/", &local(C_GUID), "C:/");
    let e_now = windows_volume_id("E:/", &local(D_GUID), "C:/");
    assert_eq!(d_then, e_now);
    assert_eq!(e_then, d_now);
}

#[test]
fn a_folder_mount_keys_on_its_guid_too() {
    let mounted = windows_volume_id("C:/mnt/data", &local(D_GUID), "C:/");
    assert_eq!(mounted, windows_volume_id("D:/", &local(D_GUID), "C:/"));
}

#[test]
fn a_mapped_drive_its_unc_root_and_the_direct_share_are_one_volume() {
    let mapped = windows_volume_id("Z:/", &share(r"\\testnas.local\Media"), "C:/");
    let unc = windows_volume_id("//testnas.local/media/", &share("//testnas.local/media/"), "C:/");
    assert_eq!(mapped, unc);
    assert_eq!(mapped, smb_volume_id("testnas.local", 445, "media"));
    assert_eq!(VolumeScheme::of(&mapped), VolumeScheme::Smb);
}

#[test]
fn a_subst_drive_or_an_unreadable_guid_keys_on_its_path() {
    assert_eq!(
        windows_volume_id("S:/", &WindowsVolumeIdentity::Unnamed, "C:/"),
        path_volume_id("S:/")
    );
    assert_eq!(
        windows_volume_id("S:/", &local("not a guid path"), "C:/"),
        path_volume_id("S:/")
    );
    assert_eq!(windows_volume_id("S:/", &share("nas"), "C:/"), path_volume_id("S:/"));
}

#[test]
fn a_volume_guid_comes_out_of_its_path() {
    assert_eq!(volume_guid(C_GUID), Some("7ee27290-b54a-45d8-bcfd-9f3bd4318a54"));
    assert_eq!(volume_guid(r"\\?\Volume{short}\"), None);
    assert_eq!(volume_guid(r"\Device\HarddiskVolume4"), None);
}

#[test]
fn a_unc_spelling_names_its_server_and_share() {
    for unc in [
        r"\\testnas.local\media",
        r"\\testnas.local\media\",
        r"\\testnas.local\media\x\y",
        "//testnas.local/media/",
        r"\\?\UNC\testnas.local\media\x",
    ] {
        assert_eq!(
            unc_server_share(unc),
            Some(("testnas.local".to_string(), "media".to_string())),
            "{unc}"
        );
    }
    for not_a_share in [r"\\testnas.local", r"\\?\C:\x", r"\\.\PhysicalDrive0", "C:/x", "//"] {
        assert_eq!(unc_server_share(not_a_share), None, "{not_a_share}");
    }
    assert_eq!(
        unc_app_root(r"\\testnas.local\media"),
        Some("//testnas.local/media/".to_string())
    );
}

#[test]
fn a_subst_target_reads_as_an_app_path() {
    assert_eq!(subst_target(r"\??\C:\Windows"), Some("C:/Windows".to_string()));
    assert_eq!(subst_target(r"\??\d:\"), Some("D:/".to_string()));
    assert_eq!(subst_target(r"\Device\HarddiskVolume4"), None);
    assert_eq!(
        subst_target(r"\Device\LanmanRedirector\;N:000000000aa5e24f\localhost\C$"),
        None
    );
}

fn drive(root: &str, kind: WindowsDriveKind, label: Option<&str>) -> WindowsDrive {
    WindowsDrive {
        root: root.to_string(),
        id: format!("id-{root}"),
        kind,
        label: label.map(str::to_string),
        share_root: None,
        is_alias: false,
        fs_type: None,
        read_only: false,
    }
}

#[test]
fn a_drive_reads_like_explorers_this_pc() {
    assert_eq!(
        drive("D:/", WindowsDriveKind::Fixed, Some("build")).display_name(),
        "build (D:)"
    );
    assert_eq!(drive("C:/", WindowsDriveKind::Fixed, None).display_name(), "C:");
    assert_eq!(drive("E:/", WindowsDriveKind::Optical, Some("  ")).display_name(), "E:");
    let mapped = WindowsDrive {
        share_root: Some("//testnas.local/media/".to_string()),
        ..drive("Z:/", WindowsDriveKind::Network, Some("media"))
    };
    assert_eq!(mapped.display_name(), r"media (\\testnas.local) (Z:)");
    assert_eq!(
        drive("//testnas.local/media/", WindowsDriveKind::Network, Some("media")).display_name(),
        r"\\testnas.local\media"
    );
    assert_eq!(
        drive("C:/mnt/data", WindowsDriveKind::Fixed, None).display_name(),
        "data"
    );
    assert_eq!(
        drive("C:/mnt/data", WindowsDriveKind::Fixed, Some("Photos")).display_name(),
        "Photos"
    );
}

#[test]
fn a_path_lands_on_the_longest_root_that_holds_it() {
    let drives = [
        drive("C:/", WindowsDriveKind::Fixed, None),
        drive("C:/mnt/data", WindowsDriveKind::Fixed, None),
        drive("D:/", WindowsDriveKind::Fixed, None),
        drive("//testnas.local/media/", WindowsDriveKind::Network, None),
    ];
    let root_of = |path: &str| covering_drive(&drives, path).map(|d| d.root.as_str());
    assert_eq!(root_of("C:/"), Some("C:/"));
    assert_eq!(root_of("C:/Windows"), Some("C:/"));
    assert_eq!(root_of("C:/mnt/data/x"), Some("C:/mnt/data"));
    assert_eq!(root_of("C:/MNT/Data"), Some("C:/mnt/data"), "folds case like NTFS");
    assert_eq!(root_of("C:/mnt/database"), Some("C:/"), "whole components only");
    assert_eq!(root_of("D:/src"), Some("D:/"));
    assert_eq!(root_of("//TestNAS.local/Media/x"), Some("//testnas.local/media/"));
    assert_eq!(root_of("F:/x"), None);
    assert_eq!(root_of("//testnas.local/other/x"), None);
}

fn mount<'a>(root: &'a str, guid_path: &'a str) -> VolumeMountPath<'a> {
    VolumeMountPath { root, guid_path }
}

/// #1385: `D:` mounted a second time at a folder of `C:` is ONE volume at ONE root, its
/// letter, the way Explorer's This PC lists it; the folder stays a folder of `C:`.
#[test]
fn a_volume_on_a_letter_and_a_folder_has_its_letter_as_its_one_root() {
    let upper = D_GUID.to_uppercase();
    let mounts = [
        mount("C:/Users/test_user_1/dmount", &upper),
        mount("C:/", C_GUID),
        mount("D:/", D_GUID),
    ];
    assert!(
        !is_volume_root(&mounts[0], &mounts),
        "the folder is no second root of D:"
    );
    assert!(is_volume_root(&mounts[1], &mounts));
    assert!(is_volume_root(&mounts[2], &mounts));
}

/// A volume mounted only in folders keeps one of them as its root: the shortest, then the
/// first by name, whatever order the sweep listed them in.
#[test]
fn a_volume_mounted_only_in_folders_has_its_shortest_folder_as_its_root() {
    let mounts = [
        mount("C:/mnt/data/deeper", D_GUID),
        mount("C:/mnt/b", D_GUID),
        mount("C:/mnt/a", D_GUID),
        mount("C:/", C_GUID),
    ];
    let roots: Vec<&str> = mounts
        .iter()
        .filter(|m| is_volume_root(m, &mounts))
        .map(|m| m.root)
        .collect();
    assert_eq!(roots, ["C:/mnt/a", "C:/"]);
}

/// A folder mount of the system drive's own volume is a folder of `C:`, never a second
/// volume beside `root`.
#[test]
fn a_folder_mount_of_the_system_volume_is_no_root_of_its_own() {
    let mounts = [mount("C:/", C_GUID), mount("C:/loop", C_GUID)];
    assert!(!is_volume_root(&mounts[1], &mounts));
}
