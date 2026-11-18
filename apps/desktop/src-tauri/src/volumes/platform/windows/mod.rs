//! Windows: every drive letter, folder mount, and mapped share is its own volume (plan D1),
//! keyed by the disk's identity (plan D7): a local volume by its GUID, a share by its server and
//! share, the system drive by `root`. `drives.rs` holds the Win32 reads; the id ladder and the
//! drive table's pure rules are `cmdr_fs::volume::windows_volume_ids`.
//!
//! The drive table is read at startup, on every switcher refresh (`list_locations`), and again
//! when a path names a root it doesn't hold; ❌ no arrival watcher yet (task 39), so a drive
//! plugged in later shows on the next refresh. A mapped drive's UNC spelling registers as an
//! alias root of the same volume, so `Z:/x` and `//server/share/x` land on one id.

mod drives;

use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::ignore_poison::IgnorePoison;
use crate::volumes::{DEFAULT_VOLUME_ID, LocationCategory, LocationInfo, NetworkMount, SmbMountInfo, VolumeInfo};
use cmdr_fs::volume::path_grammar::{AppPath, PathFlavor, PathKind, app_spelling};
use cmdr_fs::volume::windows_volume_ids::{WindowsDrive, WindowsDriveKind, covering_drive};
use cmdr_fs::volume::{MountClass, SpaceInfo, Volume, path_volume_id};
use cmdr_local::LocalWindowsVolume;

/// What the synthetic `network` volume reports as its `fs_type`, and what a mapped share's row
/// carries: the SMB name the other arms report, so a consumer classifying by fs type sees a share.
pub(crate) const NETWORK_FS_TYPE: &str = "smbfs";

/// NTFS, ReFS, FAT, and exFAT fold case (a per-directory case-sensitive NTFS flag exists, off by
/// default).
pub(crate) const LOCAL_DISK_CASE_SENSITIVE: bool = false;

/// How long a read of the drive table answers path questions before the next one re-reads it.
/// Navigation asks per listing; two seconds keeps a burst of asks to one sweep.
const TABLE_TTL: Duration = Duration::from_secs(2);

/// The last drive table read, and when.
static TABLE: LazyLock<Mutex<Option<(Instant, Arc<Vec<WindowsDrive>>)>>> = LazyLock::new(|| Mutex::new(None));

/// The drive table, re-read when older than [`TABLE_TTL`].
fn drive_table() -> Arc<Vec<WindowsDrive>> {
    let mut cached = TABLE.lock_ignore_poison();
    if let Some((read_at, drives)) = cached.as_ref()
        && read_at.elapsed() < TABLE_TTL
    {
        return Arc::clone(drives);
    }
    let drives = Arc::new(drives::enumerate(&system_drive_root()));
    *cached = Some((Instant::now(), Arc::clone(&drives)));
    drives
}

/// The drive table, read now.
fn fresh_drive_table() -> Arc<Vec<WindowsDrive>> {
    let drives = Arc::new(drives::enumerate(&system_drive_root()));
    *TABLE.lock_ignore_poison() = Some((Instant::now(), Arc::clone(&drives)));
    drives
}

/// The drive holding `path` (an app or native path), re-reading the table once when the cached
/// one holds no root for it (a drive that arrived since). A bare UNC root nobody mapped answers
/// as a share of its own.
fn drive_for(path: &str) -> Option<WindowsDrive> {
    let app = AppPath::from_native(path, PathFlavor::Windows)?.to_string();
    if let Some(drive) = covering_drive(&drive_table(), &app) {
        return Some(drive.clone());
    }
    if let Some(drive) = covering_drive(&fresh_drive_table(), &app) {
        return Some(drive.clone());
    }
    if AppPath::parse(&app, PathFlavor::Windows)?.root().kind() == PathKind::Unc {
        return drives::unc_drive(&app, &system_drive_root());
    }
    None
}

/// The system drive's root as an app path (`C:/`, plan D1).
fn system_drive_root() -> String {
    cmdr_fs::mount_layout::boot_volume_root()
}

/// The home folders that exist, then one row per drive and folder mount (the system drive as
/// the main volume, a mapped drive under the network group), then home itself.
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
    let drives = fresh_drive_table();
    let system_root = system_drive_root();
    if !drives.iter().any(|drive| drive.id == DEFAULT_VOLUME_ID) {
        // Windows always lists its system drive; a sweep that came back without it (a
        // refused call) still publishes the volume the default pane is on.
        locations.push(row(
            DEFAULT_VOLUME_ID,
            &system_root,
            Path::new(&system_root),
            LocationCategory::MainVolume,
        ));
    }
    locations.extend(drives.iter().filter(|drive| !drive.is_alias).map(drive_row));
    locations.push(row("home", "Home", &home, LocationCategory::Favorite));
    locations
}

/// The host's local volume at the system drive, under `root`.
pub(crate) fn root_volume() -> Arc<dyn Volume> {
    let root = system_drive_root();
    let name = root.trim_end_matches('/').to_string();
    Arc::new(LocalWindowsVolume::local_folder(name, root, crate::volume_host::host()))
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

/// A switcher row for one drive, folder mount, or share.
fn drive_row(drive: &WindowsDrive) -> LocationInfo {
    let category = if drive.id == DEFAULT_VOLUME_ID {
        LocationCategory::MainVolume
    } else if drive.kind == WindowsDriveKind::Network {
        LocationCategory::Network
    } else {
        LocationCategory::AttachedVolume
    };
    let is_share = drive.kind == WindowsDriveKind::Network;
    LocationInfo {
        fs_type: if is_share {
            Some(NETWORK_FS_TYPE.to_string())
        } else {
            drive.fs_type.clone()
        },
        mount_is_read_only: drive.read_only,
        // No Recycle Bin yet (task 36), and a share never had one.
        supports_trash: !is_share,
        ..row(&drive.id, &drive.display_name(), Path::new(&drive.root), category)
    }
}

fn row(id: &str, name: &str, path: &Path, category: LocationCategory) -> LocationInfo {
    LocationInfo {
        id: id.to_string(),
        name: name.to_string(),
        path: app_spelling(&path.to_string_lossy(), PathFlavor::Windows),
        category,
        icon: None,
        // Eject is task 39's: nothing reads as ejectable yet.
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

/// The row of the drive, folder mount, or share holding `path` (either separator, long-path
/// prefix or not). A mapped share's UNC spelling answers its alias row: the same id, rooted at
/// the spelling the path uses, so a pane on `//server/share/x` stays consistent with its root.
pub(crate) fn resolve_path_volume_fast(path: &str) -> Option<VolumeInfo> {
    drive_for(path).map(|drive| drive_row(&drive))
}

pub(crate) fn network_mount_under(_path: &Path) -> Option<NetworkMount> {
    None
}

/// Nothing to copy: the listing publishes no row a backend serves.
pub(crate) fn enrich_from_volume_registry(_volumes: &mut [LocationInfo]) {}

/// ❗ No SMB mount info on Windows yet: a mapped drive's share is known (its id is the `smb-`
/// one), but nothing upgrades it to a direct session, so the SMB-mount readers answer nothing.
pub(crate) fn get_smb_mount_info(_mount_path: &str) -> Option<SmbMountInfo> {
    None
}

pub(crate) fn smb_mount_info_at(_mount_point: &str) -> Option<SmbMountInfo> {
    None
}

pub(crate) fn smb_mounts() -> Option<Vec<(String, SmbMountInfo)>> {
    Some(Vec::new())
}

/// Every volume root as `(mount point, source, fs type)`: a share's source is its UNC root,
/// a local volume's its own root.
pub(crate) fn mount_rows() -> Option<Vec<(String, String, String)>> {
    Some(
        drive_table()
            .iter()
            .map(|drive| (drive.root.clone(), source_of(drive), fs_type_of(drive)))
            .collect(),
    )
}

/// Every root the registry should hold: each drive and folder mount but the system drive
/// (registered first, as `root`), plus each mapped share's UNC spelling, which lands on the
/// same id and so records as an alias root of that volume.
pub(crate) fn registrable_mount_roots() -> Option<Vec<String>> {
    Some(
        fresh_drive_table()
            .iter()
            .filter(|drive| drive.id != DEFAULT_VOLUME_ID)
            .map(|drive| drive.root.clone())
            .collect(),
    )
}

/// The root holding `path`, as `(mount point, fs type, source)`.
pub(crate) fn get_mount_point(path: &str) -> Option<(String, String, String)> {
    drive_for(path).map(|drive| (drive.root.clone(), fs_type_of(&drive), source_of(&drive)))
}

fn fs_type_of(drive: &WindowsDrive) -> String {
    if drive.kind == WindowsDriveKind::Network {
        NETWORK_FS_TYPE.to_string()
    } else {
        drive.fs_type.clone().unwrap_or_default()
    }
}

fn source_of(drive: &WindowsDrive) -> String {
    drive.share_root.clone().unwrap_or_else(|| drive.root.clone())
}

/// The id of the volume at `mount_path`: its drive table row's, else for a root the table
/// doesn't list (a share nobody mapped) the ladder's answer from the path alone.
pub(crate) fn volume_id_for_mount(mount_path: &str) -> String {
    match drive_for(mount_path) {
        Some(drive) => drive.id,
        None => AppPath::from_native(mount_path, PathFlavor::Windows).map_or_else(
            || DEFAULT_VOLUME_ID.to_string(),
            |app| path_volume_id(&app.root().to_string()),
        ),
    }
}

/// The id of the volume a path lies on, from the drive table (cached, so no syscall in the
/// common case): `root` on the system drive, a drive's GUID id elsewhere, a share's `smb-` id
/// under either spelling.
pub(crate) fn local_volume_id_for_path(path: &str) -> String {
    volume_id_for_mount(path)
}

pub(crate) fn mount_identity_at(_path: &str) -> Option<u64> {
    None
}

pub(crate) fn has_mount_identity(_identity: u64) -> Option<bool> {
    None
}

pub(crate) fn mount_type_and_source_for(path: &Path) -> Option<(String, String)> {
    drive_for(&path.to_string_lossy()).map(|drive| (fs_type_of(&drive), source_of(&drive)))
}

pub(crate) fn live_volume_space(_path: &str) -> Option<SpaceInfo> {
    None
}

/// No mount watching yet (task 39): a drive that arrives shows on the next switcher refresh.
pub(crate) mod watcher {
    use tauri::AppHandle;

    pub(crate) fn start_volume_watcher(_app: &AppHandle) {}

    pub(crate) fn stop_volume_watcher() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_publishes_the_app_path() {
        let desktop = row(
            "fav-desktop",
            "Desktop",
            Path::new(r"C:\Users\test_user_1\Desktop"),
            LocationCategory::Favorite,
        );
        assert_eq!(desktop.path, "C:/Users/test_user_1/Desktop");
    }

    #[test]
    fn the_system_drive_resolves_to_the_root_volume() {
        let system = system_drive_root();
        for path in [
            system.clone(),
            format!("{system}Windows"),
            format!(r"\\?\{}", system.replace('/', "\\")),
        ] {
            let volume = resolve_path_volume_fast(&path).unwrap_or_else(|| panic!("no volume for {path}"));
            assert_eq!(volume.id, DEFAULT_VOLUME_ID, "{path}");
            assert_eq!(volume.path, system, "{path}");
            assert_eq!(volume.category, LocationCategory::MainVolume, "{path}");
        }
        assert_eq!(local_volume_id_for_path(&format!("{system}Users")), DEFAULT_VOLUME_ID);
    }

    #[test]
    fn a_relative_or_posix_path_resolves_to_nothing() {
        for path in ["Users", "/Users", "smb://x/y"] {
            assert!(resolve_path_volume_fast(path).is_none(), "{path}");
        }
    }

    #[test]
    fn every_listed_drive_resolves_to_its_own_row() {
        for location in list_locations()
            .into_iter()
            .filter(|l| l.category != LocationCategory::Favorite)
        {
            let resolved = resolve_path_volume_fast(&location.path).expect("a listed drive resolves");
            assert_eq!(resolved.id, location.id, "{}", location.path);
            assert_eq!(resolved.path, location.path);
        }
    }

    #[test]
    fn an_unmapped_share_is_an_smb_volume_of_its_own() {
        let volume = resolve_path_volume_fast("//testnas.local/media/x").expect("a share resolves");
        assert_eq!(volume.id, cmdr_fs::volume::smb_volume_id("testnas.local", 445, "media"));
        assert_eq!(volume.path, "//testnas.local/media/");
        assert_eq!(volume.category, LocationCategory::Network);
    }

    #[test]
    fn the_root_volume_lists_the_system_drive() {
        let root = root_volume();
        assert_eq!(root.root(), Path::new(&system_drive_root()));
        assert!(is_local_volume(root.as_ref()));
    }
}
