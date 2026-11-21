//! The set of mount roots one volume ID owns, and the rules for picking which
//! of them is active.
//!
//! One filesystem can be reached through several mount points and they all
//! derive one volume ID (an SMB share keys on `(server, port, share)`, a local
//! disk on its filesystem UUID). So a registry entry holds the SET of roots
//! carrying its ID with exactly one ACTIVE — `volume.root()` — and promotes a
//! survivor when the active one dies. Rationale and the flows that drive it:
//! `../DETAILS.md` § "A volume ID owns a set of mount roots".

use super::{Volume, VolumeManager};
use crate::ignore_poison::RwLockIgnorePoison;
use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::os_error::OsErrorClass;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::Arc;

/// Whether two spellings name one mount root: the app path's own `==`, which compares
/// components as `PathBuf`'s did, so `/Volumes/X/` and `/Volumes/X` are one root and the
/// answer is `Path`'s for every text. ❌ Not a canonical-twin comparison: on Windows that
/// also equates `\\?\C:\x` with `C:\x`, which `Path` kept apart.
pub(super) fn same_root(a: &AppPath, b: &AppPath) -> bool {
    a == b
}

/// A mount root the OS reported (a mount, unmount, or rename event's native path) as the
/// registry's app path. A name that isn't plain Unicode text (Linux bytes) stays a lossless
/// bridge value until the escape lands, so the event still finds the root it registered.
pub(super) fn mount_root_from_os(native: &Path) -> Cow<'_, AppPath> {
    match AppPathBuf::from_native_unicode(native) {
        Ok(root) => Cow::Owned(root),
        Err(_) => AppPath::legacy_unchecked(native),
    }
}

/// One mount root carrying a volume's ID.
pub(super) struct MountRoot {
    pub(super) path: AppPathBuf,
    /// Set once an operation on this root came back with an errno that PROVES
    /// the mount is gone rather than the file. Never cleared: a root that has
    /// answered `ENOTCONN` is only trustworthy again after a fresh mount event,
    /// which re-records it from scratch.
    pub(super) proven_stale: bool,
}

impl MountRoot {
    fn new(path: &AppPath) -> Self {
        Self {
            path: path.to_owned(),
            proven_stale: false,
        }
    }

    /// Ranking for "which root should be active": liveness first, then the
    /// canonical path shape. `Less` sorts better.
    ///
    /// The path half is the original rule (shortest, ties lexicographic) and it
    /// still decides between equally-live roots: macOS suffixes the LATER mount
    /// (`/Volumes/testshare-1`), so the shortest is the original, which is what every
    /// saved path, favorite, and index row already refers to. What changed is its
    /// RANK — a proven-dead shortest root loses to a live longer one, because
    /// path shape is a guess about identity and an errno is evidence about health.
    /// The tie is the app path's own order, which is `PathBuf`'s component order, so
    /// every root keeps the order it had (`/Volumes/a b` against `/Volumes/a/b`, and a
    /// `..` before any name).
    fn rank(&self, other: &Self) -> Ordering {
        (self.proven_stale, self.path.as_str().len())
            .cmp(&(other.proven_stale, other.path.as_str().len()))
            .then_with(|| self.path.cmp(&other.path))
    }
}

/// One registry entry: the volume plus every mount root known to carry its ID.
///
/// Invariant: `roots` always contains `volume.root()`, and that entry is the
/// ACTIVE root. Everything else is a fallback the unmount and stale-mount paths
/// can promote to.
pub(super) struct Registration {
    pub(super) volume: Arc<dyn Volume>,
    pub(super) roots: Vec<MountRoot>,
}

impl Registration {
    /// A fresh registration whose only known root is the volume's own.
    pub(super) fn new(volume: Arc<dyn Volume>) -> Self {
        let root = MountRoot::new(volume.root());
        Self {
            volume,
            roots: vec![root],
        }
    }

    /// Whether any known root of this entry is `root` (active or fallback).
    pub(super) fn knows_root(&self, root: &AppPath) -> bool {
        self.roots.iter().any(|r| same_root(&r.path, root))
    }

    /// Record `root` as another mount reaching this volume. Idempotent.
    pub(super) fn record_root(&mut self, root: &AppPath) {
        if !self.knows_root(root) {
            self.roots.push(MountRoot::new(root));
        }
    }

    /// Put back a root the entry is still anchored to even though its mount is
    /// gone, marked stale so it can never win the rank again.
    ///
    /// Keeps the `roots`-contains-`volume.root()` invariant true for a backend
    /// that declined to re-root: without this the entry would be anchored to a
    /// path it no longer claims, so `find_by_root` (and a later unmount or
    /// re-mount event for that same path) would stop recognizing it.
    pub(super) fn readd_stale_root(&mut self, root: &AppPath) {
        match self.roots.iter_mut().find(|r| same_root(&r.path, root)) {
            Some(existing) => existing.proven_stale = true,
            None => self.roots.push(MountRoot {
                path: root.to_owned(),
                proven_stale: true,
            }),
        }
    }

    /// Swap the volume in place (a same-root replacement, like the SMB upgrade),
    /// keeping the fallback roots the entry has collected.
    pub(super) fn replace_volume(&mut self, volume: Arc<dyn Volume>) {
        let root = volume.root().to_owned();
        self.volume = volume;
        self.record_root(&root);
    }

    /// Swap the volume for one that may sit at a DIFFERENT root, handing back
    /// the one it replaced. The old active root leaves the set and the new one
    /// enters it fresh (never inheriting a stale mark), so nothing finds this
    /// entry by a root it no longer serves. Other fallback roots stay.
    pub(super) fn replace_root(&mut self, volume: Arc<dyn Volume>) -> Arc<dyn Volume> {
        let old_root = self.volume.root();
        let new_root = volume.root();
        if !same_root(old_root, new_root) {
            self.roots
                .retain(|r| !same_root(&r.path, old_root) && !same_root(&r.path, new_root));
            self.roots.push(MountRoot::new(new_root));
        }
        std::mem::replace(&mut self.volume, volume)
    }

    /// The root that SHOULD be active: the best-ranked one, or `None` when the
    /// entry has no roots left at all.
    fn best_root(&self) -> Option<&MountRoot> {
        self.roots.iter().min_by(|a, b| a.rank(b))
    }

    /// Whether the ACTIVE root is one this entry has proven dead.
    ///
    /// A root missing from the set counts as gone: the invariant says the set
    /// contains `volume.root()`, so the only way it isn't there is that the mount
    /// went away and nothing put it back.
    fn active_root_is_dead(&self) -> bool {
        let active = self.volume.root();
        self.roots
            .iter()
            .find(|r| same_root(&r.path, active))
            .is_none_or(|r| r.proven_stale)
    }

    /// Tell the volume when it has ended up on a mount root that's gone.
    ///
    /// Runs after every move of the active seat. The volume keeps serving (that's
    /// why it's still registered), but the paths it publishes stop being openable
    /// by anything outside Cmdr, and only the registry has that evidence — nothing
    /// may probe a mount. Cheap and idempotent by contract, and it runs under the
    /// registry write lock, so an implementation must not reach back in here.
    pub(super) fn tell_volume_if_its_root_is_dead(&self) {
        if self.active_root_is_dead() {
            self.volume.note_root_mount_gone();
        }
    }

    /// Move the ID to the best surviving root, if that isn't where it already is
    /// and the backend can be re-rooted. Returns the new active root on success.
    ///
    /// No I/O: a promotion is a pure registry swap plus whatever the backend's
    /// `rerooted` costs (for a path-addressed backend, one allocation). ❌ Never
    /// add a liveness probe here — an NSURL/`statfs` round trip on a dead network
    /// mount blocks 30–120 s, which is the whole point of `volumes/DETAILS.md`
    /// § "Hung mounts". A root that is still dead simply proves it again on the
    /// next failure and gets marked in turn.
    pub(super) fn promote_to_best_root(&mut self) -> Promotion {
        let Some(best) = self.best_root() else {
            return Promotion::NoRootsLeft;
        };
        if same_root(&best.path, self.volume.root()) {
            return Promotion::AlreadyBest;
        }
        let target = best.path.clone();
        match self.volume.rerooted(&target) {
            Some(rerooted) => {
                self.volume = rerooted;
                Promotion::Promoted(target)
            }
            None => Promotion::BackendCantReroot,
        }
    }
}

/// The mount-root half of the registry API: the two ways a root leaves the
/// active seat, and the read that shows the whole set.
impl VolumeManager {
    /// Every mount root known to reach `id`, active one first.
    #[cfg(test)]
    pub fn known_roots(&self, id: &str) -> Vec<AppPathBuf> {
        let volumes = self.volumes.read_ignore_poison();
        let Some(entry) = volumes.get(id) else {
            return Vec::new();
        };
        let active = entry.volume.root();
        let mut roots: Vec<AppPathBuf> = vec![active.to_owned()];
        roots.extend(
            entry
                .roots
                .iter()
                .filter(|r| !same_root(&r.path, active))
                .map(|r| r.path.clone()),
        );
        roots
    }

    /// Drop a mount root that has gone away, promoting a survivor when it was
    /// the active one and unregistering only when it was the LAST one.
    ///
    /// This is the unmount path's entry point, and the reason a share mounted
    /// twice survives an eject of either mount. Pure registry work under the
    /// write lock: teardown (`on_unmount`, stopping an index) belongs to the
    /// caller, which is why [`RootRemoval::Unregistered`] hands the volume back.
    ///
    /// `root` is the native mount path the OS reported (the unmount event's).
    pub fn remove_root(&self, root: &Path) -> RootRemoval {
        let root = &*mount_root_from_os(root);
        let mut volumes = self.volumes.write_ignore_poison();
        let Some((id, entry)) = volumes.iter_mut().find(|(_, entry)| entry.knows_root(root)) else {
            return RootRemoval::Unknown;
        };
        let id = id.clone();

        let was_active = same_root(entry.volume.root(), root);
        entry.roots.retain(|r| !same_root(&r.path, root));

        if !was_active {
            return RootRemoval::SiblingDropped { id };
        }
        match entry.promote_to_best_root() {
            Promotion::Promoted(new_root) => {
                // A survivor took over, but "survivor" is only about ranking: with
                // every sibling already proven stale, the promotion lands on a
                // corpse too.
                entry.tell_volume_if_its_root_is_dead();
                RootRemoval::Promoted { id, new_root }
            }
            Promotion::BackendCantReroot => {
                // The backend stays anchored to a mount that's gone, so the root
                // goes back into the set marked stale. Dropping it would leave the
                // entry claiming no root while `volume.root()` still returns one.
                entry.readd_stale_root(root);
                entry.tell_volume_if_its_root_is_dead();
                RootRemoval::ActiveRootStranded { id }
            }
            // `AlreadyBest` can't follow removing the active root (it's gone from
            // the set), so both remaining arms mean the entry has no roots left.
            Promotion::AlreadyBest | Promotion::NoRootsLeft => {
                let entry = volumes.remove(&id).expect("found under this key a moment ago");
                self.clear_default_if(&id);
                super::retire(&entry.volume);
                RootRemoval::Unregistered {
                    id,
                    volume: entry.volume,
                }
            }
        }
    }

    /// Record that `root` answered with an errno proving its mount is gone, and
    /// move the ID to a live sibling if there is one.
    ///
    /// The lazy half of "liveness outranks path shape": nothing probes a mount to
    /// find out whether it's alive (that blocks for 30–120 s on a wedged one),
    /// so the evidence arrives as a failed operation and the promotion rides on
    /// it. A root already known stale re-reports cheaply and changes nothing.
    pub fn mark_root_stale(&self, id: &str, root: &AppPath) -> StaleRootOutcome {
        let mut volumes = self.volumes.write_ignore_poison();
        let Some(entry) = volumes.get_mut(id) else {
            return StaleRootOutcome::Unchanged;
        };
        let Some(marked) = entry.roots.iter_mut().find(|r| same_root(&r.path, root)) else {
            return StaleRootOutcome::Unchanged;
        };
        marked.proven_stale = true;

        let promotion = entry.promote_to_best_root();
        // Either nothing better existed and the volume is sitting on the root that
        // just proved itself dead, or the best one was dead too.
        entry.tell_volume_if_its_root_is_dead();
        match promotion {
            Promotion::Promoted(new_root) => StaleRootOutcome::Promoted { new_root },
            _ => StaleRootOutcome::Unchanged,
        }
    }

    /// Find the registered non-root volume whose mount root is the longest
    /// ancestor (or equal) of `path`, returning its registry id.
    ///
    /// Used by index read routing to map a `/Volumes/X/…` path to the per-mount
    /// index it belongs to. `root` (`/`) is skipped: it prefixes every path and is
    /// the fallback the router uses when nothing more specific matches. Component-
    /// wise `starts_with` avoids a `/Volumes/XY`-matches-`/Volumes/X` false hit, and
    /// the longest-root wins so a nested mount (`/Volumes/X/Y`) beats its parent.
    ///
    /// A ROUTED volume is not a mount and is skipped: an `ArchiveVolume`'s root
    /// is the `.zip` file and a `GitPortalVolume`'s is `<worktree>/.git`, so
    /// either would win the longest-root race for every path inside it. A path
    /// inside an archive, or inside a repo's snapshots, belongs to the volume
    /// that physically holds it, which is what the callers here route by. The
    /// question is `Volume::routes_over_a_parent`, ❌ never a downcast to a list
    /// of concrete types: a future routed backend that isn't on such a list
    /// silently reintroduces the steal.
    ///
    /// In-memory (one `RwLock<HashMap>` read, no syscall), so it's safe on the
    /// enrichment / dir-stats hot path.
    pub fn mount_id_for_path(&self, path: &str) -> Option<String> {
        let target = Path::new(path);
        self.volumes
            .read_ignore_poison()
            .iter()
            .map(|(id, entry)| (id, &entry.volume))
            // By the volume's own declaration, not by LRU membership: a routed
            // volume is registered a moment before it enters its LRU, and a
            // concurrent caller must not see it in that gap.
            .filter(|(_, v)| !v.routes_over_a_parent())
            // `Path`'s component-wise `starts_with` on the caller's text, `..` and all,
            // until this takes an app path (the tools' `..` resolution decides that).
            .filter_map(|(id, v)| {
                let root = v.root().as_std_legacy();
                (root != Path::new("/") && target.starts_with(&root)).then(|| (id, root.as_os_str().len()))
            })
            .max_by_key(|(_, len)| *len)
            .map(|(id, _)| id.clone())
    }
}

/// What [`Registration::promote_to_best_root`] did.
pub(super) enum Promotion {
    Promoted(AppPathBuf),
    /// The active root is already the best-ranked one.
    AlreadyBest,
    /// A better root exists but the backend won't leave its own.
    BackendCantReroot,
    NoRootsLeft,
}

/// What removing a mount root did to the registry. Every arm names an ID so the
/// caller can log, emit, and (for the last arm) tear the volume down.
pub enum RootRemoval {
    /// No registration claims this root.
    Unknown,
    /// A FALLBACK root went away; the active root and the volume are untouched.
    SiblingDropped { id: String },
    /// The active root went away and a surviving sibling took over.
    Promoted { id: String, new_root: AppPathBuf },
    /// The active root went away, a sibling survives, but the backend can't be
    /// re-rooted, so the registration stays where it is. The volume keeps serving
    /// whoever holds it, which beats unregistering a filesystem that's still
    /// reachable. No shipping backend declines today (`LocalPosixVolume` and
    /// `SmbVolume` both re-root), so this is the safety net for the next one.
    ActiveRootStranded { id: String },
    /// The LAST root went away, so the registration is gone. The caller owns the
    /// teardown (`on_unmount`, index stop).
    Unregistered { id: String, volume: Arc<dyn Volume> },
}

/// What marking a root stale did.
pub enum StaleRootOutcome {
    /// Nothing to do: no such volume, no such root, or nothing better to move to.
    Unchanged,
    /// A live sibling took over the ID.
    Promoted { new_root: AppPathBuf },
}

/// Whether an OS error of this class proves the MOUNT behind a path is gone or
/// wedged, rather than saying something about the file.
///
/// Typed, never message text: these reach us through `VolumeError::IoError`'s
/// `os_class`. The set is the transfer layer's lost connection
/// (`OsErrorClass::is_transport`) without a broken pipe or an aborted or reset
/// connection, plus a down host, plus a stale handle, which is precisely "this
/// handle's filesystem moved out from under you".
pub fn is_stale_mount(class: OsErrorClass) -> bool {
    matches!(
        class,
        OsErrorClass::NotConnected
            | OsErrorClass::TimedOut
            | OsErrorClass::HostDown
            | OsErrorClass::HostUnreachable
            | OsErrorClass::NetworkDown
            | OsErrorClass::NetworkUnreachable
            | OsErrorClass::Stale
    )
}

#[cfg(test)]
#[path = "roots_tests.rs"]
mod tests;

/// The mount-root lookup's own cells; `roots_tests.rs` holds the registration-lifecycle ones.
#[cfg(test)]
#[path = "mount_id_tests.rs"]
mod mount_id_tests;
