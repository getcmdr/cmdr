//! The mount-root set's registration-lifecycle cells: rank, promotion, removal, and stale marks.

#![cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, `libc`, or `std::os::unix` (windows-port plan).

use super::super::get_volume_manager;
use super::*;
use crate::file_system::volume::{InMemoryVolume, VolumeError};
use crate::ignore_poison::IgnorePoison;
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_local::LocalPosixVolume;
use std::pin::Pin;

/// A local volume for a registry cell, which never reaches the panes.
fn local_volume(name: &str, root: &str) -> Arc<LocalPosixVolume> {
    Arc::new(LocalPosixVolume::local_folder(name, root, VolumeHost::detached()))
}

fn registration_over(roots: &[&str]) -> Registration {
    let mut reg = Registration::new(local_volume("share", roots[0]));
    for root in &roots[1..] {
        reg.record_root(AppPath::from_normalized(root).expect("a canonical test root"));
    }
    reg
}

#[test]
fn among_equally_live_roots_the_shortest_path_wins() {
    // The original fix, now a tie-break: macOS suffixes the later mount, so
    // the shortest root is the one saved paths and favorites refer to.
    let reg = registration_over(&["/Volumes/testshare-1", "/Volumes/testshare"]);
    assert_eq!(
        reg.best_root().expect("two roots").path,
        *app_path!("/Volumes/testshare")
    );
}

#[test]
fn equal_length_roots_break_ties_lexicographically() {
    // Pure and order-independent, so discovery order can't decide identity.
    let reg = registration_over(&["/Volumes/bbb", "/Volumes/aaa"]);
    assert_eq!(reg.best_root().expect("two roots").path, *app_path!("/Volumes/aaa"));
}

/// The tie orders roots exactly as the `PathBuf` set did, a `..` name included:
/// `Path` sorts a `..` before every name, where comparing name TEXT puts `-a` first.
#[test]
fn the_tie_orders_roots_as_path_did() {
    let mut reg = Registration::new(local_volume("share", "/Volumes/-a"));
    // A volume's own root keeps the spelling it was built with, `..` and all.
    reg.record_root(local_volume("other", "/Volumes/..").root());
    let path_pick = [Path::new("/Volumes/-a"), Path::new("/Volumes/..")]
        .into_iter()
        .min()
        .expect("two roots");
    assert_eq!(
        Some(reg.best_root().expect("two roots").path.as_str()),
        path_pick.to_str()
    );
}

/// `same_root` answers what `Path`'s `==` answered over the roots it held.
#[test]
fn same_root_is_paths_equality() {
    let texts = [
        "/",
        "/Volumes/X",
        "/Volumes/X/",
        "/Volumes//X",
        "/Volumes/./X",
        "//Volumes/X",
        "/Volumes/x",
        "/Volumes/Data 1",
        "/Volumes/Data",
        "/Volumes/fot\u{f3}k",
        "/Volumes/foto\u{301}k",
        "/Volumes/../X",
        "smb://testnas.local/share",
        "smb://testnas.local/share/",
    ];
    for a in texts {
        for b in texts {
            // Each root as a registered volume spells it: the text it was built with.
            let (volume_a, volume_b) = (local_volume("a", a), local_volume("b", b));
            let (app_a, app_b) = (volume_a.root(), volume_b.root());
            assert_eq!((app_a.as_str(), app_b.as_str()), (a, b), "spelled as given");
            assert_eq!(same_root(app_a, app_b), Path::new(a) == Path::new(b), "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn a_proven_stale_root_loses_to_a_longer_live_one() {
    // The "worse variant": the NAS drops, macOS leaves the original mount
    // wedged, and the reconnect lands at the suffixed path. Path shape alone
    // picks the dead one every time, including across restarts.
    let mut reg = registration_over(&["/Volumes/testshare", "/Volumes/testshare-1"]);
    reg.roots[0].proven_stale = true;
    assert_eq!(
        reg.best_root().expect("two roots").path,
        *app_path!("/Volumes/testshare-1")
    );
}

#[test]
fn promotion_rebuilds_the_volume_at_the_new_root() {
    let mut reg = registration_over(&["/Volumes/testshare", "/Volumes/testshare-1"]);
    reg.roots[0].proven_stale = true;
    assert!(matches!(reg.promote_to_best_root(), Promotion::Promoted(_)));
    assert_eq!(reg.volume.root(), app_path!("/Volumes/testshare-1"));
    assert_eq!(reg.volume.name(), "share", "the display name survives a promotion");
}

#[test]
fn promotion_is_a_no_op_when_the_active_root_is_already_the_best() {
    let mut reg = registration_over(&["/Volumes/testshare", "/Volumes/testshare-1"]);
    assert!(matches!(reg.promote_to_best_root(), Promotion::AlreadyBest));
    assert_eq!(reg.volume.root(), app_path!("/Volumes/testshare"));
}

/// Every errno: the seven that say the mount is gone or wedged promote, and none
/// of the rest do. A missing file, a permission wall, or a full disk says nothing
/// about the mount, and promoting on one would rotate a healthy volume's root.
#[test]
fn stale_mount_errnos_are_told_apart_from_ordinary_file_errors() {
    let stale = [
        libc::ENOTCONN,
        libc::ETIMEDOUT,
        libc::EHOSTDOWN,
        libc::EHOSTUNREACH,
        libc::ENETDOWN,
        libc::ENETUNREACH,
        libc::ESTALE,
    ];
    for errno in 0..=4096 {
        assert_eq!(
            is_stale_mount(OsErrorClass::of_code(errno)),
            stale.contains(&errno),
            "errno {errno}"
        );
    }
}

/// A path-addressed backend that re-roots (like every real one does) and
/// RECORDS every `note_root_mount_gone` the registry sends it, so a test can
/// ask which root was told its mount is gone. The log is shared across the
/// instances a promotion creates; production keeps the answer per instance.
struct RootSpy {
    root: AppPathBuf,
    told: Arc<std::sync::Mutex<Vec<AppPathBuf>>>,
}

/// A spy rooted at `root`, plus a reader for what it has been told.
fn root_spy(root: &str) -> (Arc<RootSpy>, impl Fn() -> Vec<AppPathBuf>) {
    let told = Arc::new(std::sync::Mutex::new(Vec::new()));
    let spy = Arc::new(RootSpy {
        root: AppPath::from_normalized(root)
            .expect("a canonical test root")
            .to_owned(),
        told: Arc::clone(&told),
    });
    (spy, move || told.lock_ignore_poison().clone())
}

impl Volume for RootSpy {
    fn name(&self) -> &str {
        "spy"
    }

    fn root(&self) -> &AppPath {
        &self.root
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn rerooted(&self, new_root: &AppPath) -> Option<Arc<dyn Volume>> {
        Some(Arc::new(Self {
            root: new_root.to_owned(),
            told: Arc::clone(&self.told),
        }))
    }

    fn note_root_mount_gone(&self) {
        self.told.lock_ignore_poison().push(self.root.clone());
    }

    fn list_directory<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(crate::file_system::volume::ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_metadata<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        Box::pin(async { Err(VolumeError::NotSupported) })
    }

    fn exists<'a>(&'a self, _path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async { false })
    }

    fn is_directory<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        Box::pin(async { Err(VolumeError::NotSupported) })
    }
}

#[test]
fn a_volume_left_on_a_dead_mount_is_told_its_paths_are_gone() {
    // The share is reachable through exactly one mount and that mount dies.
    // A backend riding its own transport (a direct `SmbVolume`) keeps
    // serving, so the registration stays — but every path it hands out now
    // names a mount that isn't there, and only the registry knows.
    let manager = VolumeManager::new();
    let (spy, told) = root_spy("/Volumes/testshare");
    manager.register("share", spy);

    let outcome = manager.mark_root_stale("share", app_path!("/Volumes/testshare"));
    assert!(matches!(outcome, StaleRootOutcome::Unchanged), "nowhere to promote to");
    assert_eq!(
        told(),
        vec![app_path!("/Volumes/testshare").to_owned()],
        "the volume has to hear that its mount is gone"
    );
}

#[test]
fn a_volume_promoted_onto_a_live_mount_is_told_nothing() {
    // The promotion IS the fix here: the new root is a real mount, so the
    // volume's paths are openable again and nothing should be marked.
    let manager = doubly_mounted_share();
    let (spy, told) = root_spy("/Volumes/testshare");
    manager.register("share", spy);

    let outcome = manager.mark_root_stale("share", app_path!("/Volumes/testshare"));
    assert!(matches!(outcome, StaleRootOutcome::Promoted { .. }));
    assert!(told().is_empty(), "a live root needs no warning");
}

#[test]
fn a_promotion_onto_an_already_dead_sibling_still_tells_the_volume() {
    // Both mounts are gone: one proved it with an errno, the other with an
    // unmount event. The promotion lands on a corpse, so the honest answer
    // travels with it.
    let manager = VolumeManager::new();
    let (spy, told) = root_spy("/Volumes/testshare");
    manager.register("share", spy);
    manager.register("share", local_volume("testshare", "/Volumes/testshare-1"));

    manager.mark_root_stale("share", app_path!("/Volumes/testshare-1"));
    assert!(told().is_empty(), "the ACTIVE root still looks alive");

    manager.remove_root(Path::new("/Volumes/testshare"));
    assert_eq!(
        manager.get("share").expect("still registered").root(),
        app_path!("/Volumes/testshare-1")
    );
    assert_eq!(told(), vec![app_path!("/Volumes/testshare-1").to_owned()]);
}

/// A registry holding one share reached through two mount points, the
/// shortest one active. The shape every multi-root test starts from.
fn doubly_mounted_share() -> VolumeManager {
    let manager = VolumeManager::new();
    manager.register("share", local_volume("testshare", "/Volumes/testshare"));
    manager.register("share", local_volume("testshare", "/Volumes/testshare-1"));
    manager
}

#[test]
fn losing_the_active_mount_promotes_a_sibling_instead_of_unregistering() {
    // Ejecting one of two mounts of a share used to take the whole share
    // away until the app restarted (discovery runs at launch only), because
    // the unmount path unregistered the ID it found by root.
    let manager = doubly_mounted_share();

    let outcome = manager.remove_root(Path::new("/Volumes/testshare"));
    assert!(matches!(outcome, RootRemoval::Promoted { .. }), "a sibling survives");
    assert_eq!(
        manager.get("share").expect("still registered").root(),
        app_path!("/Volumes/testshare-1")
    );
}

#[test]
fn losing_the_last_mount_unregisters_the_volume() {
    let manager = doubly_mounted_share();
    manager.remove_root(Path::new("/Volumes/testshare"));

    let outcome = manager.remove_root(Path::new("/Volumes/testshare-1"));
    assert!(
        matches!(outcome, RootRemoval::Unregistered { .. }),
        "the last root gone means gone"
    );
    assert!(manager.get("share").is_none());
}

#[test]
fn losing_a_fallback_mount_leaves_the_active_one_alone() {
    let manager = doubly_mounted_share();

    let outcome = manager.remove_root(Path::new("/Volumes/testshare-1"));
    assert!(matches!(outcome, RootRemoval::SiblingDropped { .. }));
    assert_eq!(
        manager.get("share").expect("still registered").root(),
        app_path!("/Volumes/testshare")
    );
    assert_eq!(
        manager.known_roots("share"),
        vec![app_path!("/Volumes/testshare").to_owned()]
    );
}

#[test]
fn a_backend_that_cant_reroot_keeps_its_registration() {
    // `InMemoryVolume` takes the conservative `rerooted` default, standing in
    // for a backend whose transport is anchored to its root. Losing the
    // active mount must not unregister such a volume while another mount
    // reaches the same filesystem: its transport may not ride the mount at
    // all, so dropping it would kill something that still works.
    let manager = VolumeManager::new();
    manager.register(
        "share",
        Arc::new(InMemoryVolume::new("Direct SMB").with_root("/Volumes/testshare")),
    );
    manager.register(
        "share",
        Arc::new(InMemoryVolume::new("Second mount").with_root("/Volumes/testshare-1")),
    );

    let outcome = manager.remove_root(Path::new("/Volumes/testshare"));
    assert!(matches!(outcome, RootRemoval::ActiveRootStranded { .. }));
    assert_eq!(
        manager.get("share").expect("still registered").root(),
        app_path!("/Volumes/testshare"),
        "it stays where its transport is anchored"
    );
}

#[test]
fn a_stranded_root_stays_findable_even_though_its_mount_is_gone() {
    // The entry is still anchored to `/Volumes/testshare` (the backend declined to
    // re-root), so the set must still contain it, or `find_by_root` stops
    // recognizing the volume by the very root `volume.root()` reports and a
    // later event for that path lands nowhere.
    let manager = VolumeManager::new();
    manager.register(
        "share",
        Arc::new(InMemoryVolume::new("Direct SMB").with_root("/Volumes/testshare")),
    );
    manager.register(
        "share",
        Arc::new(InMemoryVolume::new("Second mount").with_root("/Volumes/testshare-1")),
    );

    manager.remove_root(Path::new("/Volumes/testshare"));

    assert_eq!(
        manager.find_by_root(Path::new("/Volumes/testshare")).map(|(id, _)| id),
        Some("share".to_string()),
        "the stranded active root is still one of the volume's known roots"
    );
    assert!(
        manager
            .known_roots("share")
            .contains(&app_path!("/Volumes/testshare").to_owned()),
        "and it reports as such"
    );
}

#[test]
fn a_stale_mount_errno_hands_the_id_to_a_live_sibling() {
    // Liveness outranks path shape. macOS leaves a wedged `/Volumes/testshare`
    // in place and lands the reconnect at `/Volumes/testshare-1`; picking the
    // shortest path then picks the corpse, on every launch, forever.
    let manager = doubly_mounted_share();
    assert_eq!(
        manager.get("share").expect("registered").root(),
        app_path!("/Volumes/testshare"),
        "while both look alive, the shortest path is active"
    );

    let outcome = manager.mark_root_stale("share", app_path!("/Volumes/testshare"));
    assert!(matches!(outcome, StaleRootOutcome::Promoted { .. }));
    assert_eq!(
        manager.get("share").expect("still registered").root(),
        app_path!("/Volumes/testshare-1")
    );
}

#[test]
fn a_failed_operation_promotes_only_on_an_errno_that_proves_the_mount_is_gone() {
    use crate::file_system::volume::{VolumeError, note_root_failure};

    // Drives the global registry (that's what `note_root_failure` reads), so
    // it uses ids no other test touches.
    let id = "cmdr-test-stale-errno-share";
    let manager = get_volume_manager();
    manager.unregister(id);
    manager.register(id, local_volume("testshare", "/Volumes/cmdr-test-stale"));
    manager.register(id, local_volume("testshare", "/Volumes/cmdr-test-stale-1"));

    // A missing file says nothing about the mount.
    note_root_failure(
        id,
        &VolumeError::IoError {
            message: "no such file".into(),
            raw_os_error: Some(libc::ENOENT),
        },
    );
    assert_eq!(
        manager.get(id).expect("registered").root(),
        app_path!("/Volumes/cmdr-test-stale"),
        "an ordinary file error must not rotate a healthy volume's root"
    );

    note_root_failure(
        id,
        &VolumeError::IoError {
            message: "socket is not connected".into(),
            raw_os_error: Some(libc::ENOTCONN),
        },
    );
    assert_eq!(
        manager.get(id).expect("still registered").root(),
        app_path!("/Volumes/cmdr-test-stale-1"),
        "a stale-mount errno moves the volume to the mount that still answers"
    );

    manager.unregister(id);
}

// ── Retirement through the unmount path ─────────────────────

use crate::file_system::volume::manager::test_support::RetiringVolume;

/// Losing the LAST mount really does end the volume, so its background work
/// has to learn that. `remove_root` is a second way out of the registry
/// beside `unregister`, and a volume that leaves either way is equally gone.
#[test]
fn losing_the_last_mount_retires_the_volume() {
    let manager = VolumeManager::new();
    let (volume, is_retired) = RetiringVolume::at("/Volumes/testshare");
    manager.register("testshare", volume);

    manager.remove_root(Path::new("/Volumes/testshare"));

    assert!(
        is_retired(),
        "the volume left the registry, so nothing may keep acting for it"
    );
}

/// The share is still there, reached through a surviving mount, and the
/// promoted instance shares the state the background work hangs off. Retiring
/// here would stop the watcher of a share that is perfectly healthy.
#[test]
fn promoting_a_surviving_mount_retires_nobody() {
    let manager = VolumeManager::new();
    let (volume, is_retired) = RetiringVolume::at("/Volumes/testshare");
    manager.register("testshare", Arc::clone(&volume) as Arc<dyn Volume>);
    manager.register("testshare", volume.rerooted(app_path!("/Volumes/testshare-1")).unwrap());

    let outcome = manager.remove_root(Path::new("/Volumes/testshare"));

    assert!(matches!(outcome, RootRemoval::Promoted { .. }), "expected a promotion");
    assert!(!is_retired(), "the share is still registered, just at another mount");
}
