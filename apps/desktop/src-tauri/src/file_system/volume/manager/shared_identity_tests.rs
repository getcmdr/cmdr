//! An id seen at two roots at once is not provably one disk (a cloned disk, a double mount,
//! a btrfs subvolume, a bind mount), so the registry says so for as long as it runs, and
//! tells its conflict listeners once per id.

use std::sync::{Arc, Mutex};

use super::VolumeManager;
use crate::file_system::volume::{InMemoryVolume, Volume};
use crate::ignore_poison::IgnorePoison;

fn rooted(root: &str) -> Arc<dyn Volume> {
    Arc::new(InMemoryVolume::new("probe").with_root(root))
}

#[test]
fn an_id_at_one_root_has_one_identity() {
    let manager = VolumeManager::new();
    manager.register("vol-one", rooted("/Volumes/Backup"));
    // The same root again (an SMB upgrade in place) is still one mount.
    manager.register("vol-one", rooted("/Volumes/Backup"));
    assert!(!manager.has_shared_identity("vol-one"));
    assert!(!manager.has_shared_identity("vol-not-registered"));
}

#[test]
fn an_id_seen_at_two_roots_at_once_stays_shared_after_one_leaves() {
    let manager = VolumeManager::new();
    manager.register("vol-clone", rooted("/media/u/A"));
    assert!(manager.register_if_absent("vol-other", rooted("/media/u/C")));
    assert!(!manager.register_if_absent("vol-clone", rooted("/media/u/B")));
    assert!(manager.has_shared_identity("vol-clone"));
    assert!(!manager.has_shared_identity("vol-other"));

    // Clone B leaves; the id was still two disks once, so it stays shared this session.
    let _ = manager.remove_root(std::path::Path::new("/media/u/B"));
    assert!(manager.has_shared_identity("vol-clone"));
}

#[test]
fn both_registration_paths_tell_the_conflict_listeners() {
    let manager = VolumeManager::new();
    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = Arc::clone(&heard);
    manager.on_identity_conflict(move |id| recorder.lock_ignore_poison().push(id.to_string()));

    manager.register("vol-a", rooted("/Volumes/A"));
    manager.register("vol-a", rooted("/Volumes/A 1"));
    manager.register("vol-b", rooted("/Volumes/B"));
    let _ = manager.register_if_absent("vol-b", rooted("/Volumes/B 1"));
    // A third root on an id already shared says nothing new.
    manager.register("vol-a", rooted("/Volumes/A 2"));

    assert_eq!(*heard.lock_ignore_poison(), ["vol-a", "vol-b"]);
}
