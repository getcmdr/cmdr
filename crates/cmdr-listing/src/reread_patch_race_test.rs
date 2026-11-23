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
use std::sync::atomic::Ordering;
use std::time::Duration;

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};
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

/// A listing on a volume nothing serves, the host's `std::fs` read's case.
fn host_read_listing(tag: &str) -> (TestEngine, AppPathBuf, TestListingGuard) {
    let engine = TestEngine::builder().build();
    let dir = super::testing::app_text(&format!("/test/{tag}"));
    let listing = listing(&engine, &format!("{tag}-vol"), &dir, vec![entry(&dir, "alpha")], tag);
    (engine, dir, listing)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_read_with_a_patch_during_it_keeps_what_the_patch_saw() {
    // Nothing serves this listing's volume, so the engine can't stat what was
    // patched: the patch's own observation stands, and a later change sends its
    // own event.
    let (engine, dir, listing) = host_read_listing("race-host-read");

    let ticket = engine.begin_read(&listing.listing_id()).expect("the listing is open");
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    let outcome = engine.refresh_from_entries(ticket, vec![entry(&dir, "alpha")]).await;
    assert_eq!(outcome, RefreshOutcome::Landed);
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
    assert_eq!(patch_log_len(&listing), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_read_that_owes_a_reread_asks_the_host_to_read_again() {
    let (engine, dir, listing) = host_read_listing("race-host-overflow");

    let ticket = engine.begin_read(&listing.listing_id()).expect("the listing is open");
    engine.observe_children(&listing.listing_id(), burst_of(&dir, MAX_TRACKED_PATCHES + 1));
    let outcome = engine.refresh_from_entries(ticket, vec![entry(&dir, "alpha")]).await;
    assert_eq!(outcome, RefreshOutcome::NoVolume);
}

/// `count` new files in `dir`, as one watcher batch of adds.
fn burst_of(dir: &AppPath, count: usize) -> Vec<(PathBuf, Option<FileEntry>)> {
    (0..count)
        .map(|i| {
            let name = format!("file-{i:05}");
            (child(dir, &name), Some(entry(dir, &name)))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_check_that_saw_a_file_does_not_bring_it_back_after_a_newer_remove() {
    // The check stats bravo while it's there; the watcher's remove of bravo lands
    // before the check applies. The remove saw the newer disk, so it wins.
    let (engine, volume_id, dir, volume) = register("race-check-remove", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(
        &engine,
        &volume_id,
        &dir,
        vec![entry(&dir, "alpha")],
        "race-check-remove",
    );
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    volume.put(entry(&dir, "bravo"));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    let stat_gate = hold_stats(&volume);
    gate.notify_one();
    wait_until_async(Duration::from_secs(5), "the check to stat bravo", || {
        volume.stats() == 1
    })
    .await;

    volume.delete(&entry(&dir, "bravo").path);
    engine.observe_children(&listing.listing_id(), vec![(child(&dir, "bravo"), None)]);
    stat_gate.notify_waiters();
    refresh.await.expect("the refresh ends");

    assert_eq!(names(listing.entries()), ["alpha"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_check_that_found_a_file_gone_does_not_remove_it_after_a_newer_add() {
    // The reverse: the check stats bravo while it's gone; the watcher's add of
    // bravo lands before the check applies. The add saw the newer disk.
    let (engine, volume_id, dir, volume) = register("race-check-add", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let both = vec![entry(&dir, "alpha"), entry(&dir, "bravo")];
    let listing = listing(&engine, &volume_id, &dir, both.clone(), "race-check-add");
    let gate = hold_next_read(&volume, both);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(&listing.listing_id(), vec![(child(&dir, "bravo"), None)]);
    let stat_gate = hold_stats(&volume);
    gate.notify_one();
    wait_until_async(Duration::from_secs(5), "the check to stat bravo", || {
        volume.stats() == 1
    })
    .await;

    volume.put(entry(&dir, "bravo"));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    stat_gate.notify_waiters();
    refresh.await.expect("the refresh ends");

    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
}

/// Holds every stat on `volume` from now on, once it looked at the disk.
fn hold_stats(volume: &GatedListVolume) -> Arc<Notify> {
    let gate = Arc::new(Notify::new());
    *volume.stat_gate.lock_ignore_poison() = Some(Arc::clone(&gate));
    gate
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stat_that_cannot_tell_twice_keeps_what_the_patch_saw_with_no_extra_read() {
    let (engine, volume_id, dir, volume) = register("race-unknown", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-unknown");
    volume
        .unanswerable
        .lock_ignore_poison()
        .insert(entry(&dir, "bravo").path);
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    gate.notify_one();
    refresh.await.expect("the refresh ends");

    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
    assert_eq!(volume.stats(), 2, "one stat and one retry");
    assert_eq!(volume.started(), 1, "no extra full re-read");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_volume_whose_stat_lists_the_parent_reads_the_folder_again_instead() {
    let (engine, volume_id, dir, volume) = register("race-lists-parent", Vec::new());
    volume.stat_lists_the_parent.store(true, Ordering::SeqCst);
    let listing = listing(
        &engine,
        &volume_id,
        &dir,
        vec![entry(&dir, "alpha")],
        "race-lists-parent",
    );
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    volume.script.lock_ignore_poison().push_back(Read {
        gate: None,
        entries: vec![entry(&dir, "alpha"), entry(&dir, "bravo")],
    });
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    gate.notify_one();
    refresh.await.expect("the refresh ends");
    wait_until_async(Duration::from_secs(5), "the second read to land", || {
        volume.started() == 2 && listing.entries().len() == 2
    })
    .await;
    assert_eq!(volume.stats(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_case_only_rename_during_a_reread_reads_the_folder_again() {
    // `foo` became `Foo`. On a case-insensitive volume a stat finds either
    // spelling, so checking them one by one would keep both rows.
    let (engine, volume_id, dir, volume) = register("race-case", Vec::new());
    volume.put(entry(&dir, "Foo"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "foo")], "race-case");
    let gate = hold_next_read(&volume, vec![entry(&dir, "foo")]);
    volume.script.lock_ignore_poison().push_back(Read {
        gate: None,
        entries: vec![entry(&dir, "Foo")],
    });
    let refresh = start_refresh(&engine, &volume, &listing).await;

    engine.observe_children(
        &listing.listing_id(),
        vec![
            (child(&dir, "foo"), None),
            (child(&dir, "Foo"), Some(entry(&dir, "Foo"))),
        ],
    );
    gate.notify_one();
    refresh.await.expect("the refresh ends");
    wait_until_async(Duration::from_secs(5), "the second read to land", || {
        volume.started() == 2 && names(listing.entries()) == ["Foo"]
    })
    .await;
    assert_eq!(volume.stats(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_read_stops_the_listing_noting_patches() {
    let (engine, volume_id, dir, volume) = register("race-failed", Vec::new());
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-failed");
    volume.failing_reads.store(1, Ordering::SeqCst);

    assert_eq!(engine.refresh(&listing.listing_id()).await, RefreshOutcome::Skipped);
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    assert_eq!(patch_log_len(&listing), 0);
    assert_eq!(listing.with_listing(|listing| listing.reads_in_flight), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_host_read_stops_the_listing_noting_patches() {
    let (engine, dir, listing) = host_read_listing("race-dropped-ticket");
    let ticket = engine.begin_read(&listing.listing_id()).expect("the listing is open");
    // The host's read failed, so it never hands the ticket over.
    drop(ticket);
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    assert_eq!(patch_log_len(&listing), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlapping_reads_keep_every_patch_whichever_lands_first() {
    for newer_lands_first in [true, false] {
        let tag = format!("race-overlap-{newer_lands_first}");
        let (engine, volume_id, dir, volume) = register(&tag, Vec::new());
        volume.put(entry(&dir, "alpha"));
        let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], &tag);

        let older_gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
        let older = start_refresh(&engine, &volume, &listing).await;
        volume.put(entry(&dir, "bravo"));
        engine.observe_children(
            &listing.listing_id(),
            vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
        );

        let newer_gate = hold_next_read(&volume, vec![entry(&dir, "alpha"), entry(&dir, "bravo")]);
        let newer = start_refresh(&engine, &volume, &listing).await;
        volume.put(entry(&dir, "charlie"));
        engine.observe_children(
            &listing.listing_id(),
            vec![(child(&dir, "charlie"), Some(entry(&dir, "charlie")))],
        );

        let (first, second) = if newer_lands_first {
            ((newer_gate, newer), (older_gate, older))
        } else {
            ((older_gate, older), (newer_gate, newer))
        };
        first.0.notify_one();
        first.1.await.expect("the first refresh ends");
        second.0.notify_one();
        second.1.await.expect("the second refresh ends");

        assert_eq!(
            names(listing.entries()),
            ["alpha", "bravo", "charlie"],
            "newer lands first: {newer_lands_first}"
        );
        assert_eq!(patch_log_len(&listing), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_backend_replacement_during_a_reread_leaves_the_patch_and_stops_noting() {
    let (engine, volume_id, dir, volume) = register("race-replaced", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-replaced");
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    let refresh = start_refresh(&engine, &volume, &listing).await;

    volume.put(entry(&dir, "bravo"));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    // The backend's own read, newer than the ⌘R's, lands first.
    super::testing::publish_replacement(
        &engine,
        listing.id(),
        vec![entry(&dir, "alpha"), entry(&dir, "bravo")],
        0,
    );
    gate.notify_one();
    assert_eq!(refresh.await.expect("the refresh ends"), RefreshOutcome::Superseded);

    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
    assert_eq!(listing.with_listing(|listing| listing.reads_in_flight), 0);
    assert_eq!(patch_log_len(&listing), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_respell_also_rechecks_what_was_patched_while_it_read() {
    let (engine, volume_id, dir, volume) = register("race-respell", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-respell");
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    let respell = {
        let (inner, volume_id, listing_id, dir) = (
            Arc::clone(&engine.inner),
            volume_id.clone(),
            listing.id().to_owned(),
            dir.clone(),
        );
        tokio::spawn(async move { inner.respell_listing(volume_id, listing_id, dir).await })
    };
    wait_until_async(Duration::from_secs(5), "the respell to start reading", || {
        volume.started() == 1
    })
    .await;

    volume.put(entry(&dir, "bravo"));
    engine.observe_children(
        &listing.listing_id(),
        vec![(child(&dir, "bravo"), Some(entry(&dir, "bravo")))],
    );
    gate.notify_one();
    respell.await.expect("the respell ends");
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_gone_child_the_listing_never_held_leaves_it_untouched_with_no_read_in_flight() {
    let (engine, volume_id, dir, _volume) = register("race-quiet-gone", Vec::new());
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(dir.as_str())
        .entries(vec![entry(&dir, "alpha")])
        .last_accessed_ms(5)
        .insert(&engine, "race-quiet-gone");
    engine.observe_children(&listing.listing_id(), vec![(child(&dir, "bravo"), None)]);
    assert_eq!(
        listing.with_listing(|listing| listing.last_accessed_ms.load(Ordering::SeqCst)),
        5,
        "touching it would keep a background listing from being reaped"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_patch_to_a_name_that_is_not_utf8_reads_the_folder_again() {
    use std::os::unix::ffi::OsStrExt;

    let (engine, volume_id, dir, volume) = register("race-lossy", Vec::new());
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-lossy");
    let gate = hold_next_read(&volume, vec![entry(&dir, "alpha")]);
    volume.script.lock_ignore_poison().push_back(Read {
        gate: None,
        entries: vec![entry(&dir, "alpha")],
    });
    let refresh = start_refresh(&engine, &volume, &listing).await;

    let lossy = child(&dir, "placeholder").with_file_name(std::ffi::OsStr::from_bytes(b"caf\xe9"));
    engine.observe_children(&listing.listing_id(), vec![(lossy, None)]);
    gate.notify_one();
    refresh.await.expect("the refresh ends");
    wait_until_async(Duration::from_secs(5), "the second read", || volume.started() == 2).await;
    assert_eq!(volume.stats(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_check_whose_stats_never_answer_keeps_the_patch_and_frees_the_turn() {
    // A mount dies while the check stats: each pass waits out its budget, then the
    // patch's own observation stands, and the folder's turn was free all along.
    let (engine, volume_id, dir, volume) = register("race-dead-mount", Vec::new());
    volume.put(entry(&dir, "alpha"));
    let listing = listing(&engine, &volume_id, &dir, vec![entry(&dir, "alpha")], "race-dead-mount");
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
    let _never_answers = hold_stats(&volume);
    volume.script.lock_ignore_poison().push_back(Read {
        gate: None,
        entries: vec![entry(&dir, "alpha"), entry(&dir, "bravo")],
    });
    gate.notify_one();
    wait_until_async(Duration::from_secs(1), "the check to stat bravo", || {
        volume.stats() == 1
    })
    .await;

    let next = spawn_full_refresh(&engine, &volume_id, &dir, listing.id());
    wait_until_async(
        Duration::from_secs(1),
        "the next FullRefresh to read while the check still waits",
        || volume.started() == 2,
    )
    .await;
    assert!(!full.is_finished(), "the first check is still waiting on its stat");

    next.await.expect("the next FullRefresh ends");
    full.await
        .expect("the FullRefresh ends despite stats that never answer");
    assert_eq!(names(listing.entries()), ["alpha", "bravo"]);
    assert_eq!(volume.stats(), 2, "one stat and one retry");
}
