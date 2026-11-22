//! The macOS watch's FSEvents half (`watcher/fsevents.rs`), driven with synthetic
//! events: real drops and must-scans can't be provoked from a test.
//!
//! - `classify` keeps what `notify`'s non-recursive filter kept (the root and its
//!   direct children), and lets a lost-history signal through when it's anchored at
//!   or ABOVE the root, up to `/` (cmdr-reports#85, #813).
//! - `pump` debounces batches into one delivery, ignores events that don't concern
//!   the listing, and turns the stream's overflow flag into a rescan.
//! - A lost-history event at `/` re-reads the listing end to end.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fsevent_stream::stream::{Event, StreamFlags};
use futures_util::stream;

use super::watcher::{ListingChange, WatchRoot, classify, handle_directory_change_incremental, pump};
use crate::ignore_poison::IgnorePoison;
use crate::test_support::{TestDir, wait_until_async};
use cmdr_fs::testing::unique_test_id;
use cmdr_listing::testing::TestListing;
use cmdr_local::reading::list_directory_core;

fn fsevent(path: &Path, flags: StreamFlags) -> Event {
    Event {
        path: path.to_path_buf(),
        inode: None,
        flags,
        raw_flags: flags.bits(),
        id: 1,
    }
}

const DROPPED: StreamFlags = StreamFlags::MUST_SCAN_SUBDIRS.union(StreamFlags::USER_DROPPED);

/// A watched `dir` inside a scratch folder. Returns the root as FSEvents names it
/// (its `realpath`: `/private/var/…` for a temp dir under `/var/…`).
fn watched_root(tag: &str) -> (TestDir, WatchRoot) {
    let scratch = TestDir::new(tag);
    let dir = scratch.join("dir");
    std::fs::create_dir(&dir).expect("scratch dir is writable");
    let root = WatchRoot::new(&dir);
    (scratch, root)
}

#[test]
fn lost_history_at_or_above_the_root_is_a_rescan() {
    let (_scratch, root) = watched_root("fsevents-lost-above");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");

    for anchor in [
        PathBuf::from("/"),
        canonical.parent().expect("the root has a parent").to_path_buf(),
        canonical.clone(),
        root_path(&root).to_path_buf(),
    ] {
        for flags in [
            DROPPED,
            StreamFlags::MUST_SCAN_SUBDIRS,
            StreamFlags::MUST_SCAN_SUBDIRS | StreamFlags::KERNEL_DROPPED,
        ] {
            assert_eq!(
                classify(&fsevent(&anchor, flags), &root),
                Some(ListingChange::Rescan),
                "{flags} at {}",
                anchor.display()
            );
        }
    }
}

#[test]
fn lost_history_below_the_listing_restats_a_child_or_changes_nothing() {
    let (_scratch, root) = watched_root("fsevents-lost-below");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");
    let child = canonical.join("child");

    assert_eq!(
        classify(&fsevent(&child, DROPPED), &root),
        Some(ListingChange::Content(child.clone())),
        "a must-scan on a direct child re-stats that child"
    );
    assert_eq!(classify(&fsevent(&child.join("grandchild"), DROPPED), &root), None);
    let sibling = canonical.parent().expect("the root has a parent").join("sibling");
    assert_eq!(classify(&fsevent(&sibling, DROPPED), &root), None);
}

#[test]
fn ordinary_events_count_on_the_root_and_its_children_only() {
    let (_scratch, root) = watched_root("fsevents-ordinary");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");
    let child = canonical.join("a.txt");
    let created = StreamFlags::ITEM_CREATED | StreamFlags::IS_FILE;

    assert_eq!(
        classify(&fsevent(&child, created), &root),
        Some(ListingChange::Structural(child.clone()))
    );
    assert_eq!(
        classify(
            &fsevent(&child, StreamFlags::ITEM_RENAMED | StreamFlags::IS_FILE),
            &root
        ),
        Some(ListingChange::Structural(child.clone()))
    );
    assert_eq!(
        classify(
            &fsevent(&child, StreamFlags::ITEM_MODIFIED | StreamFlags::IS_FILE),
            &root
        ),
        Some(ListingChange::Content(child.clone()))
    );
    assert_eq!(
        classify(&fsevent(&canonical.join("sub").join("deep.txt"), created), &root),
        None
    );
    assert_eq!(
        classify(
            &fsevent(&canonical, StreamFlags::INODE_META_MOD | StreamFlags::IS_DIR),
            &root
        ),
        Some(ListingChange::Content(canonical.clone())),
        "the root's mtime bump is content, so it can't escalate to a re-read"
    );
    assert_eq!(
        classify(
            &fsevent(&canonical, StreamFlags::ITEM_REMOVED | StreamFlags::IS_DIR),
            &root
        ),
        Some(ListingChange::Structural(canonical.clone())),
        "a removed root is structural: the handler re-reads it"
    );
    assert_eq!(classify(&fsevent(&child, StreamFlags::HISTORY_DONE), &root), None);
    assert_eq!(classify(&fsevent(&child, StreamFlags::IS_FILE), &root), None);
}

/// The navigated form of a child reaches the listing too (FSEvents may hand either).
#[test]
fn an_event_in_the_navigated_form_counts() {
    let (_scratch, root) = watched_root("fsevents-navigated-form");
    let child = root_path(&root).join("a.txt");

    assert_eq!(
        classify(&fsevent(&child, StreamFlags::ITEM_CREATED), &root),
        Some(ListingChange::Structural(child))
    );
}

fn root_path(root: &WatchRoot) -> &Path {
    &root.path
}

/// Runs `pump` over `batches` (ending the stream after them) and returns what it delivered.
async fn pump_batches(root: WatchRoot, batches: Vec<Vec<Event>>, overflowed: bool) -> Vec<Vec<ListingChange>> {
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&delivered);
    pump(
        stream::iter(batches),
        Arc::new(AtomicBool::new(overflowed)),
        root,
        Duration::from_millis(50),
        move |changes| sink.lock_ignore_poison().push(changes),
    )
    .await;

    delivered.lock_ignore_poison().clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batches_inside_one_window_arrive_as_one_delivery() {
    let (_scratch, root) = watched_root("fsevents-pump-window");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");
    let (a, b) = (canonical.join("a.txt"), canonical.join("b.txt"));

    let delivered = pump_batches(
        root,
        vec![
            vec![fsevent(&a, StreamFlags::ITEM_CREATED)],
            vec![fsevent(&b, StreamFlags::ITEM_MODIFIED)],
        ],
        false,
    )
    .await;

    assert_eq!(
        delivered,
        vec![vec![ListingChange::Structural(a), ListingChange::Content(b)]]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_that_dont_concern_the_listing_deliver_nothing() {
    let (_scratch, root) = watched_root("fsevents-pump-quiet");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");

    let delivered = pump_batches(
        root,
        vec![vec![fsevent(
            &canonical.join("sub").join("deep.txt"),
            StreamFlags::ITEM_CREATED,
        )]],
        false,
    )
    .await;

    assert!(delivered.is_empty(), "{delivered:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_overflowed_stream_is_a_rescan() {
    let (_scratch, root) = watched_root("fsevents-pump-overflow");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");

    let delivered = pump_batches(
        root,
        vec![vec![fsevent(&canonical.join("a.txt"), StreamFlags::ITEM_CREATED)]],
        true,
    )
    .await;

    assert_eq!(delivered, vec![vec![ListingChange::Rescan]]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_500_changes_in_a_window_collapse_to_a_rescan() {
    let (_scratch, root) = watched_root("fsevents-pump-cap");
    let canonical = std::fs::canonicalize(root_path(&root)).expect("the root exists");
    let batch = (0..501)
        .map(|i| fsevent(&canonical.join(format!("f{i}")), StreamFlags::ITEM_CREATED))
        .collect();

    let delivered = pump_batches(root, vec![batch], false).await;

    assert_eq!(delivered, vec![vec![ListingChange::Rescan]]);
}

/// The bug in cmdr-reports#85: another app moved a file out of a pane's folder while
/// macOS was dropping events, and the pane showed it for minutes. A drop anchored at
/// `/` now re-reads the listing, so the file the cache still holds goes away.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_drop_anchored_at_the_volume_root_rereads_the_listing() {
    let (_scratch, root) = watched_root("fsevents-drop-rereads");
    let dir = root_path(&root).to_path_buf();
    std::fs::write(dir.join("moved-away.txt"), b"m").expect("scratch dir is writable");
    std::fs::write(dir.join("stays.txt"), b"s").expect("scratch dir is writable");
    // An unregistered volume id, so the re-read takes the `std::fs` fallback.
    let listing = TestListing::new()
        .volume(&unique_test_id("fsevents-drop-rereads-vol"))
        .path(crate::test_support::native_app_path(&dir).as_str())
        .entries(list_directory_core(&dir, &crate::listing_host::backends::AppListings).expect("the dir lists"))
        .insert(crate::listing_host::listings(), "fsevents-drop-rereads");
    // The move nobody heard about.
    std::fs::remove_file(dir.join("moved-away.txt")).expect("scratch dir is writable");

    let listing_id = listing.id().to_string();
    pump(
        stream::iter(vec![vec![fsevent(Path::new("/"), DROPPED)]]),
        Arc::new(AtomicBool::new(false)),
        root,
        Duration::from_millis(50),
        move |changes| handle_directory_change_incremental(&listing_id, changes),
    )
    .await;

    // Generous: the re-read runs on a spawned task, which a saturated run starves.
    wait_until_async(Duration::from_secs(30), "the drop to re-read the listing", || {
        listing.entry_names() == ["stays.txt"]
    })
    .await;
}
