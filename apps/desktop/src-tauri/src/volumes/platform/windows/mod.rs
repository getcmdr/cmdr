//! Windows: every drive letter, folder-only mount, and mapped share is its own volume (plan D1),
//! keyed by the disk's identity (plan D8): a local volume by its GUID, a share by its server and
//! share, the system drive by `root`. A local volume has ONE root (its letter, else its shortest
//! folder mount); a folder it's mounted at again is a folder of the drive holding it
//! (`volumes/DETAILS.md` § "Windows: a volume per drive"). `drives.rs` holds the Win32 reads; the
//! id ladder and the drive table's pure rules are `cmdr_fs::volume::windows_volume_ids`.
//!
//! The drive table is read at startup, on every switcher refresh (`list_locations`), when a
//! path names a root it doesn't hold, and whenever Windows says a drive came or went
//! (`watcher/`, task 39). A mapped drive's UNC spelling registers as an alias root of the same
//! volume, so `Z:/x` and `//server/share/x` land on one id.

mod drive_icons;
mod drives;
pub(crate) mod storage;
pub(crate) mod watcher;

use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::ignore_poison::IgnorePoison;
use crate::volumes::{DEFAULT_VOLUME_ID, LocationCategory, LocationInfo, NetworkMount, SmbMountInfo, VolumeInfo};
use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::path_grammar::{self, PathFlavor, PathKind, app_spelling};
use cmdr_fs::volume::windows_volume_ids::{WindowsDrive, WindowsDriveKind, covering_drive};
use cmdr_fs::volume::{MountClass, SpaceInfo, Volume, path_volume_id};
use cmdr_local::LocalVolume;

/// What the synthetic `network` volume reports as its `fs_type`, and what a mapped share's row
/// carries: the SMB name the other arms report, so a consumer classifying by fs type sees a share.
pub(crate) const NETWORK_FS_TYPE: &str = "smbfs";

/// Whether a row's `fs_type` names an SMB share: on Windows, exactly the mapped shares, which
/// carry [`NETWORK_FS_TYPE`] (a local drive reports `NTFS`, `FAT32`, `exFAT`, or `ReFS`).
pub(crate) fn is_smb_fs_type(fs_type: Option<&str>) -> bool {
    fs_type == Some(NETWORK_FS_TYPE)
}

/// Whether a row's `fs_type` names a network filesystem whose calls can hang: on Windows, the
/// only network filesystem the drive table names is a mapped SMB share.
pub(crate) fn is_network_fs_type(fs_type: Option<&str>) -> bool {
    is_smb_fs_type(fs_type)
}

/// NTFS, ReFS, FAT, and exFAT fold case (a per-directory case-sensitive NTFS flag exists, off by
/// default).
pub(crate) const LOCAL_DISK_CASE_SENSITIVE: bool = false;

/// How long a read of the drive table answers path questions before the next one re-reads it.
/// Navigation asks per listing; two seconds keeps a burst of asks to one sweep.
const TABLE_TTL: Duration = Duration::from_secs(2);

/// One drive table read, and when it was taken.
type TableRead = (Instant, Arc<Vec<WindowsDrive>>);

/// The last drive table read.
static TABLE: LazyLock<Mutex<Option<TableRead>>> = LazyLock::new(|| Mutex::new(None));

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

/// The drive table, read now, or `None` when the mount table wouldn't answer: the watcher's
/// read, where ❌ an empty table would read as every drive gone.
fn listed_drive_table() -> Option<Arc<Vec<WindowsDrive>>> {
    let drives = Arc::new(drives::enumerate_listed(&system_drive_root())?);
    *TABLE.lock_ignore_poison() = Some((Instant::now(), Arc::clone(&drives)));
    Some(drives)
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
    let app = path_grammar::native_to_app(path, PathFlavor::Windows)?;
    if let Some(drive) = covering_drive(&drive_table(), &app) {
        return Some(drive.clone());
    }
    if let Some(drive) = covering_drive(&fresh_drive_table(), &app) {
        return Some(drive.clone());
    }
    if path_grammar::path_kind(&app, PathFlavor::Windows)? == PathKind::Unc {
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
            crate::volumes::os_mount_path(&system_root),
            LocationCategory::MainVolume,
        ));
    }
    locations.extend(drives.iter().filter(|drive| !drive.is_alias).map(drive_row));
    locations.push(row("home", "Home", &home, LocationCategory::Favorite));
    // Icons only here, the switcher's list: `resolve_path_volume_fast` runs per navigation.
    let kinds = drives
        .iter()
        .filter(|drive| !drive.is_alias)
        .map(|drive| (drive.root.clone(), drive.kind))
        .collect();
    drive_icons::fill_icons(&mut locations, &kinds);
    locations
}

/// The host's local volume at the system drive, under `root`.
pub(crate) fn root_volume() -> Arc<dyn Volume> {
    let root = system_drive_root();
    let name = root.trim_end_matches('/').to_string();
    Arc::new(LocalVolume::local_folder(name, root, crate::volume_host::host()))
}

pub(crate) fn local_volume_on_mount(name: &str, root: &Path, mount: MountClass) -> Arc<dyn Volume> {
    Arc::new(LocalVolume::on_mount(name, root, mount, crate::volume_host::host()))
}

pub(crate) fn is_local_volume(volume: &dyn Volume) -> bool {
    volume.as_any().is::<LocalVolume>()
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
        // The drive's own answer, the one its volume's `has_trash` and the Recycle Bin's refusal
        // read too (`cmdr_local::recycle_bin`): a fixed disk with its bin on. Elsewhere a delete is
        // for good, so it asks. A share says no with no call to it.
        // `cmdr_fs::volume::windows_eject` decides; the eject itself asks the same question again
        // (`eject_target`), so a row can't offer an eject the eject then refuses.
        is_ejectable: storage::route_of(drive).is_some(),
        supports_trash: !is_share
            && AppPath::from_normalized(&drive.root)
                .ok()
                .and_then(|root| root.to_native_plain().ok())
                .is_some_and(|root| cmdr_local::recycle_bin::drive_bin(&root).is_some()),
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
        // A favorite, home, or the system drive's fallback row: nothing to eject.
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

/// A drive Cmdr's eject can take away: its route, its volume id, and its app root.
pub(crate) struct EjectTarget {
    pub(crate) route: cmdr_fs::volume::windows_eject::EjectRoute,
    pub(crate) volume_id: String,
    pub(crate) root: String,
}

/// What an eject of the volume at `mount_path` does, or `None` when it can't be ejected. Reads
/// the cached drive table and the cached storage facts, so it doesn't touch the media.
pub(crate) fn eject_target(mount_path: &str) -> Option<EjectTarget> {
    let drive = drive_for(mount_path)?;
    let route = storage::route_of(&drive)?;
    Some(EjectTarget {
        route,
        volume_id: drive.id,
        root: drive.root,
    })
}

pub(crate) fn network_mount_under(_path: &Path) -> Option<NetworkMount> {
    None
}

/// Copies each registered volume's capabilities and connection state onto its row, as on Linux.
/// The drives and the server and device rows (appended before this, `volume_listing::complete`)
/// are all served by a registered backend; a no-op here shipped every one with
/// `capabilities: None`, so the pane guessed from the kind.
pub(crate) fn enrich_from_volume_registry(volumes: &mut [LocationInfo]) {
    let manager = crate::file_system::volume::manager::get_volume_manager();
    for vol in volumes.iter_mut() {
        if let Some(registered) = manager.get(&vol.id) {
            vol.capabilities = Some(registered.capabilities());
            vol.connection_state = registered.connection_state();
        }
    }
}

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
    drive.remote.clone().unwrap_or_else(|| drive.root.clone())
}

/// The id of the volume at `mount_path`: its drive table row's, else for a root the table
/// doesn't list (a share nobody mapped) the ladder's answer from the path alone.
pub(crate) fn volume_id_for_mount(mount_path: &str) -> String {
    match drive_for(mount_path) {
        Some(drive) => drive.id,
        None => path_grammar::native_to_app(mount_path, PathFlavor::Windows)
            .and_then(|app| path_grammar::root_of(&app, PathFlavor::Windows))
            .map_or_else(|| DEFAULT_VOLUME_ID.to_string(), |root| path_volume_id(&root)),
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
        assert_eq!(root.root().as_str(), system_drive_root());
        assert!(is_local_volume(root.as_ref()));
    }
}
