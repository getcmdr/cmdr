//! Each app adapter against the real subsystem it answers from, so the seams
//! are proven on both ends and not only through the engine's fakes. The events
//! adapter's matrix is `streaming_reaction_test.rs`, the lease's is
//! `streaming_test.rs`, the reading language's is `listing_host/locale.rs`, and
//! the mount table's is `listing_host/mounts.rs`. What the FSEvents watch, as the
//! first observer, hears relative to the cache is `lifecycle_order_test.rs`'s.

use std::sync::Arc;

use crate::file_system::volume::manager::get_volume_manager;
use crate::listing_host::index::AppListingIndex;
use crate::listing_host::volumes::AppVolumes;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_listing::host::{ListingIndex, ListingVolumes, Unregistered};

/// The registry serves what was registered, unrouted, and once it's gone the
/// adapter asks the app why: an id nobody knows is gone, not a server waiting
/// for a connection.
#[tokio::test]
async fn app_volumes_answer_from_the_volume_registry() {
    let volume_id = unique_test_id("adapter-volumes");
    get_volume_manager().register(&volume_id, Arc::new(InMemoryVolume::new("Adapter")) as Arc<dyn Volume>);

    assert!(AppVolumes.get(&volume_id).is_some(), "a registered volume serves");
    let resolved = AppVolumes.resolve_local_only(&volume_id, cmdr_fs::app_path!("/folder"));
    assert!(resolved.volume.is_some() && !resolved.is_routed());
    let resolved = AppVolumes.resolve(&volume_id, cmdr_fs::app_path!("/folder")).await;
    assert!(resolved.volume.is_some() && !resolved.is_routed());

    get_volume_manager().unregister(&volume_id);
    assert!(AppVolumes.get(&volume_id).is_none());
    assert!(
        matches!(
            AppVolumes
                .unregistered(&volume_id, cmdr_fs::app_path!("/no/such/mount"))
                .await,
            Unregistered::Gone
        ),
        "an id nothing serves and nothing explains is gone"
    );
}

/// The index adapter reaches the app's drive index, which knows nothing about
/// a volume it never indexed: it leaves a listing's rows as the volume read them.
#[test]
fn the_app_index_leaves_a_volume_it_never_indexed_alone() {
    let volume_id = unique_test_id("adapter-index");
    let mut entries = vec![
        FileEntry::new("photos".to_string(), "/photos".to_string(), true, false),
        FileEntry::new("a.txt".to_string(), "/a.txt".to_string(), false, false),
    ];

    AppListingIndex.enrich(&volume_id, &mut entries);
    AppListingIndex.verify_directory(&volume_id, cmdr_fs::app_path!("/"));

    assert!(
        entries
            .iter()
            .all(|e| e.recursive_size.is_none() && e.recursive_file_count.is_none()),
        "no index sizes for a volume the index never saw: {entries:?}"
    );
}

/// The engine calls observers in registration order, and the app's depend on it:
/// the change watch first, so it disarms before the git portal, the restic
/// snapshot watch, or the index-size router hears a close, then the two virtual
/// views' watches, then index sizes.
#[test]
fn the_app_installs_its_observers_watch_then_git_portal_then_restic_then_index_sizes() {
    let ids: Vec<&str> = crate::listing_host::observers()
        .iter()
        .map(|observer| observer.id())
        .collect();

    assert_eq!(ids, vec!["fs-watcher", "git-portal", "restic-snapshots", "index-sizes"]);
}
