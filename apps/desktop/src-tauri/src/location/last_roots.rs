//! Each volume's last roots, saved across sessions (plan D8), so a path recorded under an
//! earlier root finds its disk again after that root moved.
//!
//! A volume's id is its identity and its root only an address. On Windows the address moves
//! routinely: a USB stick comes back on `F:` where it was `E:`, two drives swap letters. A
//! restored tab or an undo knows the volume id it was recorded on, so it asks [`root_move`]
//! (or [`live_path`]): when the recorded path isn't under the volume's live root but is under
//! a root that volume held before (this session's, or one saved here by an earlier session),
//! the path is rebased onto the live root. A path already under the live root comes back as
//! is, which is every path on macOS and Linux unless a disk came back at another mount point.
//!
//! ❗ **Only an id that is provably one tree rebases.** An id seen at two roots at once (a
//! `dd` clone, a double mount, a btrfs subvolume or bind mount, a mapped drive beside its UNC
//! spelling) is "shared": the registry knows it this run (`has_shared_identity`), and this
//! file remembers it for good, so a clone plugged in alone next week can't take an undo
//! meant for its twin. A shared id gets no rebase: an undo acts on the stored path or
//! refuses, as it would with no saved roots.
//!
//! `volume-roots.json` in the app data dir holds, per volume id, its last [`ROOTS_KEPT`]
//! roots, when it was last seen, and whether it was ever shared, for at most [`MAX_VOLUMES`]
//! ids seen within [`MAX_AGE_SECS`]. A cache, left on its own fallback on purpose
//! (`state_recovery/DETAILS.md` § Per store): losing it costs one rebase after a root moved,
//! never a person's data. The volume-arrival listener only notes a root in memory; a
//! `volume-roots` thread writes the file, debounced (the listener runs inside the
//! registration, so it must never wait on an fsync).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::file_system::volume::manager::get_volume_manager;
use crate::ignore_poison::IgnorePoison;
use cmdr_fs::volume::app_paths::RootMove;

/// Filename inside `{app_data_dir}/`.
const FILE_NAME: &str = "volume-roots.json";

/// How many roots each volume keeps, newest last: enough for an undo a few letter changes
/// old, small enough that the file stays a few lines per drive.
const ROOTS_KEPT: usize = 4;

/// How many volumes the file remembers. Least recently seen go first, a shared one last.
const MAX_VOLUMES: usize = 256;

/// A volume not seen for this long is forgotten: a year, longer than any undo is kept.
const MAX_AGE_SECS: u64 = 365 * 24 * 60 * 60;

/// How long `last_seen` may lag before seeing a volume again is worth a write.
const SEEN_GRANULARITY_SECS: u64 = 24 * 60 * 60;

/// How long the writer waits for more news before it writes.
const FLUSH_DEBOUNCE: Duration = Duration::from_secs(2);

/// One volume as the file holds it.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedVolume {
    /// The last roots it answered at, oldest first.
    roots: Vec<String>,
    /// When it was last seen, in Unix seconds.
    last_seen: u64,
    /// Whether it was ever seen at two roots at once. Sticky.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    shared: bool,
}

/// On-disk shape.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SavedRoots {
    volumes: BTreeMap<String, SavedVolume>,
}

impl SavedRoots {
    /// Note `root` as `id`'s newest, seen at `now`, keeping [`ROOTS_KEPT`]. `true` when that
    /// changed what the file should say.
    fn note(&mut self, id: &str, root: &str, now: u64) -> bool {
        let saved = self.volumes.entry(id.to_string()).or_default();
        let fresh = saved.last_seen.saturating_add(SEEN_GRANULARITY_SECS) > now;
        if saved.roots.last().map(String::as_str) == Some(root) && fresh {
            return false;
        }
        saved.last_seen = now;
        saved.roots.retain(|known| known != root);
        saved.roots.push(root.to_string());
        let excess = saved.roots.len().saturating_sub(ROOTS_KEPT);
        saved.roots.drain(..excess);
        true
    }

    /// Mark `id` shared for good. `true` when it wasn't yet.
    fn mark_shared(&mut self, id: &str, now: u64) -> bool {
        let saved = self.volumes.entry(id.to_string()).or_default();
        if saved.shared {
            return false;
        }
        saved.shared = true;
        saved.last_seen = saved.last_seen.max(now);
        true
    }

    fn is_shared(&self, id: &str) -> bool {
        self.volumes.get(id).is_some_and(|saved| saved.shared)
    }

    /// Forget volumes unseen for [`MAX_AGE_SECS`], then the least recently seen beyond
    /// [`MAX_VOLUMES`], dropping a never-shared one before a shared one.
    fn prune(&mut self, now: u64) {
        self.volumes
            .retain(|_, saved| saved.last_seen.saturating_add(MAX_AGE_SECS) >= now);
        let excess = self.volumes.len().saturating_sub(MAX_VOLUMES);
        if excess == 0 {
            return;
        }
        let mut by_keep_order: Vec<(bool, u64, String)> = self
            .volumes
            .iter()
            .map(|(id, saved)| (saved.shared, saved.last_seen, id.clone()))
            .collect();
        by_keep_order.sort();
        for (_, _, id) in by_keep_order.into_iter().take(excess) {
            self.volumes.remove(&id);
        }
    }
}

/// The roots as saved, kept in memory; the file mirrors this, written by the flush thread.
static SAVED: LazyLock<Mutex<Option<SavedRoots>>> = LazyLock::new(|| Mutex::new(None));

fn file_path() -> Option<PathBuf> {
    Some(crate::config::standalone_app_data_dir()?.join(FILE_NAME))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Run `f` over the saved roots, reading the file the first time.
fn with_saved<T>(f: impl FnOnce(&mut SavedRoots) -> T) -> T {
    let mut saved = SAVED.lock_ignore_poison();
    let roots = saved.get_or_insert_with(|| file_path().map(|path| read(&path)).unwrap_or_default());
    f(roots)
}

/// Every `(id, root)` earlier sessions saved, oldest root first per id. An absent or
/// unreadable file reads as none. The first call reads the file; call it at startup,
/// before any registration, so the arrival listener never does.
pub(crate) fn saved_roots() -> Vec<(String, String)> {
    with_saved(|roots| {
        roots
            .volumes
            .iter()
            .flat_map(|(id, saved)| saved.roots.iter().map(move |root| (id.clone(), root.clone())))
            .collect()
    })
}

fn read(path: &Path) -> SavedRoots {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Note `root` as `id`'s newest root, in memory, and ask the flush thread to write. Cheap
/// and never waits on the disk: it runs inside a volume registration.
pub(crate) fn note(id: &str, root: &str) {
    if root.is_empty() {
        return;
    }
    if with_saved(|roots| roots.note(id, root, now_secs())) {
        request_flush();
    }
}

/// Note that `id` was seen at two roots at once, for good. Same cost rule as [`note`].
pub(crate) fn note_shared(id: &str) {
    if with_saved(|roots| roots.mark_shared(id, now_secs())) {
        request_flush();
    }
}

/// The writer: a thread that waits for news, lets it settle for [`FLUSH_DEBOUNCE`], then
/// writes one pruned snapshot. Started on first use.
static FLUSH: LazyLock<Option<Mutex<Sender<()>>>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel::<()>();
    let spawned = std::thread::Builder::new().name("volume-roots".into()).spawn(move || {
        while rx.recv().is_ok() {
            loop {
                match rx.recv_timeout(FLUSH_DEBOUNCE) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        write_now();
                        return;
                    }
                }
            }
            write_now();
        }
    });
    match spawned {
        Ok(_) => Some(Mutex::new(tx)),
        Err(e) => {
            log::warn!(target: "volume", "Couldn't start the volume-roots writer: {e}");
            None
        }
    }
});

fn request_flush() {
    if let Some(tx) = FLUSH.as_ref() {
        // A send only fails once the writer is gone, and then there's nobody to tell.
        let _ = tx.lock_ignore_poison().send(());
    }
}

/// Prune and write the saved roots now, outside the lock.
fn write_now() {
    let snapshot = with_saved(|roots| {
        roots.prune(now_secs());
        roots.clone()
    });
    let Some(path) = file_path() else {
        return;
    };
    let json = match serde_json::to_string_pretty(&snapshot) {
        Ok(json) => json,
        Err(e) => {
            log::warn!(target: "volume", "Couldn't serialize the volume roots: {e}");
            return;
        }
    };
    if let Err(e) = crate::config::durable_write_json(&path, &path.with_extension("json.tmp"), &json) {
        log::warn!(target: "volume", "Couldn't save the volume roots: {e}");
    }
}

/// Where volume `volume_id`'s paths move now: `Some` when it's registered, provably one tree
/// (never seen at two roots at once, this run or any saved one), and held another root
/// before; `None` keeps every recorded path as it is.
pub(crate) fn root_move(volume_id: &str) -> Option<RootMove> {
    let manager = get_volume_manager();
    let live = manager.get(volume_id)?.root().to_string_lossy().into_owned();
    let shared = manager.has_shared_identity(volume_id) || with_saved(|roots| roots.is_shared(volume_id));
    let earlier = super::facts::earlier_roots_of(volume_id, &live);
    eligible_move(live, earlier, shared)
}

/// The pure half of [`root_move`].
fn eligible_move(live: String, earlier: Vec<String>, shared: bool) -> Option<RootMove> {
    if shared || earlier.is_empty() {
        return None;
    }
    Some(RootMove { live, earlier })
}

/// `path`, recorded on volume `volume_id`, as it reads under that volume's live root now:
/// `Some(rebased)` when [`root_move`] has a move that holds it, `None` to keep it.
pub(crate) fn live_path(volume_id: &str, path: &str) -> Option<String> {
    root_move(volume_id)?.rebase(path)
}

/// How long a restored tab waits for the startup sweep to register its volume.
const DISCOVERY_WAIT: Duration = Duration::from_secs(5);

/// What a restored tab does with its stored path ([`restored_tab_path`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RestoredTabPath {
    /// The stored path stands on its volume as it is.
    Keep,
    /// The volume's root moved: the tab opens here instead.
    Moved {
        /// The stored path on the volume's live root.
        path: String,
    },
    /// The registry isn't complete yet, so nothing can tell where this volume is: keep the
    /// stored path AND id untouched, unprobed. ❌ Never re-key the tab from an incomplete
    /// registry: the old root may now be another disk.
    NotReady,
}

/// Where a restored tab recorded on `volume_id` at `path` opens. An id already registered
/// answers at once; any other waits (bounded) for the startup sweep, since the volume may
/// be about to register at another root. Blocks: call it off the IPC thread.
pub(crate) fn restored_tab_path(volume_id: &str, path: &str) -> RestoredTabPath {
    restored_tab_path_after(volume_id, path, || {
        crate::file_system::discovery::wait_until_done(DISCOVERY_WAIT)
    })
}

/// [`restored_tab_path`] with the sweep wait injected: `sweep_done` blocks until the sweep
/// finished (`true`) or gave up (`false`).
fn restored_tab_path_after(volume_id: &str, path: &str, sweep_done: impl FnOnce() -> bool) -> RestoredTabPath {
    let registered = get_volume_manager().get(volume_id).is_some();
    if !registered && !sweep_done() {
        return RestoredTabPath::NotReady;
    }
    match live_path(volume_id, path) {
        Some(path) => RestoredTabPath::Moved { path },
        None => RestoredTabPath::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

    #[test]
    fn a_shared_id_never_moves() {
        let earlier = vec!["/media/u/A".to_string()];
        assert_eq!(eligible_move("/media/u/B".into(), earlier.clone(), true), None);
        let moved = eligible_move("/media/u/B".into(), earlier, false).expect("one disk, moved");
        assert_eq!(
            moved.rebase("/media/u/A/report.pdf").as_deref(),
            Some("/media/u/B/report.pdf")
        );
        assert_eq!(eligible_move("/media/u/B".into(), Vec::new(), false), None);
    }

    #[test]
    fn a_tab_on_a_volume_the_sweep_hasnt_registered_waits_or_keeps_its_id() {
        let id = "vol-last-roots-restore-probe";
        // The sweep gave up before the volume came: nothing can tell where it is.
        assert_eq!(
            restored_tab_path_after(id, "E:/photos", || false),
            RestoredTabPath::NotReady
        );

        // The volume registers while the tab waits: the answer is about the full registry.
        let answer = restored_tab_path_after(id, "/Volumes/Probe/photos", || {
            get_volume_manager().register(
                id,
                std::sync::Arc::new(cmdr_fs::volume::InMemoryVolume::new("probe").with_root("/Volumes/Probe")),
            );
            true
        });
        get_volume_manager().unregister(id);
        assert_eq!(answer, RestoredTabPath::Keep);
    }

    #[test]
    fn a_volume_keeps_its_last_few_roots_newest_last() {
        let mut saved = SavedRoots::default();
        assert!(saved.note("vol-x", "E:/", 10));
        assert!(
            !saved.note("vol-x", "E:/", 20),
            "an unchanged root seen again soon writes nothing"
        );
        assert!(
            saved.note("vol-x", "E:/", 10 + 2 * DAY),
            "a day later, the new last-seen is worth a write"
        );
        assert!(saved.note("vol-x", "F:/", 10 + 2 * DAY));
        assert!(
            saved.note("vol-x", "E:/", 10 + 2 * DAY),
            "back on E: moves E: to newest"
        );
        assert_eq!(saved.volumes["vol-x"].roots, ["F:/", "E:/"]);
        for root in ["G:/", "H:/", "I:/"] {
            saved.note("vol-x", root, 10 + 2 * DAY);
        }
        assert_eq!(saved.volumes["vol-x"].roots, ["E:/", "G:/", "H:/", "I:/"]);
    }

    #[test]
    fn shared_is_sticky_and_survives_the_file() {
        let mut saved = SavedRoots::default();
        saved.note("vol-clone", "/media/u/A", 10);
        assert!(saved.mark_shared("vol-clone", 10));
        assert!(!saved.mark_shared("vol-clone", 11));
        saved.note("vol-clone", "/media/u/B", 12);
        let json = serde_json::to_string(&saved).expect("serializes");
        let back: SavedRoots = serde_json::from_str(&json).expect("parses");
        assert!(back.is_shared("vol-clone"));
        assert_eq!(back, saved);
        assert_eq!(read(Path::new("/definitely/not/here.json")), SavedRoots::default());
    }

    #[test]
    fn pruning_forgets_old_volumes_and_caps_the_rest_shared_last() {
        let now = 1000 * DAY;
        let mut saved = SavedRoots::default();
        saved.note("vol-ancient", "/Volumes/Old", now - 400 * DAY);
        saved.mark_shared("vol-shared-old", now - 300 * DAY);
        for n in 0..MAX_VOLUMES {
            saved.note(&format!("vol-{n:04}"), "/Volumes/X", now - DAY + n as u64);
        }
        saved.prune(now);
        assert!(!saved.volumes.contains_key("vol-ancient"), "a year unseen is forgotten");
        assert_eq!(saved.volumes.len(), MAX_VOLUMES);
        assert!(saved.is_shared("vol-shared-old"), "a shared id outlives the cap");
        assert!(
            !saved.volumes.contains_key("vol-0000"),
            "the least recently seen goes first"
        );
    }
}
