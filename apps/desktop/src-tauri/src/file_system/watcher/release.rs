//! Letting a watch go for a while, and taking it back.
//!
//! Two reasons, one mechanism:
//!
//! - **Cmdr's own write at or above a watched folder** (Windows). A `ReadDirectoryChangesW`
//!   watch holds an open handle on its folder, and NTFS refuses to rename or move any folder
//!   above an open handle, whatever its share mode (#1340). The write engine announces every
//!   path before its syscall (`OwnWrites`, `operations_host/own_writes.rs`), so each watch at or
//!   below that path closes its handle first, synchronously, and re-arms once the writes there go
//!   quiet. FSEvents and inotify watches pin nothing (`OsWatch::pins_ancestors`), so none lets go there.
//! - **A watch the OS dropped** (a fatal `ReadDirectoryChangesW` completion: a dismounted
//!   volume, a dropped share). The notify backend stops watching after it, so without this the
//!   listing would keep its coverage over a dead watch.
//!
//! While released, a watch's coverage is `None`, so no delete walk or copy scan trusts its
//! listing. The re-arm re-reads the listing unconditionally, since the watch heard nothing
//! meanwhile; when the folder is gone (renamed away with its parent, deleted), that re-read
//! lands the pane on the nearest parent through the usual `directory-deleted`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::ignore_poison::{IgnorePoison, RwLockIgnorePoison};
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::WatchCoverage;

use super::{
    WATCHER_MANAGER, WatchStream, WatchedDirectory, arm_stream, coverage_for_watched_path, handle_directory_change,
};

/// How long the writes at or above a released folder must stay quiet before its watch
/// re-arms. Each announce resets it, so a batch move holds the watch off until it's done; a
/// re-arm between an announce and its syscall would refuse that syscall again.
pub(super) const QUIET: Duration = Duration::from_millis(1_000);

/// Tells each release apart, so only the last one's timer re-arms.
static RELEASES: AtomicU64 = AtomicU64::new(1);

/// Closes every listing watch on `announced` or a folder below it, before Cmdr writes there,
/// and schedules each to re-arm once the writes go quiet.
///
/// `announced` is a local path in any spelling the write engine uses (`C:\x`, `\\?\C:\x`); it
/// crosses to the app spelling the watches key on first. Returns at once when no watch is
/// below it, which is every announce of a copy's files, so it's cheap on the hot path: one
/// read lock and a prefix compare per open listing.
pub(crate) fn release_watches_at_or_below(announced: &Path) {
    // The count is for a test; the writer asks nothing back.
    let _ = release_with(announced, QUIET);
}

/// [`release_watches_at_or_below`] with the quiet period passed in, answering how many
/// watches it let go: a hold, taken back once the writes go quiet.
pub(crate) fn release_with(announced: &Path, quiet: Duration) -> usize {
    let Some(held) = hold_at_or_below(announced) else {
        return 0;
    };
    let count = held.listing_ids.len();
    take_back_after(held, quiet);
    count
}

/// Listing watches a write let go, until its quiet period hands them back.
struct HeldWatches {
    generation: u64,
    listing_ids: Vec<String>,
}

/// Closes every listing watch on `announced` or a folder below it, synchronously, under one
/// fresh generation. `None` when no watch was there, without taking the write lock.
fn hold_at_or_below(announced: &Path) -> Option<HeldWatches> {
    let target = cmdr_local::reading::listing_spelling_of(announced);
    let any_below = WATCHER_MANAGER
        .read_ignore_poison()
        .watches
        .values()
        .any(|w| releasable(w, &target));
    if !any_below {
        return None;
    }

    let generation = next_release();
    let released: Vec<(String, PathBuf, WatchStream)> = {
        let mut manager = WATCHER_MANAGER.write_ignore_poison();
        manager
            .watches
            .iter_mut()
            .filter(|(_, w)| releasable(w, &target))
            .map(|(id, w)| {
                mark_released(w, generation);
                let stream = std::mem::replace(&mut w.stream, WatchStream::Released);
                (id.clone(), w.path.clone(), stream)
            })
            .collect()
    };

    // Outside the lock: closing waits on the OS backend's thread.
    let mut listing_ids = Vec::with_capacity(released.len());
    for (id, path, stream) in released {
        log::debug!("Watcher: letting go of {}", LogPrivate::dir_path(&path));
        close_now(stream);
        listing_ids.push(id);
    }
    Some(HeldWatches {
        generation,
        listing_ids,
    })
}

/// Drives held for a removal or lock query, in the listing spelling: nothing under one re-arms
/// (a write's quiet timer included) until [`take_back_drive`].
static HELD_DRIVES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

// The Windows drive watcher's doors (`volumes/platform/windows/watcher/`), pinned here so every
// OS builds them and their cells run everywhere.
const _: fn(&Path) -> usize = hold_drive;
const _: fn(&Path) = take_back_drive;
const _: fn(&Path) = forget_drive;

/// Holds every listing watch on the drive at `root`: closes them now and keeps them closed,
/// whatever else releases or re-arms below it, until [`take_back_drive`] or [`forget_drive`]. A
/// second hold of a held drive closes what armed since and changes nothing else. Answers how
/// many it closed.
pub(crate) fn hold_drive(root: &Path) -> usize {
    let target = cmdr_local::reading::listing_spelling_of(root);
    {
        let mut held = HELD_DRIVES.lock_ignore_poison();
        if !held.contains(&target) {
            held.push(target);
        }
    }
    hold_at_or_below(root).map_or(0, |held| held.listing_ids.len())
}

/// The removal was refused: re-arm and re-read every watch under `root` that's let go, whichever
/// release let it go last. Nothing when the drive isn't held.
pub(crate) fn take_back_drive(root: &Path) {
    let target = cmdr_local::reading::listing_spelling_of(root);
    {
        let mut held = HELD_DRIVES.lock_ignore_poison();
        let before = held.len();
        held.retain(|drive| drive != &target);
        if held.len() == before {
            return;
        }
    }
    let released: Vec<(String, u64)> = WATCHER_MANAGER
        .read_ignore_poison()
        .watches
        .iter()
        .filter(|(_, w)| w.path.starts_with(&target))
        .filter_map(|(id, w)| w.released.map(|generation| (id.clone(), generation)))
        .collect();
    for (id, generation) in released {
        schedule_rearm(id, generation, Duration::ZERO);
    }
}

/// The drive left: stop holding it, re-arming nothing (its panes move off it, which closes the
/// listings).
pub(crate) fn forget_drive(root: &Path) {
    let target = cmdr_local::reading::listing_spelling_of(root);
    HELD_DRIVES.lock_ignore_poison().retain(|drive| drive != &target);
}

/// Whether `path` lies on a drive a removal query holds.
fn on_a_held_drive(path: &Path) -> bool {
    HELD_DRIVES
        .lock_ignore_poison()
        .iter()
        .any(|drive| path.starts_with(drive))
}

/// A watch on `target` or below it whose stream pins its ancestors. A released one counts
/// (a second write restarts its quiet period), and so does a test build's scripted one, so
/// the mechanism runs under test on every OS.
fn releasable(watch: &WatchedDirectory, target: &Path) -> bool {
    let pins = match &watch.stream {
        WatchStream::Os(os) => os.pins_ancestors(),
        WatchStream::Released => true,
        #[cfg(test)]
        WatchStream::Scripted => true,
    };
    pins && watch.path.starts_with(target)
}

/// A fresh release generation: a timer re-arms only for its watch's latest one.
pub(super) fn next_release() -> u64 {
    RELEASES.fetch_add(1, Ordering::Relaxed)
}

/// Marks `watch` released under `generation`: no coverage until it re-arms.
pub(super) fn mark_released(watch: &mut WatchedDirectory, generation: u64) {
    watch.coverage = WatchCoverage::None;
    watch.released = Some(generation);
}

/// Closes `stream`'s OS handle before returning, where the backend can.
fn close_now(stream: WatchStream) {
    match stream {
        WatchStream::Os(watch) => watch.close_now(),
        other => drop(other),
    }
}

/// After `quiet`, re-arms what [`hold_at_or_below`] let go and re-reads each listing, skipping
/// one a later release or a close took over.
fn take_back_after(held: HeldWatches, quiet: Duration) {
    for id in held.listing_ids {
        schedule_rearm(id, held.generation, quiet);
    }
}

/// After `quiet`, re-arms `listing_id`'s watch if `generation` is still its latest release
/// and the listing is still open, then re-reads the listing.
pub(super) fn schedule_rearm(listing_id: String, generation: u64, quiet: Duration) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(quiet).await;
        if rearm(&listing_id, generation) {
            handle_directory_change(&listing_id).await;
        }
    });
}

/// Re-arms `listing_id` when `generation` is still its latest release. Answers whether the
/// listing should be re-read: the watch came back, or it couldn't (the folder may be gone,
/// which the re-read finds). `false` when a later release or a close took over.
pub(crate) fn rearm(listing_id: &str, generation: u64) -> bool {
    let Some(path) = WATCHER_MANAGER
        .read_ignore_poison()
        .watches
        .get(listing_id)
        .filter(|w| w.released == Some(generation))
        .map(|w| w.path.clone())
    else {
        return false;
    };
    if on_a_held_drive(&path) {
        // Its drive is being removed: re-arming would reopen a handle the removal waits on.
        // `take_back_drive` brings it back if the removal is refused.
        return false;
    }
    let stream = match arm_stream(listing_id, &path) {
        Ok(stream) => stream,
        Err(e) => {
            log::debug!(
                "Watcher: {} didn't re-arm ({e}), re-reading",
                LogPrivate::dir_path(&path)
            );
            return true;
        }
    };
    let coverage = coverage_for_watched_path(&path);
    let replaced = {
        let mut manager = WATCHER_MANAGER.write_ignore_poison();
        match manager.watches.get_mut(listing_id) {
            Some(w) if w.released == Some(generation) => {
                w.coverage = coverage;
                w.released = None;
                Some(std::mem::replace(&mut w.stream, stream))
            }
            // Closed or released again while arming: this stream isn't wanted.
            _ => {
                drop(manager);
                drop(stream);
                return false;
            }
        }
    };
    // Outside the lock, like `stop_watching`.
    drop(replaced);
    true
}
