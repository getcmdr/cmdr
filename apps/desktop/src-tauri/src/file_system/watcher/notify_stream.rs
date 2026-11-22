//! The non-macOS half of a pane's watch: a debounced `notify` watcher (inotify on
//! Linux, `ReadDirectoryChangesW` on Windows), translated into [`ListingChange`]s. macOS
//! uses `fsevents.rs` instead.
//!
//! The watch opens the folder's OS path (`cmdr_local::reading::watch_os_path`: the path
//! itself on Linux, the verbatim `\\?\C:\…` on Windows, so a folder past `MAX_PATH` arms),
//! and every event path crosses back to the app spelling the listing keys on
//! (`listing_spelling_of`) before it leaves here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, new_debouncer_opt,
    notify::{
        self, RecommendedWatcher, RecursiveMode, Watcher as _, WatcherKind,
        event::{EventKind, ModifyKind},
    },
};

use cmdr_fs::log_private::LogPrivate;
use cmdr_local::reading::{listing_spelling_of, watch_os_path};

use super::ListingChange;

/// A live `notify` watch on one folder. Dropping it stops watching.
pub(super) struct ListingStream {
    debouncer: Debouncer<RecommendedWatcher, NoCache>,
    /// The OS path the watch opened, which `close_now` unwatches.
    os_dir: PathBuf,
}

/// Arms a non-recursive `notify` watch on `path` whose debounced batches reach
/// `deliver` on notify's own thread. A watcher error re-reads the listing and marks
/// `listing_id`'s watch lost: notify stops a watch after a fatal error, so the listing must
/// stop counting on it.
pub(super) fn arm(
    path: &Path,
    debounce: Duration,
    deliver: impl Fn(Vec<ListingChange>) + Send + 'static,
    listing_id: &str,
) -> Result<ListingStream, String> {
    let id = listing_id.to_string();
    let lost = move || watch_lost(&id);
    let os_dir = watch_os_path(path).map_err(|e| format!("Failed to watch path: {e}"))?;
    let shown = path.to_path_buf();
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
        debounce,
        None, // No tick rate limit
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                let changes = translate(events);
                // One line per window, like FSEvents' "lost history" line, so a bug bundle
                // tells an OS that dropped events (an inotify queue or a `ReadDirectoryChangesW`
                // buffer overflow) from a watch that heard nothing.
                if changes.contains(&ListingChange::Rescan) {
                    log::info!(
                        "Watcher: the OS lost events for {}, re-reading",
                        LogPrivate::dir_path(&shown)
                    );
                }
                deliver(changes);
            }
            // Watcher errors often mean the watched directory was deleted. The re-read
            // emits `directory-deleted` when it finds it gone; `lost` re-arms otherwise.
            Err(_errors) => {
                lost();
                deliver(vec![ListingChange::Rescan]);
            }
        },
        // `NoCache`: the file-id map pairs a rename's two halves by `stat`ing every
        // entry at arm time, and the handler re-stats each path anyway.
        NoCache,
        notify::Config::default(),
    )
    .map_err(|e| format!("Failed to create watcher: {}", e))?;
    debouncer
        .watch(&os_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to watch path: {}", e))?;
    Ok(ListingStream { debouncer, os_dir })
}

/// The backend stopped `listing_id`'s watch: drop its coverage now and re-arm after the
/// release quiet period (`release.rs`), which re-reads the listing and finds a vanished
/// folder gone. Without it the listing kept its coverage over a dead watch.
fn watch_lost(listing_id: &str) {
    use super::release::{QUIET, mark_released, next_release, schedule_rearm};
    use crate::ignore_poison::RwLockIgnorePoison;

    let generation = next_release();
    {
        let mut manager = super::WATCHER_MANAGER.write_ignore_poison();
        let Some(watch) = manager.watches.get_mut(listing_id) else {
            return;
        };
        // The stream stays: it's dead, and the re-arm replaces it. Dropping it here would
        // run on the backend's own delivery thread.
        mark_released(watch, generation);
    }
    schedule_rearm(listing_id.to_string(), generation, QUIET);
}

impl super::OsWatch for ListingStream {
    /// `ReadDirectoryChangesW` holds the folder open, and NTFS refuses to rename any folder
    /// above an open handle (#1340); inotify holds nothing. Asked of the backend's own kind,
    /// so no OS test sits here.
    fn pins_ancestors(&self) -> bool {
        RecommendedWatcher::kind() == WatcherKind::ReadDirectoryChangesWatcher
    }

    /// Returns once the OS handle is closed. A plain drop returns first: on Windows the
    /// `ReadDirectoryChangesW` handle closes on notify's server thread, up to one 100 ms wait
    /// later, and a rename above the folder in that window is refused. `unwatch` queues the
    /// close, and `configure` is answered by the same thread from the same queue after it, so
    /// its reply means the handle is gone (`vendor/notify/src/windows.rs`, `run`'s action
    /// loop). inotify answers `configure` too.
    fn close_now(self: Box<Self>) {
        let ListingStream { mut debouncer, os_dir } = *self;
        if debouncer.unwatch(&os_dir).is_ok() {
            // The reply is only a barrier; its `bool` (whether the option applied) means
            // nothing for the default config.
            let _ = debouncer.configure(notify::Config::default());
        }
    }
}

/// One debounced batch as listing changes, each path in the listing's app spelling. A
/// rescan-flagged, `Any`, or `Other` event (an inotify queue overflow, a
/// `ReadDirectoryChangesW` buffer overflow) is a rescan; access events say nothing changed.
/// A rename arrives as its old and new names (Windows reports them as two halves): each is
/// `Structural`, and the handler's stat sorts out which one is still there.
pub(crate) fn translate(events: Vec<DebouncedEvent>) -> Vec<ListingChange> {
    let mut changes = Vec::with_capacity(events.len());
    for event in events {
        if event.need_rescan() || matches!(event.kind, EventKind::Any | EventKind::Other) {
            changes.push(ListingChange::Rescan);
            continue;
        }
        let structural = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        );
        if matches!(event.kind, EventKind::Access(_)) {
            continue;
        }
        for path in &event.paths {
            let path = listing_spelling_of(path);
            changes.push(if structural {
                ListingChange::Structural(path)
            } else {
                ListingChange::Content(path)
            });
        }
    }
    changes
}
