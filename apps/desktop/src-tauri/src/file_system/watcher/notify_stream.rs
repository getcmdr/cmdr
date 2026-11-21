//! The non-macOS half of a pane's watch: a debounced `notify` watcher (inotify on
//! Linux, `ReadDirectoryChangesW` on Windows), translated into [`ListingChange`]s. macOS
//! uses `fsevents.rs` instead.
//!
//! The watch opens the folder's OS path (`cmdr_local::reading::watch_os_path`: the path
//! itself on Linux, the verbatim `\\?\C:\…` on Windows, so a folder past `MAX_PATH` arms),
//! and every event path crosses back to the app spelling the listing keys on
//! (`listing_spelling_of`) before it leaves here.

use std::path::Path;
use std::time::Duration;

use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, new_debouncer_opt,
    notify::{
        self, RecommendedWatcher, RecursiveMode,
        event::{EventKind, ModifyKind},
    },
};

use super::ListingChange;

/// A live `notify` watch. Dropping it stops watching.
pub(super) type ListingStream = Debouncer<RecommendedWatcher, NoCache>;

/// Arms a non-recursive `notify` watch on `path` whose debounced batches reach
/// `deliver` on notify's own thread. A watcher error re-reads the listing.
pub(super) fn arm(
    path: &Path,
    debounce: Duration,
    deliver: impl Fn(Vec<ListingChange>) + Send + 'static,
) -> Result<ListingStream, String> {
    let os_dir = cmdr_local::reading::watch_os_path(path).map_err(|e| format!("Failed to watch path: {e}"))?;
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
        debounce,
        None, // No tick rate limit
        move |result: DebounceEventResult| match result {
            Ok(events) => deliver(translate(events)),
            // Watcher errors often mean the watched directory was deleted. The re-read
            // emits `directory-deleted` when it finds it gone.
            Err(_errors) => deliver(vec![ListingChange::Rescan]),
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
    Ok(debouncer)
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
            let path = cmdr_local::reading::listing_spelling_of(path);
            changes.push(if structural {
                ListingChange::Structural(path)
            } else {
                ListingChange::Content(path)
            });
        }
    }
    changes
}
