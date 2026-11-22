//! The macOS half of a pane's watch: one FSEvents stream per listing, through our
//! vendored `cmdr-fsevent-stream` (the indexer's FSEvents layer too).
//!
//! Why not `notify`: its FSEvents callback keeps an event only when its path
//! `starts_with` the watched folder, so a `MustScanSubDirs` / dropped-events signal
//! anchored at an ANCESTOR (as high as `/`) never reached us, and the pane never
//! learned it had lost history. Here every event reaches [`classify`], which keeps
//! what `notify`'s non-recursive filter kept (the root and its direct children)
//! plus any rescan anchored at or above the root. `file_system/DETAILS.md` § "Lost history".

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cmdr_fs::firmlinks;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fsevent_stream::ffi::{
    kFSEventStreamCreateFlagFileEvents, kFSEventStreamCreateFlagNoDefer, kFSEventStreamEventIdSinceNow,
};
use cmdr_fsevent_stream::stream::{Event, EventStreamHandler, StreamFlags, create_event_stream};
use futures_util::{Stream, StreamExt};

use super::{FULL_REREAD_THRESHOLD, ListingChange};

/// A live stream for one listing. Dropping it stops the run loop and the pump.
pub(super) struct ListingStream {
    handler: EventStreamHandler,
    pump: tauri::async_runtime::JoinHandle<()>,
}

impl super::OsWatch for ListingStream {
    /// FSEvents watches by path from `fseventsd`, holding nothing open on the folder.
    fn pins_ancestors(&self) -> bool {
        false
    }

    /// Dropping already waits for the run loop to stop.
    fn close_now(self: Box<Self>) {
        drop(self);
    }
}

impl Drop for ListingStream {
    fn drop(&mut self) {
        // Blocks until the stream's run loop thread exits (it waits for the loop to go
        // idle first), which is why `stop_watching` drops us outside the manager's lock.
        self.handler.abort();
        self.pump.abort();
    }
}

/// The two forms a listing's folder can arrive in from FSEvents: the path the user
/// navigated, and its `realpath` (FSEvents reports `/private/tmp/…` for `/tmp/…`, and
/// a symlinked root's target, like Google Drive's `My Drive`).
#[derive(Debug, Clone)]
pub(crate) struct WatchRoot {
    pub(crate) path: PathBuf,
    pub(super) canonical: PathBuf,
}

impl WatchRoot {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            canonical: std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        }
    }

    /// The root itself or a direct child: what a non-recursive watch answers for.
    fn covers(&self, event_path: &Path) -> bool {
        // The fast form first, since a stream on a busy folder hears every descendant.
        if event_path.starts_with(&self.canonical) {
            return event_path == self.canonical || event_path.parent() == Some(self.canonical.as_path());
        }
        super::event_targets_watch_root(event_path, &self.path, &self.canonical)
            || super::rebase_event_path(event_path, &self.path, &self.canonical).is_some()
    }

    /// Whether the root is `anchor` or lies under it, in any path form.
    fn lies_at_or_under(&self, anchor: &Path) -> bool {
        if self.canonical.starts_with(anchor) || self.path.starts_with(anchor) {
            return true;
        }
        let anchor = PathBuf::from(firmlinks::normalize_path(&anchor.to_string_lossy()));
        [&self.path, &self.canonical]
            .iter()
            .any(|root| PathBuf::from(firmlinks::normalize_path(&root.to_string_lossy())).starts_with(&anchor))
    }
}

/// Arms an FSEvents stream on `root.path` whose batches, debounced by `debounce`,
/// reach `deliver`.
pub(super) fn arm(
    root: WatchRoot,
    debounce: Duration,
    deliver: impl Fn(Vec<ListingChange>) + Send + Sync + 'static,
) -> Result<ListingStream, String> {
    if !root.path.exists() {
        return Err(format!("Failed to watch path: {} doesn't exist", root.path.display()));
    }
    // Latency zero plus our own debounce window, as `notify` ran it: the first event
    // of a burst opens the window, and FSEvents' own latency would only add to it.
    let (stream, handler) = create_event_stream(
        [&root.path],
        kFSEventStreamEventIdSinceNow,
        Duration::ZERO,
        kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer,
    )
    .map_err(|e| format!("Failed to watch path: {e}"))?;
    let overflow = stream.overflow_flag();
    // The tauri runtime, not `tokio::spawn`: arming runs on the blocking pool (and in
    // tests on a plain thread), outside any Tokio context.
    let pump = tauri::async_runtime::spawn(pump(stream, overflow, root, debounce, deliver));
    Ok(ListingStream { handler, pump })
}

/// Reads batches off `stream` and hands each debounced window's changes to `deliver`
/// on the blocking pool (it stats every path), one window at a time.
///
/// A window opens at the first change that concerns the listing and closes `debounce`
/// later. A raised `overflow` flag (the stream's channel was full, so events were
/// lost) is a rescan, and is cleared once reported.
pub(crate) async fn pump<S, F>(
    mut stream: S,
    overflow: Arc<AtomicBool>,
    root: WatchRoot,
    debounce: Duration,
    deliver: F,
) where
    S: Stream<Item = Vec<Event>> + Unpin,
    F: Fn(Vec<ListingChange>) + Send + Sync + 'static,
{
    let deliver = Arc::new(deliver);
    let mut ended = false;
    while !ended {
        let mut window = Window::default();
        while window.is_empty() {
            match stream.next().await {
                Some(events) => window.absorb(&events, &root),
                None => return,
            }
            if overflow.load(Ordering::Relaxed) {
                window.push(ListingChange::Rescan);
            }
        }
        let deadline = tokio::time::Instant::now() + debounce;
        loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(events)) => window.absorb(&events, &root),
                Ok(None) => {
                    ended = true;
                    break;
                }
                Err(_elapsed) => break,
            }
        }
        if overflow.swap(false, Ordering::Relaxed) {
            log::info!(
                "Watcher: FSEvents channel overflowed, re-reading {}",
                LogPrivate::dir_path(&root.path)
            );
            window.push(ListingChange::Rescan);
        }
        // `info`, once per window: the line a bug bundle needs to tell "macOS lost
        // history and we re-read" from "the watch heard nothing" (cmdr-reports#85).
        if let Some((anchor, flags)) = &window.lost_history {
            log::info!(
                "Watcher: FSEvents lost history at {} ({flags}), re-reading {}",
                LogPrivate::dir_path(anchor),
                LogPrivate::dir_path(&root.path)
            );
        }
        let changes = window.changes;
        let deliver = Arc::clone(&deliver);
        // Awaited, so batches land in order; a failed join is a panic in the handler,
        // already reported by the panic hook.
        let _ = tauri::async_runtime::spawn_blocking(move || deliver(changes)).await;
    }
}

/// One debounce window's changes, capped: past the full-re-read threshold the details
/// stop mattering, so the window holds a single rescan.
// DEFAULT-OK: an empty window is exactly "nothing heard yet", which is how every window opens.
#[derive(Default)]
struct Window {
    changes: Vec<ListingChange>,
    /// The first lost-history event that made this window a rescan, for the log line.
    lost_history: Option<(PathBuf, StreamFlags)>,
}

impl Window {
    fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    fn absorb(&mut self, events: &[Event], root: &WatchRoot) {
        for event in events {
            if let Some(change) = classify(event, root) {
                if change == ListingChange::Rescan && self.lost_history.is_none() {
                    self.lost_history = Some((event.path.clone(), event.flags));
                }
                self.push(change);
            }
        }
    }

    fn push(&mut self, change: ListingChange) {
        if matches!(self.changes.as_slice(), [ListingChange::Rescan]) {
            return;
        }
        if matches!(change, ListingChange::Rescan) || self.changes.len() >= FULL_REREAD_THRESHOLD {
            self.changes = vec![ListingChange::Rescan];
        } else {
            self.changes.push(change);
        }
    }
}

/// What one FSEvents event means for the listing on `root`, or `None` when it doesn't
/// concern it.
///
/// - **Lost history** (`MustScanSubDirs`, `UserDropped`, `KernelDropped`): anchored at
///   the root or ABOVE it, the listing re-reads; at a direct child, that child is
///   re-stated; deeper, nothing in the listing can have changed.
/// - **Everything else** counts only on the root or a direct child, as `notify`'s
///   non-recursive filter had it. A create, remove, rename, mount, or unmount is
///   structural (on the root, it's a replaced root); a data or metadata change is a
///   content change. `HistoryDone` and an event with neither are dropped, as `notify`
///   dropped them.
pub(crate) fn classify(event: &Event, root: &WatchRoot) -> Option<ListingChange> {
    let flags = event.flags;
    if flags.contains(StreamFlags::HISTORY_DONE) {
        return None;
    }
    let lost_history =
        flags.intersects(StreamFlags::MUST_SCAN_SUBDIRS | StreamFlags::USER_DROPPED | StreamFlags::KERNEL_DROPPED);
    if lost_history && root.lies_at_or_under(&event.path) {
        return Some(ListingChange::Rescan);
    }
    if !root.covers(&event.path) {
        return None;
    }
    let structural = StreamFlags::ITEM_CREATED
        | StreamFlags::ITEM_REMOVED
        | StreamFlags::ITEM_RENAMED
        | StreamFlags::MOUNT
        | StreamFlags::UNMOUNT;
    let content = StreamFlags::ITEM_MODIFIED
        | StreamFlags::INODE_META_MOD
        | StreamFlags::FINDER_INFO_MOD
        | StreamFlags::ITEM_CHANGE_OWNER
        | StreamFlags::ITEM_XATTR_MOD
        | StreamFlags::ITEM_CLONED;
    if flags.intersects(structural) {
        Some(ListingChange::Structural(event.path.clone()))
    } else if flags.intersects(content) || lost_history {
        Some(ListingChange::Content(event.path.clone()))
    } else {
        None
    }
}
