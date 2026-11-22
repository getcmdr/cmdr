//! Keeps an open `Browse snapshots` pane showing new backups (#626).
//!
//! The view is unwatchable (`ResticVolume::can_watch_listings` is false: its
//! paths aren't on any disk), but the repo's real `snapshots/` folder is, and a
//! restic backup writes its snapshot file there LAST, after its data and index.
//! So while a pane shows a repo's snapshots root, this watches that folder on
//! the PARENT and re-lists the pane when a file appears or goes. The re-list is
//! the same one a manual refresh runs: the hub reads `snapshots/` again (only
//! the new files) and rebinds `latest` under its own rules.
//!
//! Two sources, one per kind of parent:
//!
//! - **A parent `notify` can watch** (`can_watch_listings`: the local disk and
//!   OS-mounted shares) gets a stream of ours on `<repo>/snapshots`. On an
//!   OS-mounted share FSEvents sees only this Mac's writes, so a backup another
//!   machine makes shows on the next refresh.
//! - **A parent with its own watcher** reports the change through the host's
//!   composer, which calls [`directory_changed`]: a direct SMB share's
//!   CHANGE_NOTIFY covers the whole share. Nothing of ours to arm.
//!
//! Every other parent (SFTP, WebDAV, S3, a phone) reports nothing, so its pane
//! shows a new backup on the next manual refresh. ❌ No polling.
//!
//! Arming follows the LISTING, the git portal's shape (`git/arming.rs`): the
//! observer takes a hold on the repo when a root listing opens and gives it back
//! when it closes; the last hold stops the stream.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use cmdr_fs::restic_view::{VIRTUAL_ENTRY_NAME, restic_route_candidates};
use cmdr_fs::volume::{BackendKind, DirectoryChange, Volume};
use cmdr_listing::ListingId;
use cmdr_listing::host::{ListingObserver, OpenedListing};
use notify_debouncer_full::notify::{self, RecommendedWatcher, RecursiveMode, event::EventKind};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};

use super::wiring;
use crate::file_system::volume::manager::{get_volume_manager, lexical_spelling};
use crate::ignore_poison::IgnorePoison;
use crate::listing_host::listings;

/// How long a burst of events in `snapshots/` settles before one re-list. A
/// backup writes one snapshot file (temp, then rename); `restic forget` may
/// remove many at once.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// A repo, as the hub keys it: the parent's volume id and the repo folder, as
/// spelled.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct RepoKey {
    volume_id: String,
    repo_root: PathBuf,
}

impl RepoKey {
    fn snapshots_dir(&self) -> PathBuf {
        self.repo_root.join("snapshots")
    }
}

/// One watched repo.
struct Watch {
    /// Root listings holding it. The watch goes with the last.
    holders: usize,
    /// Tells this watch apart from a later one on the same repo, so a stream
    /// armed for one that has since gone isn't kept by its successor.
    epoch: u64,
    /// Our OS stream, when the parent is one `notify` can watch.
    stream: Option<Stream>,
    /// A re-list is running; a change during it runs one more after.
    refreshing: bool,
    again: bool,
    /// Changes this watch acted on, and re-lists it ran: what the cells count.
    #[cfg(test)]
    heard: usize,
    #[cfg(test)]
    relists: usize,
}

/// What keeps a watch delivering: the OS stream, which disarms when dropped.
enum Stream {
    #[cfg_attr(test, allow(dead_code, reason = "a test build arms a scripted stream"))]
    Notify(
        #[allow(dead_code, reason = "held only to keep the debouncer (and its OS watch) alive")]
        Debouncer<RecommendedWatcher, NoCache>,
    ),
    /// A test build's stream: no `fseventsd`, and a cell fires it by hand
    /// ([`fire`]). Same split as the listing watcher's.
    #[cfg(test)]
    Scripted,
}

/// Every open root listing's hold, and the watches they keep.
// DEFAULT-OK: empty is the truthful start: no pane shows a snapshots root yet.
#[derive(Default)]
struct State {
    /// The repo each open root listing holds, keyed by listing id so a close
    /// gives back exactly what its open took.
    holds: HashMap<String, RepoKey>,
    watches: HashMap<RepoKey, Watch>,
    next_epoch: u64,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);

/// Keeps a repo's `snapshots/` watched while a pane shows its snapshots root.
pub struct SnapshotWatch;

/// The observer, for the listing host to install.
pub fn observer() -> Arc<dyn ListingObserver> {
    Arc::new(SnapshotWatch)
}

impl ListingObserver for SnapshotWatch {
    fn id(&self) -> &'static str {
        "restic-snapshots"
    }

    fn listing_opened(&self, opened: &OpenedListing<'_>) {
        let Some(repo_root) = repo_a_listing_watches(opened.volume_id, opened.volume, &lexical_spelling(opened.path))
        else {
            return;
        };
        hold(
            opened.id.as_str(),
            RepoKey {
                volume_id: opened.volume_id.to_string(),
                repo_root,
            },
        );
        // A close can race the open and reach every observer first; then
        // nobody would ever give this hold back.
        if !opened.still_open() {
            release(opened.id.as_str());
        }
    }

    fn listing_closed(&self, id: &ListingId) {
        release(id.as_str());
    }
}

/// The repo a listing of `path` keeps watched: only a restic view's snapshots
/// root, the one listing a new backup changes (a snapshot's own tree never
/// does). Pure memo and string work, ❌ no I/O: this runs on every listing open.
///
/// The repo is the first candidate the hub has confirmed, the same boundary the
/// route took, so a folder named `Browse snapshots` backed up INSIDE a snapshot
/// isn't read as a repo of its own.
pub(crate) fn repo_a_listing_watches(volume_id: &str, volume: &dyn Volume, path: &Path) -> Option<PathBuf> {
    if volume.backend_kind() != BackendKind::Restic {
        return None;
    }
    let repo_root = restic_route_candidates(path)
        .find(|candidate| wiring::repos().serves_confirmed(volume_id, candidate) == Some(true))?;
    (path == repo_root.join(VIRTUAL_ENTRY_NAME)).then_some(repo_root)
}

/// The host's composer heard a change on `volume_id`: re-list the snapshots
/// panes of a watched repo whose `snapshots/` it was in. One lock and a short
/// scan of the watched repos (a handful at most), per change, never per entry.
///
/// ❗ Only a change IN `snapshots/` counts, plus an overflow (`FullRefresh`)
/// reported on it or a folder above it, which may hide a new snapshot. A direct
/// SMB share reports every change on the share, so counting changes beside the
/// repo (restic's own `data/` and `index/` writes, a busy folder next door)
/// would re-read `snapshots/` back to back: polling at network speed.
pub(crate) fn directory_changed(volume_id: &str, parent_path: &Path, change: &DirectoryChange) {
    let overflow = matches!(change, DirectoryChange::FullRefresh);
    let key = {
        let state = STATE.lock_ignore_poison();
        if state.watches.is_empty() {
            return;
        }
        state
            .watches
            .keys()
            .find(|key| {
                key.volume_id == volume_id
                    && if overflow {
                        key.snapshots_dir().starts_with(parent_path)
                    } else {
                        key.snapshots_dir() == parent_path
                    }
            })
            .cloned()
    };
    if let Some(key) = key {
        snapshots_changed(&key);
    }
}

/// Takes a hold on `key` for `listing_id`, arming the watch with the first.
fn hold(listing_id: &str, key: RepoKey) {
    let arm = {
        let mut state = STATE.lock_ignore_poison();
        // A listing id is a fresh UUID per open, so a second hold under one
        // isn't something that happens; giving it back keeps the count honest.
        if let Some(previous) = state.holds.insert(listing_id.to_string(), key.clone()) {
            drop_hold(&mut state, &previous);
        }
        if let Some(watch) = state.watches.get_mut(&key) {
            watch.holders += 1;
            None
        } else {
            state.next_epoch += 1;
            let epoch = state.next_epoch;
            state.watches.insert(
                key.clone(),
                Watch {
                    holders: 1,
                    epoch,
                    stream: None,
                    refreshing: false,
                    again: false,
                    #[cfg(test)]
                    heard: 0,
                    #[cfg(test)]
                    relists: 0,
                },
            );
            Some(epoch)
        }
    };
    if let Some(epoch) = arm {
        arm_detached(key, epoch);
    }
}

/// Gives back what `listing_id` held. Idempotent: the open's reconcile and the
/// close both call it, and the map removal decides which one releases.
fn release(listing_id: &str) {
    let gone = {
        let mut state = STATE.lock_ignore_poison();
        let Some(key) = state.holds.remove(listing_id) else {
            return;
        };
        drop_hold(&mut state, &key)
    };
    // Outside the lock: tearing an FSEvents stream down blocks a while
    // (`watcher::stop_watching` has the measurement).
    drop(gone);
}

/// Counts one hold off `key`, handing back the watch when it was the last.
fn drop_hold(state: &mut State, key: &RepoKey) -> Option<Watch> {
    let watch = state.watches.get_mut(key)?;
    watch.holders = watch.holders.saturating_sub(1);
    if watch.holders > 0 {
        return None;
    }
    state.watches.remove(key)
}

/// Arms the OS stream on the blocking pool, when the parent is one `notify`
/// can watch, and keeps it only if the same watch is still there.
///
/// Detached for the reason `watcher::start_watching_detached` is: arming is a
/// chain of blocking syscalls, and a listing open must not wait on them.
/// `tauri::async_runtime` because the sync listing-start path reaches this from
/// the IPC handler thread, which has no Tokio runtime context.
fn arm_detached(key: RepoKey, epoch: u64) {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(parent) = get_volume_manager().get(&key.volume_id) else {
            return;
        };
        if !parent.can_watch_listings() {
            // Its own watcher, if it has one, reports through the composer.
            return;
        }
        let Some(stream) = arm_stream(&key) else {
            return;
        };
        let unused = {
            let mut state = STATE.lock_ignore_poison();
            match state.watches.get_mut(&key) {
                Some(watch) if watch.epoch == epoch => {
                    watch.stream = Some(stream);
                    None
                }
                // The last pane left while we armed.
                _ => Some(stream),
            }
        };
        drop(unused);
    });
}

/// The OS half of an arm. A test build arms `Stream::Scripted`; it still
/// refuses a folder that isn't there, as `notify` would.
fn arm_stream(key: &RepoKey) -> Option<Stream> {
    #[cfg(test)]
    {
        key.snapshots_dir().is_dir().then_some(Stream::Scripted)
    }
    #[cfg(not(test))]
    {
        arm_notify_stream(key).map(Stream::Notify)
    }
}

/// A debounced, non-recursive `notify` stream on `<repo>/snapshots`. Every
/// event there is about a snapshot file, so nothing needs filtering by path.
#[cfg_attr(test, allow(dead_code, reason = "a test build arms a scripted stream"))]
fn arm_notify_stream(key: &RepoKey) -> Option<Debouncer<RecommendedWatcher, NoCache>> {
    let for_events = key.clone();
    let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
        DEBOUNCE,
        None,
        move |result: DebounceEventResult| {
            // An `Err` usually means the folder itself went (the repo deleted);
            // the next re-list says so in the pane.
            let changed = match result {
                Ok(events) => events
                    .iter()
                    // Our own re-list READS the new snapshot files, and inotify
                    // reports opens: answering those would loop.
                    .any(|event| !matches!(event.kind, EventKind::Access(_))),
                Err(_) => true,
            };
            if changed {
                snapshots_changed(&for_events);
            }
        },
        NoCache,
        notify::Config::default(),
    )
    .map_err(|e| log::warn!(target: "restic", "snapshot watch: no debouncer: {e}"))
    .ok()?;
    let dir = key.snapshots_dir();
    debouncer
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| {
            log::warn!(
                target: "restic",
                "snapshot watch: can't watch {}: {e}",
                cmdr_fs::log_private::LogPrivate::dir_path(&dir)
            )
        })
        .ok()?;
    Some(debouncer)
}

/// Something changed in `key`'s `snapshots/`: re-list its open snapshots roots
/// once the burst settles ([`DEBOUNCE`]). One re-list at a time per repo; a
/// change during one runs one more after it, so the last change is always read.
/// The task belongs to one watch (its `epoch`) and stops once that watch goes.
fn snapshots_changed(key: &RepoKey) {
    let epoch = {
        let mut state = STATE.lock_ignore_poison();
        let Some(watch) = state.watches.get_mut(key) else {
            return;
        };
        #[cfg(test)]
        {
            watch.heard += 1;
        }
        if watch.refreshing {
            watch.again = true;
            return;
        }
        watch.refreshing = true;
        watch.epoch
    };
    let key = key.clone();
    // `tauri::async_runtime`: this runs on notify's own thread, which has no
    // Tokio runtime.
    tauri::async_runtime::spawn(async move {
        // Let the burst settle (restic's temp file and its rename, a `forget` of
        // many), then fold what arrived meanwhile into the first re-list.
        tokio::time::sleep(DEBOUNCE).await;
        if let Some(watch) = STATE.lock_ignore_poison().watches.get_mut(&key)
            && watch.epoch == epoch
        {
            watch.again = false;
        }
        loop {
            relist_roots(&key).await;
            let mut state = STATE.lock_ignore_poison();
            // A watch on the same repo armed after this one went (a pane closed
            // and another opened mid re-list) runs its own re-lists.
            let Some(watch) = state.watches.get_mut(&key).filter(|watch| watch.epoch == epoch) else {
                return;
            };
            #[cfg(test)]
            {
                watch.relists += 1;
            }
            if !watch.again {
                watch.refreshing = false;
                return;
            }
            watch.again = false;
        }
    });
}

/// Re-reads every open listing of `key`'s snapshots root. The root's listing
/// re-reads `snapshots/` in the hub, which is where the new backup lands and
/// `latest` may move; a pane inside a snapshot is left alone.
async fn relist_roots(key: &RepoKey) {
    let root = key.repo_root.join(VIRTUAL_ENTRY_NAME);
    for id in listings().listings_for_path_on_volume(&key.volume_id, &root) {
        // A failed re-read keeps the pane's rows (a locked repo, a parent gone),
        // and the pane words it on its next own read. A background re-list has
        // nobody to answer.
        let _outcome = listings().refresh(&id).await;
    }
}

/// Whether a pane keeps `repo_root` on `volume_id` watched.
#[cfg(test)]
pub(crate) fn is_watching(volume_id: &str, repo_root: &Path) -> bool {
    read_watch(volume_id, repo_root, |_| ()).is_some()
}

/// Whether `repo_root`'s watch has its stream armed (the arm is detached, so it
/// lands a moment after the watch itself).
#[cfg(test)]
pub(crate) fn has_stream(volume_id: &str, repo_root: &Path) -> bool {
    read_watch(volume_id, repo_root, |watch| watch.stream.is_some()).unwrap_or(false)
}

/// How many changes `repo_root`'s watch acted on.
#[cfg(test)]
pub(crate) fn changes_heard(volume_id: &str, repo_root: &Path) -> usize {
    read_watch(volume_id, repo_root, |watch| watch.heard).unwrap_or(0)
}

/// How many re-lists `repo_root`'s watch ran.
#[cfg(test)]
pub(crate) fn relists(volume_id: &str, repo_root: &Path) -> usize {
    read_watch(volume_id, repo_root, |watch| watch.relists).unwrap_or(0)
}

/// Whether `repo_root`'s watch has a re-list running or waiting.
#[cfg(test)]
pub(crate) fn is_relisting(volume_id: &str, repo_root: &Path) -> bool {
    read_watch(volume_id, repo_root, |watch| watch.refreshing).unwrap_or(false)
}

#[cfg(test)]
fn read_watch<T>(volume_id: &str, repo_root: &Path, read: impl FnOnce(&Watch) -> T) -> Option<T> {
    let key = RepoKey {
        volume_id: volume_id.to_string(),
        repo_root: repo_root.to_path_buf(),
    };
    STATE.lock_ignore_poison().watches.get(&key).map(read)
}

/// Fires the scripted stream of `repo_root`'s watch, as a new snapshot file
/// would. `false` when no watch of ours is armed there.
#[cfg(test)]
pub(crate) fn fire(volume_id: &str, repo_root: &Path) -> bool {
    let key = RepoKey {
        volume_id: volume_id.to_string(),
        repo_root: repo_root.to_path_buf(),
    };
    let armed = STATE
        .lock_ignore_poison()
        .watches
        .get(&key)
        .is_some_and(|watch| matches!(watch.stream, Some(Stream::Scripted)));
    if armed {
        snapshots_changed(&key);
    }
    armed
}
