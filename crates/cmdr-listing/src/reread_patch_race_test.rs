//! A full re-read that started before a watcher patch and lands after it must
//! not drop what the patch said (#1342).
//!
//! The landing re-stats the names patched while the read was in flight and
//! patches them again, so whichever of the two saw the disk last wins. More
//! names than the log holds fall back to one more full re-read.
//!
//! Each test holds a read open on a gate, patches the listing while it waits,
//! then lets it land: no timing, so the race is the same on every OS.

use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::app_path::{AppName, AppPath, PathArithmetic};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::wait_until_async;
use std::path::PathBuf;
use tokio::sync::Notify;

use super::Listings;
use super::RefreshOutcome;
use super::patch_log::MAX_TRACKED_PATCHES;
use super::read_generation_test::{GatedListVolume, Read, entry, names, register, spawn_full_refresh};
use super::testing::{TestEngine, TestListing, TestListingGuard};

/// Scripts `answer` as `volume`'s next read, held open until the returned gate
/// is notified.
fn hold_next_read(volume: &GatedListVolume, answer: Vec<FileEntry>) -> Arc<Notify> {
    let gate = Arc::new(Notify::new());
    volume.script.lock_ignore_poison().push_back(Read {
        gate: Some(Arc::clone(&gate)),
        entries: answer,
    });
    gate
}

fn listing(
    engine: &TestEngine,
    volume_id: &str,
    dir: &AppPath,
    entries: Vec<FileEntry>,
    tag: &str,
) -> TestListingGuard {
    TestListing::new()
        .volume(volume_id)
        .path(dir.as_str())
        .entries(entries)
        .insert(engine, tag)
}

fn child(dir: &AppPath, name: &str) -> PathBuf {
    PathBuf::from(
        dir.join(AppName::from_app_spelling(name).expect("one plain name"))
            .expect("joins")
            .into_string(),
    )
}

fn sized(dir: &AppPath, name: &str, size: u64) -> FileEntry {
    let mut entry = entry(dir, name);
    entry.size = Some(size);
    entry
}

/// `engine`'s ⌘R of `listing`, on a task of its own, started once the volume
/// saw its read begin.
async fn start_refresh(
    engine: &Listings,
    volume: &GatedListVolume,
    listing: &TestListingGuard,
) -> tokio::task::JoinHandle<RefreshOutcome> {
    let reads_before = volume.started();
    let (engine, id) = (engine.clone(), listing.listing_id());
    let task = tokio::spawn(async move { engine.refresh(&id).await });
    wait_until_async(Duration::from_secs(5), "the re-read to start reading", || {
        volume.started() > reads_before
    })
    .await;
    task
}

fn patch_log_len(listing: &TestListingGuard) -> usize {
    listing.with_listing(|listing| listing.patch_log.len())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_added_while_a_reread_was_reading_stays_after_it_lands() {
    let (engine, volume_id, dir, volume) = register("race-add", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-add");
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    volume.put(entry(&dir, "bravo"));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);

    gate.notify_one();
    assert_eq!(refresh.await.expect("the refresh ends"), RefreshOutcome::Landed);
    assert_eq!(
        names(listing.entries()),
        ["alpha", "bravo"],
        "the read started before bravo existed, so landing it alone would drop bravo"
    );
    assert_eq!(
        patch_log_len(&listing),
        0,
        "nothing in flight, so nothing stays tracked"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_removed_while_a_reread_was_reading_stays_gone_after_it_lands() {
    let (engine, volume_id, dir, volume) = register("race-remove", Vec::new());
    volume.put(entry(&dir, "alpha"));
    volume.put(entry(&dir, "bravo"));
    let both = vec![entry(&dir, "alpha"), entry(&dir, "bravo")];
    let listing = listing(&engine, &volume_id, &dir, both.clone(), "race-remove");
    let gate = hold_next_read(&volume, both);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    volume.delete(&entry(&dir, "bravo").path);
    engine.observe_children(&listing.listing_id(), vec![(child(&dir, "bravo"), None)]);
    assert_eq!(names(listing.entries()), ["alpha"]);

    gate.notify_one();
    refresh.await.expect("the refresh ends");
    assert_eq!(names(listing.entries()), ["alpha"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remove_of_a_file_the_listing_never_held_still_beats_a_read_that_saw_it() {
    // The read saw bravo, which came and went before the listing ever held it:
    // the remove patch finds nothing to remove, but the read lands it anyway.
    let (engine, volume_id, dir, volume) = register("race-remove-unseen", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(
        &engine,
        &volume_id,
        &dir,
        vec![entry(&dir, "alpha")],
        "race-remove-unseen",
    );
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha"), entry(&dir, "bravo")]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(&listing.listing_id(), vec![(child(&dir, "bravo"), None)]);

    gate.notify_one();
    refresh.await.expect("the refresh ends");
    assert_eq!(names(listing.entries()), ["alpha"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_modified_while_a_reread_was_reading_keeps_its_new_stat() {
    let (engine, volume_id, dir, volume) = register("race-modify", Vec::new());
    volume.put(sized(&dir, "alpha", 1));
    let listing = listing(&engine, &volume_id, &dir, vec![sized(&dir, "alpha", 1)], "race-modify");
    let gate = hold_next_read(&volume, vec![sized(&dir, "alpha", 1)]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    volume.put(sized(&dir, "alpha", 5));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "alpha"), Some(sized(&dir, "alpha", 5)))],
    );

    gate.notify_one();
    refresh.await.expect("the refresh ends");
    let sizes: Vec<Option<u64>> = listing.entries().into_iter().map(|e| e.size).collect();
    assert_eq!(sizes, [Some(5)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_patch_for_a_file_the_read_also_saw_lands_it_once() {
    let (engine, volume_id, dir, volume) = register("race-both", Vec::new());
    volume.put(entry(&dir, "alpha"));
    volume.put(entry(&dir, "bravo"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-both");
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha"), entry(&dir, "bravo")]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );

    gate.notify_one();
    refresh.await.expect("the refresh ends");
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_refresh_also_rechecks_what_was_patched_while_it_read() {
    // The watcher's `FullRefresh` lands through the turnstile, not `refresh`.
    let (engine, volume_id, dir, volume) = register("race-full", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-full");
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    let full = spawn_full_refresh(&engine, &volume_id, &dir, listing.id());
    wait_until_async(Duration::from_secs(5), "the FullRefresh to start reading", || {
        volume.started() == 1
    })
    .await;

    volume.put(entry(&dir, "bravo"));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );

    gate.notify_one();
    full.await.expect("the FullRefresh ends");
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn more_patches_than_the_log_holds_fall_back_to_one_more_reread() {
    let (engine, volume_id, dir, volume) = register("race-overflow", Vec::new());
    let listing = listing(&engine, &volume_id, &dir, Vec::new(), "race-overflow");
    let burst: Vec<FileEntry> = (0..=MAX_TRACKED_PATCHES)
        .map(|i| entry(&dir, &format!("file-{i:05}")))
        .collect();
    let gate = hold_next_read(&volume, Vec::new());
    // The read the fallback schedules sees the whole burst.
    volume.script.lock_ignore_poison().push_back(Read {
        gate: None,
        entries: burst.clone(),
    });
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(
        &listing.listing_id(),
        burst.iter().map(|e| (child(&dir, &e.name), Some(e.clone()))).collect(),
    );

    gate.notify_one();
    refresh.await.expect("the refresh ends");
    wait_until_async(Duration::from_secs(5), "the fallback re-read to land", || {
        volume.started() == 2 && listing.entries().len() == burst.len()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_patch_with_no_read_in_flight_tracks_nothing() {
    let (engine, volume_id, dir, _volume) = register("race-quiet", Vec::new());
    let listing = listing(&engine, &volume_id, &dir, Vec::new(), "race-quiet");
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    assert_eq!(names(listing.entries()), ["bravo"]);
    assert_eq!(patch_log_len(&listing), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_read_with_a_patch_during_it_asks_the_host_to_read_again() {
    // Nothing serves this listing's volume, so the engine can't stat what was
    // patched: it lands the read and asks the host for another one.
    let engine = TestEngine::builder().build();
    let dir = super::testing::app_text("/test/race-host-read");
    let listing = listing(
        &engine,
        "race-host-read-vol",
        &dir,
        vec![entry(&dir, "alpha")],
        "race-host-read",
    );

    let ticket = engine.begin_read(&listing.listing_id()).expect("the listing is open");
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    let outcome = engine.refresh_from_entries(ticket, vec![entry(&dir, "alpha")]).await;
    assert_eq!(outcome, RefreshOutcome::NoVolume);

    let ticket = engine.begin_read(&listing.listing_id()).expect("the listing is open");
    let outcome = engine
        .refresh_from_entries(ticket, vec![entry(&dir, "alpha"), entry(&dir, "bravo")])
        .await;
    assert_eq!(outcome, RefreshOutcome::Landed);
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
}
