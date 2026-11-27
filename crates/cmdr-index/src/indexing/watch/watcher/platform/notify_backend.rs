//! Linux and Windows: the `notify` crate (inotify on Linux, `ReadDirectoryChangesW` on Windows),
//! recursive per watched root. No event IDs and no journal, so on startup the indexer rescans,
//! comparing the disk against SQLite; live events carry a synthetic monotonic counter as
//! `event_id`.
//!
//! **A queue overflow is a `MustScanSubDirs` on every watched root** ([`rescan_anchors`]), so
//! events the OS dropped get the rescan FSEvents' own flag gets on macOS, through the same
//! routing, throttles, and status.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::pluralize::pluralize;
use tokio::sync::mpsc;

use super::super::{FsChangeEvent, FsEventFlags, WatcherBackend, WatcherError};

mod forward;
use forward::{Forward, ROOT_LOSS_ENDS_THE_WATCH};

/// No journal to replay: a restart rescans.
pub(crate) const SUPPORTS_EVENT_REPLAY: bool = false;

/// Whether a branch-watched volume is watched at its ROOT, one recursive watch whatever the
/// branches, or branch by branch.
///
/// - **Windows: the root.** A recursive `ReadDirectoryChangesW` is one directory handle and one
///   pending read for a whole subtree, so the volume root costs what any branch costs. A watch
///   per branch cost a handle, a semaphore, and a read buffer each, nested watches each
///   reported the same change, and every open handle kept the user from renaming any folder
///   above it; a phased index of `C:` reached about 16,000 of them on its second launch (#1353).
/// - **Linux: the branches.** inotify's recursive mode costs one watch per DIRECTORY against
///   `max_user_watches`, so the root would spend the machine's budget on ground we don't answer
///   for ([`NotifyBackend::start_branches`]).
const WATCHES_VOLUME_ROOT: bool = cfg!(target_os = "windows");

/// The OS API under `notify` here, for the log.
const OS_API: &str = if cfg!(target_os = "windows") {
    "ReadDirectoryChangesW"
} else {
    "inotify"
};

/// What a branch-watched volume arms watches on: its root where one recursive watch is cheap
/// ([`WATCHES_VOLUME_ROOT`]), else each branch. The loop's `WatchScope` keeps events outside the
/// branches out of the index either way, as it does for FSEvents on macOS.
fn roots_to_watch(volume_root: &Path, branches: &[PathBuf], watches_volume_root: bool) -> Vec<PathBuf> {
    if watches_volume_root {
        vec![volume_root.to_path_buf()]
    } else {
        branches.to_vec()
    }
}

/// Start watching `root`; `since_when` is ignored (no journal replay).
pub(crate) fn start(
    root: &Path,
    since_when: u64,
    event_sender: mpsc::UnboundedSender<FsChangeEvent>,
) -> Result<Box<dyn WatcherBackend>, WatcherError> {
    start_branches(root, &[root.to_path_buf()], since_when, event_sender)
}

/// Watch the covered `branches` themselves, not the volume root (see
/// [`NotifyBackend::start_branches`]).
pub(crate) fn start_branches(
    volume_root: &Path,
    branches: &[PathBuf],
    since_when: u64,
    event_sender: mpsc::UnboundedSender<FsChangeEvent>,
) -> Result<Box<dyn WatcherBackend>, WatcherError> {
    Ok(Box::new(NotifyBackend::start_branches(
        volume_root,
        branches,
        since_when,
        event_sender,
    )?))
}

/// Watches an entire volume root for filesystem changes via inotify (through the `notify` crate).
///
/// Uses `notify::RecommendedWatcher` in recursive mode. Events are translated into
/// `FsChangeEvent` with a synthetic monotonic counter for `event_id` (inotify has no
/// system-wide event IDs like FSEvents).
///
/// On startup the indexer always does a full rescan -- there is no journal replay.
pub(crate) struct NotifyBackend {
    /// Whether the watcher is running.
    running: Arc<AtomicBool>,
    /// Monotonic event counter (synthetic, not a system event ID).
    event_counter: Arc<AtomicU64>,
    /// The `notify` watcher handle. Dropped on stop.
    ///
    /// Shared and locked rather than owned outright, because a branch-watched
    /// volume adds paths to a RUNNING watcher: `notify` supports that, and it's
    /// what saves a volume from restarting its stream every time a search covers
    /// new ground.
    watcher: Arc<std::sync::Mutex<notify::RecommendedWatcher>>,
    /// The roots the OS accepted a watch on, which is the ground an overflow puts
    /// in doubt. Shared with the forward task, and grown by `watch_branch`.
    watched_roots: WatchedRoots,
    /// Branches `watch_branch` was asked for that aren't registered yet.
    pending: Arc<PendingWatches>,
    /// Raised when the OS ended the watch on its own ([`ROOT_LOSS_ENDS_THE_WATCH`]).
    ended: Arc<AtomicBool>,
    /// Task that reads notify events from the channel and forwards them.
    forward_task: Option<tokio::task::JoinHandle<()>>,
}

impl NotifyBackend {
    /// Watch a volume the way a branch-watched volume has to be watched here: on
    /// Linux the covered `branches` themselves, on Windows the volume root
    /// ([`WATCHES_VOLUME_ROOT`]).
    ///
    /// ❌ Don't copy macOS's shape on Linux. `notify`'s recursive inotify mode
    /// registers ONE watch per directory against `max_user_watches` (8,192 by
    /// default on many distributions), so watching a whole drive to then discard
    /// most of its events would spend the machine's watch budget on ground we
    /// don't answer for — and fail outright on a big tree.
    fn start_branches(
        volume_root: &Path,
        branches: &[PathBuf],
        _since_when: u64,
        event_sender: mpsc::UnboundedSender<FsChangeEvent>,
    ) -> Result<Self, WatcherError> {
        use notify::Watcher;

        let roots = &roots_to_watch(volume_root, branches, WATCHES_VOLUME_ROOT);
        let running = Arc::new(AtomicBool::new(true));
        let event_counter = Arc::new(AtomicU64::new(1));

        // Create a std channel for notify events, then forward to the tokio mpsc.
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();

        // ❌ Never follow links: inotify walks each recursive root to arm a watch
        // per directory (and again under every new folder), and a link to `/` in
        // a user folder (a Wine prefix's `dosdevices/z:`) would arm the whole disk
        // and run out of `max_user_watches` (#1538). The scanner doesn't descend
        // links either, so the watch covers exactly what the index holds.
        let config = notify::Config::default().with_follow_symlinks(false);
        let watcher = notify::RecommendedWatcher::new(notify_tx, config)
            .map_err(|e| WatcherError::StreamCreate(format!("inotify: {e}")))?;
        let watcher = Arc::new(std::sync::Mutex::new(watcher));

        // One branch that's gone (a drive renamed under its index) costs that branch,
        // ❌ never the others: failing the whole start left every live branch
        // unwatched for the session (#695). Nothing watched at all is still an error.
        let watched_roots = WatchedRoots::default();
        let mut first_refusal = None;
        for root in roots {
            match watch_one(&watcher, root) {
                Ok(()) => note_watched(&watched_roots, root),
                Err(e) => {
                    log::warn!("Watcher: skipping a branch inotify refused: {e}");
                    first_refusal.get_or_insert(e);
                }
            }
        }
        if watched_roots.lock_ignore_poison().is_empty()
            && let Some(e) = first_refusal
        {
            return Err(e);
        }
        // Name the roots, ❌ never just count them. A branch watch covering
        // ground the user isn't working in reads identically to one covering the
        // right ground when the line is a bare number, and "the index went quiet"
        // is then unfalsifiable from a log alone (CI run 33909203247).
        let root_count = roots.len().max(1);
        log::info!(
            "Watcher: started on {root_count} path(s) ({OS_API}, recursive): {}",
            std::fmt::from_fn(|f| {
                for (i, root) in roots.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", LogPrivate::dir_path(root))?;
                }
                Ok(())
            }),
        );

        let ended = Arc::new(AtomicBool::new(false));
        let forward = Forward {
            running: Arc::clone(&running),
            counter: Arc::clone(&event_counter),
            watched_roots: Arc::clone(&watched_roots),
            ended: Arc::clone(&ended),
            end_on_root_loss: ROOT_LOSS_ENDS_THE_WATCH,
        };

        let forward_task = crate::indexing::host::runtime::spawn(async move {
            // Bridge the std channel to the tokio world via spawn_blocking
            let handle = tokio::task::spawn_blocking(move || forward.run(&notify_rx, &event_sender));
            let _ = handle.await;
            log::debug!("DriveWatcher ({OS_API}) forward task exiting");
        });

        Ok(Self {
            running,
            event_counter,
            watcher,
            watched_roots,
            pending: Arc::default(),
            ended,
            forward_task: Some(forward_task),
        })
    }
}

impl WatcherBackend for NotifyBackend {
    /// Also watch `branch`, on a watcher that's already running. A no-op where the
    /// volume root's watch already carries it ([`WATCHES_VOLUME_ROOT`]).
    ///
    /// The registration walks the branch adding one inotify watch per directory,
    /// which is slow enough on a big tree to matter, so it runs on a blocking task
    /// rather than on the caller's thread (a search's, holding the index
    /// registry's lock). ONE task drains every branch asked for meanwhile
    /// ([`PendingWatches`]): a task per branch parked a blocking-pool thread per
    /// branch on the watcher's lock, and a phased index asks for thousands, which
    /// filled the runtime's pool and timed out every other blocking read in the
    /// app (#1353). The window that opens is the one between asking and landing;
    /// the branch is being WALKED across it, and its events buffer until the walk
    /// ends either way.
    fn watch_branch(&self, branch: &Path) {
        if WATCHES_VOLUME_ROOT || !self.pending.push(branch.to_path_buf()) {
            return;
        }
        let watcher = Arc::clone(&self.watcher);
        let watched_roots = Arc::clone(&self.watched_roots);
        let pending = Arc::clone(&self.pending);
        let running = Arc::clone(&self.running);
        crate::indexing::host::runtime::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let mut drain = pending.drain();
                while let Some(branch) = drain.next(&running) {
                    match watch_one(&watcher, &branch) {
                        Ok(()) => {
                            note_watched(&watched_roots, &branch);
                            log::info!("Watcher: now watching branch {}", LogPrivate::dir_path(&branch));
                        }
                        Err(e) => log::warn!("Watcher: can't watch {}: {e}", LogPrivate::dir_path(&branch)),
                    }
                }
            })
            .await;
        });
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        if let Some(task) = self.forward_task.take() {
            task.abort();
        }

        log::info!("Watcher: stopped (inotify)");
    }

    /// Return the last synthetic event counter value.
    fn last_event_id(&self) -> u64 {
        self.event_counter.load(Ordering::Relaxed)
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Never sets. The flag means OUR bridge dropped events, and this one is unbounded
    /// from `notify` to the loop. The OS dropping events is a different signal with its
    /// own route: [`rescan_anchors`].
    fn overflow_flag(&self) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn ended_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ended)
    }
}

impl Drop for NotifyBackend {
    fn drop(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            self.stop();
        }
    }
}

/// Register one recursive inotify watch, mapping the two failures a caller can
/// do nothing about (a vanished path, an exhausted `max_user_watches`) onto one
/// typed error.
fn watch_one(watcher: &std::sync::Mutex<notify::RecommendedWatcher>, root: &Path) -> Result<(), WatcherError> {
    use notify::{RecursiveMode, Watcher};

    watcher
        .lock_ignore_poison()
        .watch(root, RecursiveMode::Recursive)
        // The cause without its paths: `notify` appends the root it was given, which the
        // variant already carries typed.
        .map_err(|e| WatcherError::WatchRefused {
            root: root.to_path_buf(),
            cause: notify::Error::new(e.kind).to_string(),
        })
}

/// Branches waiting for their watch, and whether a task is already draining them, so a
/// burst of `watch_branch` calls holds ONE blocking-pool thread, however long it is.
// DEFAULT-OK: an empty queue with no drain running is how every watcher starts.
#[derive(Default)]
struct PendingWatches {
    state: std::sync::Mutex<Pending>,
}

// DEFAULT-OK: as above.
#[derive(Default)]
struct Pending {
    queue: std::collections::VecDeque<PathBuf>,
    draining: bool,
}

impl PendingWatches {
    /// Queue `branch`. True when no drain is running, so the caller must start one
    /// ([`Self::drain`]).
    fn push(&self, branch: PathBuf) -> bool {
        let mut state = self.state.lock_ignore_poison();
        state.queue.push_back(branch);
        !std::mem::replace(&mut state.draining, true)
    }

    /// The one drain a `push` that returned true asked for.
    fn drain(&self) -> Drain<'_> {
        Drain {
            pending: self,
            ended: false,
        }
    }
}

/// The running drain. However it ends (the queue ran dry, the watcher stopped, or a
/// registration panicked), the mark goes with it, so the next `push` starts a new one:
/// a mark left up would make every later branch wait for a drain that's gone.
struct Drain<'a> {
    pending: &'a PendingWatches,
    /// `next` already took the mark down, under the lock that found the queue empty. A
    /// second take-down on drop could clear the mark of a NEWER drain a `push` started
    /// since, and let a third run beside it.
    ended: bool,
}

impl Drain<'_> {
    /// The next branch to register, or `None`, which ends the drain under the same lock
    /// that found the queue empty: a `push` that lands after it starts a new drain, one
    /// that lands before it is drained here. A stopped watcher (`running` false) drops
    /// what's left, since nobody reads its events.
    fn next(&mut self, running: &AtomicBool) -> Option<PathBuf> {
        let mut state = self.pending.state.lock_ignore_poison();
        if !running.load(Ordering::Relaxed) {
            state.queue.clear();
        }
        let next = state.queue.pop_front();
        if next.is_none() {
            state.draining = false;
            self.ended = true;
        }
        next
    }
}

impl Drop for Drain<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.pending.state.lock_ignore_poison().draining = false;
        }
    }
}

/// The roots a running watcher holds a watch on.
type WatchedRoots = Arc<std::sync::Mutex<Vec<PathBuf>>>;

/// Record a root the OS accepted a watch on (once, however often it's asked for).
fn note_watched(watched_roots: &WatchedRoots, root: &Path) {
    let mut roots = watched_roots.lock_ignore_poison();
    if !roots.iter().any(|known| known == root) {
        roots.push(root.to_path_buf());
    }
}

/// What a queue overflow becomes: one `MustScanSubDirs` anchor per watched root.
///
/// The OS says only THAT it dropped events, not where, so every root this watcher
/// holds is in doubt: the same meaning as FSEvents coalescing to its stream's root.
/// Naming the roots as anchors (rather than raising `overflow_flag`) puts the signal
/// on the route macOS's takes, and that route is what bounds it: the loop merges a
/// burst of overflows into one anchor per root per flush, a volume-wide root meets
/// the sweep window (counted into the drive's status while it waits), a branch root
/// meets the cost-proportional throttle on the single-flight drain, and an anchor
/// that lands during a scan waits for the scan to end.
///
/// Which backends say so (verified against the source of `notify` 8.2.1, the locked
/// version, 2026-11-12):
///
/// - **inotify**: `IN_Q_OVERFLOW` arrives as `EventKind::Other` with `Flag::Rescan`.
///   The kernel queues one per overflow episode, however many events it dropped.
/// - **`ReadDirectoryChangesW`**: a zero-byte completion or `ERROR_NOTIFY_ENUM_DIR`
///   (its 64 KiB buffer overflowed: "enumerate the directory yourself") arrives as
///   `EventKind::Other` with `Flag::Rescan`, through the vendored `notify`'s patch
///   (`vendor/notify/CMDR-PATCH.md`, #1128). With the volume root the only watch
///   there, its anchor is the root, which the branch scope re-anchors to EVERY branch:
///   one overflow re-walks all walked ground, once per branch's throttle window. The
///   buffer is what keeps that rarer (a burst of about 3,800 files overflowed 16 KiB and
///   not 64 KiB on an idle builder, though 64 KiB too while an index walk held both
///   cores, 2026-11-22; `watch/DETAILS.md` § "When the OS drops events").
fn rescan_anchors(watched_roots: &[PathBuf], counter: &AtomicU64) -> Vec<FsChangeEvent> {
    log::info!(
        "Watcher: the system dropped file change events (its queue overflowed), so {} will be rescanned",
        pluralize(watched_roots.len() as u64, "watched path")
    );
    watched_roots
        .iter()
        .map(|root| FsChangeEvent {
            path: event_path(root),
            event_id: counter.fetch_add(1, Ordering::Relaxed),
            flags: FsEventFlags {
                must_scan_sub_dirs: true,
                item_is_dir: true,
                ..Default::default()
            },
        })
        .collect()
}

/// What a queue overflow over `roots` puts on the watcher's channel, for a test that
/// drives it on through the reconciler's routing.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn overflow_events_for_test(roots: &[PathBuf]) -> Vec<FsChangeEvent> {
    let watched_roots = WatchedRoots::default();
    for root in roots {
        note_watched(&watched_roots, root);
    }
    let overflow = notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan);
    translate_notify_event(overflow, &AtomicU64::new(1), &watched_roots)
}

/// Translate a `notify::Event` into zero or more `FsChangeEvent`s.
///
/// The `notify` crate emits one event per filesystem operation, possibly affecting
/// multiple paths (for example, a rename emits one event with both old and new paths).
/// An event flagged for a rescan carries no paths at all: see [`rescan_anchors`].
fn translate_notify_event(
    event: notify::Event,
    counter: &AtomicU64,
    watched_roots: &std::sync::Mutex<Vec<PathBuf>>,
) -> Vec<FsChangeEvent> {
    use notify::EventKind;

    // Before the kind: the flag is what says events were lost, on any kind.
    if event.need_rescan() {
        return rescan_anchors(&watched_roots.lock_ignore_poison(), counter);
    }

    let flags = match &event.kind {
        EventKind::Create(_) => FsEventFlags {
            item_created: true,
            ..classify_paths(&event.paths)
        },
        EventKind::Remove(_) => FsEventFlags {
            item_removed: true,
            ..classify_paths(&event.paths)
        },
        EventKind::Modify(modify_kind) => {
            use notify::event::ModifyKind;
            match modify_kind {
                ModifyKind::Name(_) => FsEventFlags {
                    item_renamed: true,
                    ..classify_paths(&event.paths)
                },
                _ => FsEventFlags {
                    item_modified: true,
                    ..classify_paths(&event.paths)
                },
            }
        }
        EventKind::Access(_) | EventKind::Other | EventKind::Any => {
            return Vec::new(); // skip non-mutation events
        }
    };

    event
        .paths
        .into_iter()
        .map(|path| {
            let id = counter.fetch_add(1, Ordering::Relaxed);
            FsChangeEvent {
                path: event_path(&path),
                event_id: id,
                flags: flags.clone(),
            }
        })
        .collect()
}

/// An event's path as the index's path space spells it. On Windows `notify` reports the
/// native form (the watched `C:/` joined with `\` names), and the index holds app paths
/// (`C:/Windows/x`, plan D1); on Linux the path passes through byte for byte.
fn event_path(path: &Path) -> String {
    crate::indexing::paths::app_text::native_text(path).into_owned()
}

/// Classify the first path in the list as file, dir, or symlink.
fn classify_paths(paths: &[PathBuf]) -> FsEventFlags {
    if let Some(path) = paths.first()
        && let Ok(meta) = std::fs::symlink_metadata(path)
    {
        return FsEventFlags {
            item_is_file: meta.is_file(),
            item_is_dir: meta.is_dir(),
            ..Default::default()
        };
    }
    // Path may no longer exist (deletion); the caller sets the kind flags
    Default::default()
}

#[cfg(test)]
mod tests {
    use super::super::super::{current_event_id, supports_event_replay};
    use super::*;
    use std::sync::atomic::AtomicU64;

    fn no_roots() -> std::sync::Mutex<Vec<PathBuf>> {
        std::sync::Mutex::new(Vec::new())
    }

    /// What `notify` 8.2.1's inotify backend sends for `IN_Q_OVERFLOW`.
    fn overflow_event() -> notify::Event {
        notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan)
    }

    /// The bug behind #1105: the overflow event has kind `Other`, which the translation
    /// skipped as a non-mutation, so nothing ever made up for the lost events.
    #[test]
    fn a_queue_overflow_asks_for_a_rescan_of_every_watched_root() {
        let counter = AtomicU64::new(7);
        let roots = std::sync::Mutex::new(vec![PathBuf::from("/home/me/code"), PathBuf::from("/mnt/photos")]);

        let results = translate_notify_event(overflow_event(), &counter, &roots);

        let anchors: Vec<(&str, u64)> = results.iter().map(|e| (e.path.as_str(), e.event_id)).collect();
        assert_eq!(anchors, vec![("/home/me/code", 7), ("/mnt/photos", 8)]);
        for event in &results {
            assert!(event.flags.must_scan_sub_dirs, "the anchor asks for its subtree");
            assert!(event.flags.item_is_dir);
            assert!(
                !event.flags.item_created && !event.flags.item_removed && !event.flags.item_modified,
                "an anchor is not a change to the root itself"
            );
        }
    }

    /// The flag is what counts, whichever kind a backend sets beside it.
    #[test]
    fn the_rescan_flag_wins_over_the_event_kind() {
        let counter = AtomicU64::new(1);
        let roots = std::sync::Mutex::new(vec![PathBuf::from("/home/me/code")]);
        let event = notify::Event::new(notify::EventKind::Any).set_flag(notify::event::Flag::Rescan);

        let results = translate_notify_event(event, &counter, &roots);

        assert_eq!(results.len(), 1);
        assert!(results[0].flags.must_scan_sub_dirs);
    }

    #[test]
    fn an_other_event_without_the_rescan_flag_is_still_skipped() {
        let counter = AtomicU64::new(1);
        let roots = std::sync::Mutex::new(vec![PathBuf::from("/home/me/code")]);

        let results = translate_notify_event(notify::Event::new(notify::EventKind::Other), &counter, &roots);

        assert!(results.is_empty());
    }

    #[test]
    fn a_root_asked_for_twice_is_one_anchor() {
        let roots = WatchedRoots::default();
        note_watched(&roots, Path::new("/home/me/code"));
        note_watched(&roots, Path::new("/home/me/code"));
        note_watched(&roots, Path::new("/mnt/photos"));

        let results = translate_notify_event(overflow_event(), &AtomicU64::new(1), &roots);

        assert_eq!(results.len(), 2);
    }

    /// #1353: a phased index of `C:` armed one `ReadDirectoryChangesW` per walked
    /// branch, about 16,000 on its second launch. Windows watches the volume root once.
    #[test]
    fn a_branch_watched_volume_arms_its_root_where_a_recursive_watch_is_cheap() {
        let root = PathBuf::from("/vol");
        let branches = vec![PathBuf::from("/vol/a"), PathBuf::from("/vol/b/c")];

        assert_eq!(roots_to_watch(&root, &branches, true), vec![root.clone()]);
        assert_eq!(roots_to_watch(&root, &branches, false), branches);
        assert_eq!(WATCHES_VOLUME_ROOT, cfg!(target_os = "windows"));
    }

    /// #1353: a task per `watch_branch` parked a blocking-pool thread per branch.
    #[test]
    fn a_burst_of_branches_starts_one_drain() {
        let pending = PendingWatches::default();

        assert!(pending.push(PathBuf::from("/a")), "the first branch starts the drain");
        assert!(!pending.push(PathBuf::from("/b")), "the running drain takes the second");
        assert!(!pending.push(PathBuf::from("/c")));

        let running = AtomicBool::new(true);
        let mut drain = pending.drain();
        let drained: Vec<String> = std::iter::from_fn(|| drain.next(&running))
            .map(|branch| branch.to_string_lossy().into_owned())
            .collect();
        assert_eq!(drained, ["/a", "/b", "/c"]);
        assert!(
            pending.push(PathBuf::from("/d")),
            "once the drain found the queue empty, the next branch starts a new one"
        );
    }

    /// The handoff between a drain finding the queue empty and a `push` racing it: every
    /// branch pushed from many threads is registered exactly once, and no drain is left
    /// marked running with work queued behind it (a lost wakeup would strand a branch
    /// unwatched for the session).
    #[test]
    fn branches_pushed_from_many_threads_are_each_drained_exactly_once() {
        use std::collections::HashMap;
        use std::sync::Mutex;

        const THREADS: usize = 8;
        const PER_THREAD: usize = 500;
        let pending = Arc::new(PendingWatches::default());
        let running = Arc::new(AtomicBool::new(true));
        let seen = Arc::new(Mutex::new(HashMap::<PathBuf, usize>::new()));
        let drains = Arc::new(Mutex::new(Vec::new()));

        let pushers: Vec<_> = (0..THREADS)
            .map(|t| {
                let (pending, running, seen, drains) = (
                    Arc::clone(&pending),
                    Arc::clone(&running),
                    Arc::clone(&seen),
                    Arc::clone(&drains),
                );
                std::thread::spawn(move || {
                    for i in 0..PER_THREAD {
                        if !pending.push(PathBuf::from(format!("/t{t}/b{i}"))) {
                            continue;
                        }
                        let (pending, running, seen) = (Arc::clone(&pending), Arc::clone(&running), Arc::clone(&seen));
                        drains.lock_ignore_poison().push(std::thread::spawn(move || {
                            let mut drain = pending.drain();
                            while let Some(branch) = drain.next(&running) {
                                *seen.lock_ignore_poison().entry(branch).or_default() += 1;
                            }
                        }));
                    }
                })
            })
            .collect();
        for pusher in pushers {
            pusher.join().unwrap();
        }
        for drain in std::mem::take(&mut *drains.lock_ignore_poison()) {
            drain.join().unwrap();
        }

        let seen = seen.lock_ignore_poison();
        assert_eq!(seen.len(), THREADS * PER_THREAD, "every branch registered");
        assert!(seen.values().all(|&count| count == 1), "and none twice");
        assert!(pending.push(PathBuf::from("/after")), "no drain is left marked running");
    }

    /// A registration that panics ends its drain, ❌ never the next branch's watch: the
    /// mark has to come down with it.
    #[test]
    fn a_drain_that_panics_lets_the_next_branch_start_a_new_one() {
        let pending = PendingWatches::default();
        assert!(pending.push(PathBuf::from("/a")));
        assert!(!pending.push(PathBuf::from("/b")));

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut drain = pending.drain();
            let _ = drain.next(&AtomicBool::new(true));
            panic!("notify panicked registering /a");
        }));

        assert!(unwound.is_err());
        assert!(pending.push(PathBuf::from("/c")), "the panicked drain's mark came down");
    }

    /// A watcher that stopped drops what its drain still holds: nobody reads its events.
    #[test]
    fn a_stopped_watcher_drains_nothing_more() {
        let pending = PendingWatches::default();
        assert!(pending.push(PathBuf::from("/a")));
        assert!(!pending.push(PathBuf::from("/b")));

        let mut drain = pending.drain();
        assert_eq!(drain.next(&AtomicBool::new(false)), None);
        drop(drain);
        assert!(pending.push(PathBuf::from("/c")), "and the next push starts afresh");
    }

    #[test]
    fn translate_create_event() {
        let counter = AtomicU64::new(1);
        let event = notify::Event {
            kind: notify::EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/tmp/test_file.txt")],
            attrs: Default::default(),
        };

        let results = translate_notify_event(event, &counter, &no_roots());
        assert!(!results.is_empty());
        assert!(results[0].flags.item_created);
        assert_eq!(results[0].event_id, 1);
    }

    #[test]
    fn translate_remove_event() {
        let counter = AtomicU64::new(1);
        let event = notify::Event {
            kind: notify::EventKind::Remove(notify::event::RemoveKind::File),
            paths: vec![PathBuf::from("/tmp/deleted_file.txt")],
            attrs: Default::default(),
        };

        let results = translate_notify_event(event, &counter, &no_roots());
        assert!(!results.is_empty());
        assert!(results[0].flags.item_removed);
    }

    #[test]
    fn translate_rename_event() {
        let counter = AtomicU64::new(1);
        let event = notify::Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Name(notify::event::RenameMode::From)),
            paths: vec![PathBuf::from("/tmp/old_name.txt")],
            attrs: Default::default(),
        };

        let results = translate_notify_event(event, &counter, &no_roots());
        assert!(!results.is_empty());
        assert!(results[0].flags.item_renamed);
    }

    #[test]
    fn translate_access_event_is_skipped() {
        let counter = AtomicU64::new(1);
        let event = notify::Event {
            kind: notify::EventKind::Access(notify::event::AccessKind::Read),
            paths: vec![PathBuf::from("/tmp/read_file.txt")],
            attrs: Default::default(),
        };

        let results = translate_notify_event(event, &counter, &no_roots());
        assert!(results.is_empty());
    }

    #[test]
    fn event_counter_increments() {
        let counter = AtomicU64::new(10);
        let event = notify::Event {
            kind: notify::EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")],
            attrs: Default::default(),
        };

        let results = translate_notify_event(event, &counter, &no_roots());
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].event_id, 10);
        assert_eq!(results[1].event_id, 11);
        assert_eq!(counter.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn current_event_id_is_zero_without_a_journal() {
        assert_eq!(current_event_id(Path::new("/")), 0);
    }

    #[test]
    fn no_journal_means_no_replay() {
        assert!(!supports_event_replay());
    }
}
