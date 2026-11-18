//! Windows: no volume discovery yet. Lists the system drive as the root and three home folders;
//! everything that reads a mount table answers "nothing here", and the watcher has nothing to
//! watch. The local volume is `LocalWindowsVolume`.

use std::path::Path;
use std::sync::Arc;

use crate::volumes::{DEFAULT_VOLUME_ID, LocationCategory, LocationInfo, NetworkMount, SmbMountInfo, VolumeInfo};
use cmdr_fs::volume::path_grammar::{AppPath, PathFlavor};
use cmdr_fs::volume::{MountClass, SpaceInfo, Volume};
use cmdr_local::LocalWindowsVolume;

/// What the synthetic `network` volume reports as its `fs_type`: the SMB name the other arms
/// report, since this one mounts no shares.
pub(crate) const NETWORK_FS_TYPE: &str = "smbfs";

/// NTFS, ReFS, FAT, and exFAT fold case (a per-directory case-sensitive NTFS flag exists, off by
/// default).
pub(crate) const LOCAL_DISK_CASE_SENSITIVE: bool = false;

/// The home folders that exist, then the root, then home itself.
pub(crate) fn list_locations() -> Vec<LocationInfo> {
    let home = dirs::home_dir().unwrap_or_default();
    let favorites = [
        (home.join("Desktop"), "Desktop", "fav-desktop"),
        (home.join("Documents"), "Documents", "fav-documents"),
        (home.join("Downloads"), "Downloads", "fav-downloads"),
    ];
    let mut locations: Vec<LocationInfo> = favorites
        .into_iter()
        .filter(|(path, _, _)| path.exists())
        .map(|(path, name, id)| row(id, name, &path, LocationCategory::Favorite))
        .collect();
    locations.push(row(
        DEFAULT_VOLUME_ID,
        "Root",
        Path::new(&system_drive_root()),
        LocationCategory::MainVolume,
    ));
    locations.push(row("home", "Home", &home, LocationCategory::Favorite));
    locations
}

/// ❗ A stub: the system drive alone, under the root volume's id, so the default pane opens on a
/// real disk. Every other drive and UNC share waits on the multi-root task's open decision
/// (which drives register, and their volume ids; `docs/specs/windows-port-plan.md`).
pub(crate) fn root_volume() -> Arc<dyn Volume> {
    Arc::new(LocalWindowsVolume::local_folder(
        "Root",
        system_drive_root(),
        crate::volume_host::host(),
    ))
}

pub(crate) fn local_volume_on_mount(name: &str, root: &Path, mount: MountClass) -> Arc<dyn Volume> {
    Arc::new(LocalWindowsVolume::on_mount(
        name,
        root,
        mount,
        crate::volume_host::host(),
    ))
}

pub(crate) fn is_local_volume(volume: &dyn Volume) -> bool {
    volume.as_any().is::<LocalWindowsVolume>()
}

/// The system drive's root as an app path (`C:/`, plan D1): `%SystemDrive%`, which Windows sets
/// for every process, uppercased, else `C:/`.
fn system_drive_root() -> String {
    system_drive_root_from(std::env::var("SystemDrive").ok().as_deref())
}

fn system_drive_root_from(system_drive: Option<&str>) -> String {
    match system_drive.map(str::as_bytes) {
        Some([letter, b':']) if letter.is_ascii_alphabetic() => format!("{}:/", letter.to_ascii_uppercase() as char),
        _ => "C:/".to_string(),
    }
}

fn row(id: &str, name: &str, path: &Path, category: LocationCategory) -> LocationInfo {
    LocationInfo {
        id: id.to_string(),
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        category,
        icon: None,
        is_ejectable: false,
        fs_type: None,
        supports_trash: true,
        mount_is_read_only: false,
        is_disk_image: false,
        shares_disk_with_others: false,
        is_cloud_mount: false,
        case_sensitive: LOCAL_DISK_CASE_SENSITIVE,
        connection_state: None,
        pinned: None,
        landing_path: None,
        device_readiness: None,
        usb_speed: None,
        capabilities: None,
        favorite_shortcut: None,
        favorite_target: None,
        root_label: None,
        mount_account: None,
    }
}

/// The root volume's row for a path on the system drive (either separator, long-path prefix or
/// not), else `None`. ❗ A stub like [`root_volume`]: another drive or a UNC share answers "no
/// volume" until the multi-root task registers them.
pub(crate) fn resolve_path_volume_fast(path: &str) -> Option<VolumeInfo> {
    resolve_on_system_drive(path, &system_drive_root())
}

fn resolve_on_system_drive(path: &str, system_drive: &str) -> Option<VolumeInfo> {
    let parsed = AppPath::from_native(path, PathFlavor::Windows)?;
    let drive = AppPath::parse(system_drive, PathFlavor::Windows)?;
    if parsed.root() != drive.root() {
        return None;
    }
    Some(row(
        DEFAULT_VOLUME_ID,
        "Root",
        Path::new(system_drive),
        LocationCategory::MainVolume,
    ))
}

pub(crate) fn network_mount_under(_path: &Path) -> Option<NetworkMount> {
    None
}

/// Nothing to copy: the listing publishes no row a backend serves.
pub(crate) fn enrich_from_volume_registry(_volumes: &mut [LocationInfo]) {}

pub(crate) fn get_smb_mount_info(_mount_path: &str) -> Option<SmbMountInfo> {
    None
}

pub(crate) fn smb_mount_info_at(_mount_point: &str) -> Option<SmbMountInfo> {
    None
}

pub(crate) fn smb_mounts() -> Option<Vec<(String, SmbMountInfo)>> {
    Some(Vec::new())
}

pub(crate) fn mount_rows() -> Option<Vec<(String, String, String)>> {
    Some(Vec::new())
}

pub(crate) fn registrable_mount_roots() -> Option<Vec<String>> {
    Some(Vec::new())
}

pub(crate) fn get_mount_point(_path: &str) -> Option<(String, String, String)> {
    None
}

pub(crate) fn volume_id_for_mount(_mount_path: &str) -> String {
    DEFAULT_VOLUME_ID.to_string()
}

/// ❗ A marked seam, not an answer: each drive and UNC share is its own volume
/// (`AppPath::parse(path, PathFlavor::Windows)` names its root), but their volume ids aren't
/// decided yet (`docs/specs/windows-port-plan.md`, the drives task), so every path still answers
/// the root volume, as the stub did.
pub(crate) fn local_volume_id_for_path(_path: &str) -> String {
    DEFAULT_VOLUME_ID.to_string()
}

pub(crate) fn mount_identity_at(_path: &str) -> Option<u64> {
    None
}

pub(crate) fn has_mount_identity(_identity: u64) -> Option<bool> {
    None
}

pub(crate) fn mount_type_and_source_for(_path: &Path) -> Option<(String, String)> {
    None
}

pub(crate) fn live_volume_space(_path: &str) -> Option<SpaceInfo> {
    None
}

/// No mount watching yet.
pub(crate) mod watcher {
    use tauri::AppHandle;

    pub(crate) fn start_volume_watcher(_app: &AppHandle) {}

    pub(crate) fn stop_volume_watcher() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_is_an_uppercase_drive_root() {
        assert_eq!(system_drive_root_from(Some("C:")), "C:/");
        assert_eq!(system_drive_root_from(Some("d:")), "D:/");
    }

    #[test]
    fn anything_else_falls_back_to_c() {
        assert_eq!(system_drive_root_from(None), "C:/");
        assert_eq!(system_drive_root_from(Some("")), "C:/");
        assert_eq!(system_drive_root_from(Some("C:\\")), "C:/");
        assert_eq!(system_drive_root_from(Some("1:")), "C:/");
    }

    #[test]
    fn a_path_on_the_system_drive_resolves_to_the_root_volume() {
        for path in [
            "C:/",
            "C:/Users",
            r"C:\Users\test_user_1",
            r"\\?\C:\Windows",
            "c:/users",
        ] {
            let volume = resolve_on_system_drive(path, "C:/").unwrap_or_else(|| panic!("{path} resolves"));
            assert_eq!(volume.id, DEFAULT_VOLUME_ID, "{path}");
            assert_eq!(volume.path, "C:/", "{path}");
            assert_eq!(volume.category, LocationCategory::MainVolume, "{path}");
        }
    }

    #[test]
    fn another_drive_a_share_or_a_relative_path_resolves_to_nothing_yet() {
        for path in [
            "D:/src",
            "//testnas.local/share/x",
            r"\\testnas.local\share",
            "Users",
            "/Users",
            "smb://x/y",
        ] {
            assert!(resolve_on_system_drive(path, "C:/").is_none(), "{path}");
        }
    }

    #[test]
    fn the_root_volume_lists_the_system_drive() {
        let root = root_volume();
        assert_eq!(root.root(), Path::new(&system_drive_root()));
        assert!(is_local_volume(root.as_ref()));
    }
}
