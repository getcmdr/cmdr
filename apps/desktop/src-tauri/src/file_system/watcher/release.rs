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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::ignore_poison::RwLockIgnorePoison;
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
    let count = held.len();
    take_back_after(held, quiet);
    count
}

/// Listing watches let go until someone hands them back: a write's quiet period
/// ([`release_with`]), or a drive's removal or lock query (Windows,
/// `volumes/platform/windows/watcher/`), which takes them back only if the removal is refused.
/// Dropping it re-arms nothing: a drive that left takes its panes elsewhere, which closes the
/// listings.
#[derive(Debug)]
pub(crate) struct HeldWatches {
    generation: u64,
    listing_ids: Vec<String>,
}

impl HeldWatches {
    /// How many watches it holds.
    pub(crate) fn len(&self) -> usize {
        self.listing_ids.len()
    }
}

/// Closes every listing watch on `root` or a folder below it, synchronously, under one fresh
/// generation, with no timer to bring them back: [`take_back_after`] does, or nothing. `None`
/// when no watch was there, without taking the write lock.
pub(crate) fn hold_at_or_below(announced: &Path) -> Option<HeldWatches> {
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

/// After `quiet` (zero for a refused removal), re-arms what [`hold_at_or_below`] let go and
/// re-reads each listing, skipping one a later release or a close took over.
pub(crate) fn take_back_after(held: HeldWatches, quiet: Duration) {
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
