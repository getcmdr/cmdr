//! Caching structures for MTP path resolution and directory listings,
//! plus event debouncing for directory change notifications.

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use mtp_rs::ObjectHandle;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::RwLockIgnorePoison;

use super::twins;

/// Cache mapping virtual paths to MTP object handles, and back.
///
/// Both directions are populated together (see [`insert`](Self::insert)) at the
/// same sites that list a directory, so they never drift. The forward map
/// (`path_to_handle`) backs [`resolve_path_to_handle`](super::MtpConnectionManager::resolve_path_to_handle)
/// for browsing; the reverse map (`handle_to_path`) lets the pathless PTP change
/// events ([`DeviceEvent::ObjectAdded`](mtp_rs::mtp::DeviceEvent) and friends, which carry only an
/// opaque handle) short-circuit a parent-walk the moment they hit a cached
/// ancestor instead of always walking to the storage root over USB.
///
/// A third map, `children`, indexes the forward entries by parent, so a fresh
/// listing can drop what its folder no longer holds
/// ([`record_listing`](Self::record_listing)) and a removed folder takes its
/// subtree with it, without a scan of every cached path.
///
/// ❗ PTP lets a folder hold two siblings with the same name. Each twin's row
/// carries its own path (`twins.rs`), cached like any other; the name they
/// share gets NO forward entry: it's `same_named`, and [`lookup`](Self::lookup)
/// answers [`Cached::SameNamed`] so a path-only op refuses it instead of
/// picking a twin (`connection/DETAILS.md` § "Same-named siblings"). Children
/// are indexed under [`twins::folder_of`], so a twin's row sits in its real
/// folder.
#[derive(Default)]
pub(super) struct PathHandleCache {
    /// Maps virtual path -> MTP object handle.
    pub(super) path_to_handle: HashMap<AppPathBuf, ObjectHandle>,
    /// Maps MTP object handle -> virtual path. The reverse of `path_to_handle`,
    /// kept in lockstep with it. A same-named sibling's handle maps to the path
    /// it shares, so a change event for it still refreshes the right folder.
    pub(super) handle_to_path: HashMap<ObjectHandle, AppPathBuf>,
    /// Maps a folder's path -> the cached paths directly inside it.
    children: HashMap<AppPathBuf, HashSet<AppPathBuf>>,
    /// Maps a name more than one sibling answered to in its folder's last
    /// listing (the path they share) -> their handles.
    same_named: HashMap<AppPathBuf, Vec<ObjectHandle>>,
}

/// What the path cache knows about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cached {
    /// The one object at the path.
    Handle(ObjectHandle),
    /// More than one sibling answers to the path, so it names no one object.
    SameNamed,
}

impl PathHandleCache {
    /// What the cache holds for `path`, if anything.
    ///
    /// A twin's row path whose twin has since gone (renamed, deleted) still
    /// reaches its object while the name it shared now resolves to that very
    /// handle, so a row a pane hasn't re-read yet keeps working.
    pub(super) fn lookup(&self, path: &AppPath) -> Option<Cached> {
        if self.same_named.contains_key(path) {
            return Some(Cached::SameNamed);
        }
        if let Some(handle) = self.path_to_handle.get(path) {
            return Some(Cached::Handle(*handle));
        }
        let pick = twins::pick_in(path)?;
        (self.path_to_handle.get(&twins::shared_path(path)) == Some(&pick)).then_some(Cached::Handle(pick))
    }

    /// Records a `(path, handle)` pair in both directions.
    ///
    /// Always insert through this (never `path_to_handle.insert` directly) so the
    /// reverse map can't fall out of sync. A path that held a DIFFERENT handle,
    /// or several, was another object, so what's cached under it goes: those
    /// children were that object's.
    pub(super) fn insert(&mut self, path: AppPathBuf, handle: ObjectHandle) {
        if self.same_named.contains_key(&path) || self.path_to_handle.get(&path).is_some_and(|old| *old != handle) {
            self.remove_path(&path);
        }
        if let Some(parent) = twins::folder_of(&path) {
            self.children.entry(parent.to_owned()).or_default().insert(path.clone());
        }
        self.path_to_handle.insert(path.clone(), handle);
        self.handle_to_path.insert(handle, path);
    }

    /// Records what a handle walk found (`handle_resolver.rs`): `handle` sits at `path`.
    ///
    /// A walk spells a twin by the name it shares, so it may only teach a path the
    /// cache doesn't yet tie to another object. ❗ When the path names a same-named
    /// pair, or another handle, two objects now answer to it: the path's entry goes
    /// (the next op re-lists and finds the twins), ❌ never pointing at this handle,
    /// or a row meaning the first object would act on this one (#523).
    pub(super) fn learn_resolved(&mut self, path: AppPathBuf, handle: ObjectHandle) {
        match self.lookup(&path) {
            None => self.insert(path, handle),
            Some(Cached::Handle(cached)) if cached == handle => {}
            Some(Cached::Handle(_)) => self.remove_path(&path),
            Some(Cached::SameNamed) => {}
        }
    }

    /// Forgets `path` in both directions, and everything cached under it.
    ///
    /// Always remove through this (never `path_to_handle.remove` directly).
    /// Dropping only the forward entry leaves the reverse map claiming a path for
    /// a handle that no longer holds it, so an object that inherits the handle
    /// would resolve to the removed object's path. The subtree goes too: a folder
    /// that's gone or renamed no longer holds what was cached under its old path,
    /// and a new folder taking the name would otherwise resolve its children to
    /// the old handles.
    pub(super) fn remove_path(&mut self, path: &AppPath) {
        if let Some(handle) = self.path_to_handle.remove(path) {
            // Only if the reverse entry still points AT this path: a rename
            // re-inserts the handle under its new path, and that newer mapping
            // must survive a later removal of the stale forward entry.
            if self.handle_to_path.get(&handle).is_some_and(|p| p == path) {
                self.handle_to_path.remove(&handle);
            }
        }
        for twin in self.same_named.remove(path).unwrap_or_default() {
            if self.handle_to_path.get(&twin).is_some_and(|p| p == path) {
                self.handle_to_path.remove(&twin);
            }
        }
        if let Some(parent) = twins::folder_of(path)
            && let Some(siblings) = self.children.get_mut(parent)
        {
            siblings.remove(path);
        }
        for child in self.children.remove(path).unwrap_or_default() {
            self.remove_path(&child);
        }
    }

    /// Records a complete listing of `parent`: every child it names, and the
    /// end of every cached child it doesn't (with that child's subtree), so a
    /// name the folder no longer holds can't resolve to a handle the device may
    /// since have given to another object. A name more than one child answers
    /// to is recorded as same-named, resolving to none of them; a twin's own row
    /// path (`twins.rs`) resolves to its object, and marks the name it shares.
    pub(super) fn record_listing(&mut self, parent: &AppPath, listed: Vec<(AppPathBuf, ObjectHandle)>) {
        let fresh: HashSet<&AppPath> = listed.iter().map(|(path, _)| path.as_app_path()).collect();
        let gone: Vec<AppPathBuf> = self
            .children
            .get(parent)
            .map(|cached| {
                cached
                    .iter()
                    .filter(|p| !fresh.contains(p.as_app_path()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for path in gone {
            self.remove_path(&path);
        }
        let mut by_path: HashMap<AppPathBuf, Vec<ObjectHandle>> = HashMap::with_capacity(listed.len());
        let mut twins_by_name: HashMap<AppPathBuf, Vec<ObjectHandle>> = HashMap::new();
        for (path, handle) in listed {
            if twins::pick_in(&path).is_some() {
                twins_by_name.entry(twins::shared_path(&path)).or_default().push(handle);
            }
            by_path.entry(path).or_default().push(handle);
        }
        for (path, handles) in by_path {
            match handles[..] {
                [handle] => self.insert(path, handle),
                _ => self.record_same_named(path, handles),
            }
        }
        for (shared, handles) in twins_by_name {
            self.mark_shared(shared, handles);
        }
    }

    /// Records `shared` as the name `handles` share, each already cached under
    /// its own row path: no forward entry, nothing cached beneath it, and the
    /// twins' reverse entries left pointing at their rows.
    fn mark_shared(&mut self, shared: AppPathBuf, handles: Vec<ObjectHandle>) {
        self.remove_path(&shared);
        if let Some(parent) = twins::folder_of(&shared) {
            self.children
                .entry(parent.to_owned())
                .or_default()
                .insert(shared.clone());
        }
        self.same_named.insert(shared, handles);
    }

    /// Records `path` as a name all of `handles` answer to: no forward entry,
    /// a reverse entry per twin, and nothing cached beneath it.
    fn record_same_named(&mut self, path: AppPathBuf, handles: Vec<ObjectHandle>) {
        self.remove_path(&path);
        if let Some(parent) = twins::folder_of(&path) {
            self.children.entry(parent.to_owned()).or_default().insert(path.clone());
        }
        for handle in &handles {
            self.handle_to_path.insert(*handle, path.clone());
        }
        self.same_named.insert(path, handles);
    }

    /// Forgets the object `handle` named, for a removal the device reported.
    /// A path that has since moved on to another handle is left alone.
    pub(super) fn forget_handle(&mut self, handle: ObjectHandle) {
        let Some(path) = self.handle_to_path.get(&handle).cloned() else {
            return;
        };
        if self.path_to_handle.get(&path) == Some(&handle) {
            self.remove_path(&path);
        } else {
            self.handle_to_path.remove(&handle);
        }
    }
}

/// Cache for directory listings.
#[derive(Default)]
pub(super) struct ListingCache {
    /// Maps directory path -> cached file entries.
    pub(super) listings: HashMap<AppPathBuf, CachedListing>,
}

/// A cached directory listing with timestamp for invalidation.
pub(super) struct CachedListing {
    /// The cached file entries.
    pub(super) entries: Vec<FileEntry>,
    /// When this listing was cached (for TTL checks).
    pub(super) cached_at: Instant,
}

/// How long to keep cached listings (5 seconds).
pub(super) const LISTING_CACHE_TTL_SECS: u64 = 5;

/// Debounce duration for MTP directory change events (500ms).
/// MTP devices can emit rapid events during bulk operations (like copying many files).
pub(super) const EVENT_DEBOUNCE_MS: u64 = 500;

/// Debouncer for MTP directory change events.
///
/// Prevents flooding the frontend with events during rapid operations like
/// bulk copy/delete. Each device has its own last-emit timestamp.
pub(super) struct EventDebouncer {
    /// Last emit time per device ID.
    last_emit: RwLock<HashMap<String, Instant>>,
    /// Devices with a trailing re-emit already scheduled, so a burst coalesces
    /// to one pending emit instead of one per suppressed event.
    trailing_scheduled: RwLock<HashSet<String>>,
    /// Debounce duration.
    debounce_duration: Duration,
}

impl EventDebouncer {
    /// Creates a new debouncer with the given duration.
    pub(super) fn new(debounce_duration: Duration) -> Self {
        Self {
            last_emit: RwLock::new(HashMap::new()),
            trailing_scheduled: RwLock::new(HashSet::new()),
            debounce_duration,
        }
    }

    /// Checks if we should emit an event for the given device.
    /// Updates the last emit time if we should emit.
    pub(super) fn should_emit(&self, device_id: &str) -> bool {
        let now = Instant::now();
        let mut last_emit = self.last_emit.write_ignore_poison();

        if let Some(last) = last_emit.get(device_id)
            && now.duration_since(*last) < self.debounce_duration
        {
            return false;
        }

        last_emit.insert(device_id.to_string(), now);
        true
    }

    /// Claims the right to schedule the trailing re-emit for `key`, so a burst
    /// gets ONE trailing emit rather than one per suppressed event.
    ///
    /// `key` is the coalescing scope, not necessarily a device id: the targeted
    /// refresh keys by device AND affected dir, so a pending emit for one folder
    /// can't swallow another folder's. [`clear`](Self::clear) drops every key
    /// belonging to a device.
    ///
    /// Returns `true` for the first caller after each [`release_trailing`](Self::release_trailing), and
    /// `false` while a trailing emit is already pending.
    ///
    /// **Without the claim this is a livelock, not just waste.** Each suppressed
    /// event used to spawn its own sleeping task that re-entered
    /// `emit_directory_changed`, where all but one were suppressed again and
    /// spawned yet another. The population then retires one event per debounce
    /// window instead of collapsing, so a burst of N changes on the device (a
    /// bulk copy onto a phone, a camera burst, Android's media scanner) pegs a
    /// core and starves foreground listings for N × the window. Measured: 48k
    /// events left the app at 100% CPU with pane listings unserved, not
    /// recovering.
    pub(super) fn claim_trailing(&self, key: &str) -> bool {
        // `insert` returns false when the key was already present, which is
        // exactly "someone else already scheduled this one".
        self.trailing_scheduled.write_ignore_poison().insert(key.to_string())
    }

    /// Releases the trailing-emit claim, letting the next burst schedule one.
    /// Call this when the trailing task wakes, BEFORE re-emitting, so an event
    /// arriving during the re-emit can still claim the following window.
    pub(super) fn release_trailing(&self, key: &str) {
        let mut scheduled = self.trailing_scheduled.write_ignore_poison();
        scheduled.remove(key);
    }

    /// The coalescing key for a targeted (per-folder) refresh. A NUL can't occur
    /// in a device id or a path, so the two parts can't run together.
    pub(super) fn targeted_key(device_id: &str, affected_dir: &AppPath) -> String {
        format!("{device_id}\0{}", affected_dir.as_str())
    }

    /// Clears the debounce state for a device (called on disconnect).
    pub(super) fn clear(&self, device_id: &str) {
        let mut last_emit = self.last_emit.write_ignore_poison();
        last_emit.remove(device_id);
        // Drop the device's own claim plus every per-folder one, else a stale
        // claim survives the disconnect and blocks trailing emits after the
        // next connect.
        let targeted_prefix = format!("{device_id}\0");
        self.trailing_scheduled
            .write_ignore_poison()
            .retain(|key| key != device_id && !key.starts_with(&targeted_prefix));
    }
}

#[cfg(test)]
mod path_handle_cache_tests {
    use cmdr_fs::app_path::AppPathBuf;
    use mtp_rs::ObjectHandle;

    use super::super::storage_path::{storage_path, storage_root};
    use super::{Cached, PathHandleCache};

    fn path(text: &str) -> AppPathBuf {
        storage_path(text).expect("a storage path")
    }

    fn at(text: &str, handle: u64) -> (AppPathBuf, ObjectHandle) {
        (path(text), ObjectHandle(handle))
    }

    fn handle_of(cache: &PathHandleCache, text: &str) -> Option<u64> {
        cache.path_to_handle.get(path(text).as_app_path()).map(|h| h.0)
    }

    /// ❗ Regression anchor (#524): a name the folder no longer holds kept its
    /// handle for the whole session, so a stale row resolved to whatever object
    /// the device gave that handle next.
    #[test]
    fn a_fresh_listing_forgets_the_children_it_no_longer_names_and_their_subtrees() {
        let mut cache = PathHandleCache::default();
        cache.record_listing(storage_root(), vec![at("/DCIM", 1), at("/gone.txt", 2)]);
        cache.record_listing(&path("/DCIM"), vec![at("/DCIM/a.jpg", 3)]);

        cache.record_listing(storage_root(), vec![at("/Music", 4)]);

        assert_eq!(handle_of(&cache, "/gone.txt"), None);
        assert_eq!(handle_of(&cache, "/DCIM"), None);
        assert_eq!(
            handle_of(&cache, "/DCIM/a.jpg"),
            None,
            "a vanished folder takes its subtree"
        );
        assert_eq!(handle_of(&cache, "/Music"), Some(4));
        assert!(!cache.handle_to_path.contains_key(&ObjectHandle(3)));
    }

    #[test]
    fn a_folder_listed_under_a_new_handle_drops_what_was_cached_under_the_old_one() {
        let mut cache = PathHandleCache::default();
        cache.record_listing(storage_root(), vec![at("/DCIM", 1)]);
        cache.record_listing(&path("/DCIM"), vec![at("/DCIM/a.jpg", 3)]);

        cache.record_listing(storage_root(), vec![at("/DCIM", 9)]);

        assert_eq!(handle_of(&cache, "/DCIM"), Some(9));
        assert_eq!(
            handle_of(&cache, "/DCIM/a.jpg"),
            None,
            "those children were the old folder's"
        );
    }

    /// ❗ Regression anchor (#523): the last twin listed won the path, so a
    /// delete of row one took row two's file.
    #[test]
    fn same_named_siblings_resolve_to_neither_until_a_listing_names_one() {
        let mut cache = PathHandleCache::default();
        cache.record_listing(storage_root(), vec![at("/IMG_1.JPG", 1), at("/IMG_1.JPG", 2)]);

        assert_eq!(cache.lookup(&path("/IMG_1.JPG")), Some(Cached::SameNamed));
        assert_eq!(
            cache.handle_to_path.get(&ObjectHandle(1)).map(AppPathBuf::as_app_path),
            Some(path("/IMG_1.JPG").as_app_path()),
            "an event for either twin still finds its folder"
        );

        cache.record_listing(storage_root(), vec![at("/IMG_1.JPG", 2)]);
        assert_eq!(cache.lookup(&path("/IMG_1.JPG")), Some(Cached::Handle(ObjectHandle(2))));
        assert!(!cache.handle_to_path.contains_key(&ObjectHandle(1)));

        cache.record_listing(storage_root(), vec![at("/IMG_1.JPG", 3), at("/IMG_1.JPG", 4)]);
        cache.record_listing(storage_root(), vec![]);
        assert_eq!(
            cache.lookup(&path("/IMG_1.JPG")),
            None,
            "a listing without the name ends it"
        );
        assert!(!cache.handle_to_path.contains_key(&ObjectHandle(3)));
    }

    /// ❗ Regression anchor (#523): a handle walk spells a twin by the name it shares,
    /// so what it learns never makes that name reach one object: not over a cached
    /// pair, and not over the one object a row of that name means (a twin appearing
    /// beside it, which an `ObjectAdded` resolves before any listing sees two).
    #[test]
    fn a_handle_walk_never_ties_a_shared_name_to_one_twin() {
        let mut cache = PathHandleCache::default();
        cache.record_listing(storage_root(), vec![at("/IMG_1.JPG", 1), at("/IMG_1.JPG", 2)]);
        cache.learn_resolved(path("/IMG_1.JPG"), ObjectHandle(2));
        assert_eq!(cache.lookup(&path("/IMG_1.JPG")), Some(Cached::SameNamed));

        let mut cache = PathHandleCache::default();
        cache.record_listing(storage_root(), vec![at("/a.txt", 1)]);
        cache.learn_resolved(path("/a.txt"), ObjectHandle(7));
        assert_eq!(
            cache.lookup(&path("/a.txt")),
            None,
            "two objects answer to the name now: the next op re-lists, ❌ never reaching the newcomer"
        );

        let mut cache = PathHandleCache::default();
        cache.learn_resolved(path("/new.txt"), ObjectHandle(3));
        assert_eq!(cache.lookup(&path("/new.txt")), Some(Cached::Handle(ObjectHandle(3))));
        cache.learn_resolved(path("/new.txt"), ObjectHandle(3));
        assert_eq!(cache.lookup(&path("/new.txt")), Some(Cached::Handle(ObjectHandle(3))));
    }

    /// ❗ Regression anchor (#523): each twin's row reaches its own object, and
    /// the name they share, given alone, reaches neither.
    #[test]
    fn each_twin_row_resolves_to_its_own_object_and_the_shared_name_to_neither() {
        let row = |handle| {
            super::super::twins::picked(storage_root(), ObjectHandle(handle), "IMG_1.JPG").expect("a twin row")
        };
        let mut cache = PathHandleCache::default();
        cache.record_listing(
            storage_root(),
            vec![(row(1), ObjectHandle(1)), (row(2), ObjectHandle(2))],
        );

        assert_eq!(cache.lookup(&row(1)), Some(Cached::Handle(ObjectHandle(1))));
        assert_eq!(cache.lookup(&row(2)), Some(Cached::Handle(ObjectHandle(2))));
        assert_eq!(cache.lookup(&path("/IMG_1.JPG")), Some(Cached::SameNamed));
        assert_eq!(
            cache.handle_to_path.get(&ObjectHandle(1)),
            Some(&row(1)),
            "an event for a twin finds its own row"
        );

        // Twin one is renamed away: row two still reaches its object, which the
        // name alone now names too, and row one reaches nothing.
        cache.record_listing(storage_root(), vec![at("/IMG_1.JPG", 2), at("/IMG_2.JPG", 1)]);
        assert_eq!(cache.lookup(&row(2)), Some(Cached::Handle(ObjectHandle(2))));
        assert_eq!(cache.lookup(&row(1)), None);
        assert_eq!(cache.lookup(&path("/IMG_1.JPG")), Some(Cached::Handle(ObjectHandle(2))));
    }

    #[test]
    fn a_removed_handle_stops_resolving_its_path() {
        let mut cache = PathHandleCache::default();
        cache.record_listing(storage_root(), vec![at("/a.txt", 1), at("/b.txt", 2)]);

        cache.forget_handle(ObjectHandle(1));

        assert_eq!(handle_of(&cache, "/a.txt"), None);
        assert_eq!(handle_of(&cache, "/b.txt"), Some(2));
        assert!(!cache.handle_to_path.contains_key(&ObjectHandle(1)));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::EventDebouncer;

    #[test]
    fn test_event_debouncer_allows_first_event() {
        let debouncer = EventDebouncer::new(Duration::from_millis(500));

        // First event for a device should always be allowed
        assert!(debouncer.should_emit("device-1"));

        // First event for a different device should also be allowed
        assert!(debouncer.should_emit("device-2"));
    }

    #[test]
    fn test_event_debouncer_throttles_rapid_events() {
        let debouncer = EventDebouncer::new(Duration::from_millis(100));

        // First event should be allowed
        assert!(debouncer.should_emit("device-1"));

        // Immediate second event should be throttled
        assert!(!debouncer.should_emit("device-1"));

        // Third rapid event should also be throttled
        assert!(!debouncer.should_emit("device-1"));
    }

    #[test]
    fn test_event_debouncer_allows_after_timeout() {
        let debouncer = EventDebouncer::new(Duration::from_millis(10));

        // First event should be allowed
        assert!(debouncer.should_emit("device-1"));

        // allowed-test-sleep: outliving the 10 ms debounce window is the subject; `should_emit`
        // compares against the wall clock, so only real elapsed time reopens it
        std::thread::sleep(Duration::from_millis(20));

        // Event after timeout should be allowed
        assert!(debouncer.should_emit("device-1"));
    }

    #[test]
    fn test_event_debouncer_clear() {
        let debouncer = EventDebouncer::new(Duration::from_millis(500));

        // First event allowed
        assert!(debouncer.should_emit("device-1"));

        // Second event should be throttled
        assert!(!debouncer.should_emit("device-1"));

        // Clear the device state
        debouncer.clear("device-1");

        // After clear, next event should be allowed immediately
        assert!(debouncer.should_emit("device-1"));
    }

    #[test]
    fn test_event_debouncer_per_device_isolation() {
        let debouncer = EventDebouncer::new(Duration::from_millis(500));

        // First event for device-1
        assert!(debouncer.should_emit("device-1"));

        // Rapid event for device-1 should be throttled
        assert!(!debouncer.should_emit("device-1"));

        // But event for device-2 should be allowed (independent)
        assert!(debouncer.should_emit("device-2"));

        // And rapid event for device-2 should be throttled independently
        assert!(!debouncer.should_emit("device-2"));
    }

    /// A burst must coalesce to ONE pending trailing emit.
    ///
    /// Pre-fix this would have passed wrongly: every suppressed event claimed
    /// its own trailing emit, and since each of those re-enters the debouncer
    /// and re-claims, the population retires one event per window instead of
    /// collapsing. A 1,000-file copy onto a phone then keeps a core busy for
    /// ~500 × 1,000 ms while foreground listings go unserved.
    #[test]
    fn a_burst_claims_only_one_trailing_emit() {
        let debouncer = EventDebouncer::new(Duration::from_millis(500));
        assert!(debouncer.should_emit("device-1"), "first event does the real emit");

        let claims = (0..100)
            .filter(|_| {
                assert!(!debouncer.should_emit("device-1"), "burst events are suppressed");
                debouncer.claim_trailing("device-1")
            })
            .count();

        assert_eq!(claims, 1, "a burst must schedule one trailing emit, not one per event");
    }

    #[test]
    fn releasing_the_claim_lets_the_next_burst_schedule_one() {
        let debouncer = EventDebouncer::new(Duration::from_millis(500));
        assert!(debouncer.claim_trailing("device-1"));
        assert!(!debouncer.claim_trailing("device-1"), "still pending");

        debouncer.release_trailing("device-1");
        assert!(debouncer.claim_trailing("device-1"), "the next burst gets its own emit");
    }

    #[test]
    fn trailing_claims_are_per_device() {
        let debouncer = EventDebouncer::new(Duration::from_millis(500));
        assert!(debouncer.claim_trailing("device-1"));
        assert!(
            debouncer.claim_trailing("device-2"),
            "one device's pending emit must not suppress another's"
        );
    }

    #[test]
    fn clear_drops_a_pending_trailing_claim() {
        // Disconnect wipes the device's state; a stale claim would otherwise
        // block every trailing emit after the next connect.
        let debouncer = EventDebouncer::new(Duration::from_millis(500));
        assert!(debouncer.claim_trailing("device-1"));

        debouncer.clear("device-1");
        assert!(debouncer.claim_trailing("device-1"));
    }
}
