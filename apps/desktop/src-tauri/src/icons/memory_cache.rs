//! The in-memory icon cache: the hot tier in front of `disk_cache.rs` and the OS
//! fetch. Bounded keys stay uncapped; the per-path subset is LRU-capped.

use super::{IconVariant, disk_cache, per_path, special_folders};
use crate::ignore_poison::RwLockIgnorePoison;
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, RwLock};

/// Prefix marking per-path icon keys (custom-icon folders, and on Windows the files that
/// carry their own icon). Unlike `dir` / `ext:*` / `file` (an inherently bounded set),
/// `path:` keys grow with the number of distinct places visited, so they're capped by an
/// LRU backstop (see `PATH_KEY_CAP`).
pub(super) use cmdr_fs::icons::own_icons::PATH_KEY_PREFIX;

/// True for the unbounded Tier-C keys (`path:*` custom-icon folders and own-icon files,
/// `pkg:*` package bundles). All share the same lifecycle: LRU-capped in memory, backed
/// by the on-disk cache, never persisted to localStorage on the FE. `.app` icons
/// are per-app and custom-folder icons are per-folder, so both grow with browsing.
fn is_per_path_key(icon_id: &str) -> bool {
    icon_id.starts_with(PATH_KEY_PREFIX) || icon_id.starts_with(per_path::PKG_KEY_PREFIX)
}

/// Backstop LRU cap for the unbounded Tier-C keys (`path:*` + `pkg:*`). Folder and
/// package icons are unbounded in the number of directories/bundles a user can
/// visit; without a cap, a long session browsing thousands of distinct folders
/// would accumulate one base64 WebP data-URL per folder forever (only cleared
/// wholesale on theme/accent change). A few hundred covers any plausible
/// visible/recent working set; the rest evict oldest-first. `dir` / `ext:*` /
/// `file` / `symlink*` / `special:*` keys stay uncapped (inherently bounded).
const PATH_KEY_CAP: usize = 256;

/// In-memory icon cache plus an LRU recency queue for the per-path subset.
///
/// `entries` holds every icon (`dir`, `ext:*`, `path:*`, `pkg:*`, …) keyed by icon
/// id. `path_lru` tracks the insertion/refresh order of *only* the per-path keys
/// (`path:*` + `pkg:*`) so we can evict the oldest when their count exceeds
/// `PATH_KEY_CAP`. Bounded keys never enter `path_lru` and are never evicted by
/// the cap.
struct IconCache {
    entries: HashMap<String, String>,
    /// Per-path keys (`path:*` + `pkg:*`) in least-recently-inserted-first order.
    /// Front = oldest.
    path_lru: VecDeque<String>,
    /// Bumped by every flush, so a fetch that started before one can tell its pixels
    /// are stale (see `insert_if_current`).
    generation: u64,
    /// The display options the cached pixels were drawn under.
    variant: IconVariant,
}

/// What a fetch snapshots before asking macOS: the cache generation (to refuse a
/// late write after a flush) and the variant (to key the on-disk tier).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CacheEpoch {
    generation: u64,
    pub(super) variant: IconVariant,
}

impl IconCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            path_lru: VecDeque::new(),
            generation: 0,
            variant: IconVariant::default(),
        }
    }

    fn epoch(&self) -> CacheEpoch {
        CacheEpoch {
            generation: self.generation,
            variant: self.variant,
        }
    }

    /// Caches an icon fetched under `epoch`, unless a flush happened since. Returns
    /// whether it was cached.
    ///
    /// Gotcha/Why: a fetch can outlive the flush that should have dropped it (a slow
    /// real-folder lookup, or one the 2 s IPC deadline abandoned that keeps running).
    /// Written back blindly, it restores the previous look a few seconds after the
    /// switch, and every later request serves it.
    fn insert_if_current(&mut self, epoch: CacheEpoch, icon_id: String, data_url: String) -> bool {
        if epoch.generation != self.generation {
            return false;
        }
        self.insert(icon_id, data_url);
        true
    }

    /// Switches to the pixels for `variant`, dropping every key if it changed. Returns
    /// whether it flushed.
    fn set_variant(&mut self, variant: IconVariant) -> bool {
        if variant == self.variant {
            return false;
        }
        self.variant = variant;
        self.retain(|_| false);
        true
    }

    /// Inserts or refreshes an icon. For per-path keys (`path:*` + `pkg:*`),
    /// maintains the LRU order and evicts the oldest entries once the cap is
    /// exceeded.
    fn insert(&mut self, icon_id: String, data_url: String) {
        if is_per_path_key(&icon_id) {
            // Refresh recency: drop any existing position, then push to the back.
            if self.entries.contains_key(&icon_id) {
                self.path_lru.retain(|k| k != &icon_id);
            }
            self.path_lru.push_back(icon_id.clone());
            self.entries.insert(icon_id, data_url);
            // Evict oldest `path:` entries beyond the cap.
            while self.path_lru.len() > PATH_KEY_CAP {
                if let Some(evicted) = self.path_lru.pop_front() {
                    self.entries.remove(&evicted);
                }
            }
        } else {
            self.entries.insert(icon_id, data_url);
        }
    }

    /// Keeps only the entries (and their LRU bookkeeping) matching `pred`, and starts
    /// a new generation so fetches already in flight can't write back.
    fn retain(&mut self, pred: impl Fn(&str) -> bool) {
        self.entries.retain(|key, _| pred(key));
        self.path_lru.retain(|key| pred(key));
        self.generation += 1;
    }
}

/// Cache for generated icons (icon_id -> base64 WebP data URL), with an LRU cap on
/// the unbounded `path:` subset.
static ICON_CACHE: LazyLock<RwLock<IconCache>> = LazyLock::new(|| RwLock::new(IconCache::new()));

/// Gets cached icon data URL for the given icon ID, if available.
pub(super) fn get_cached_icon(icon_id: &str) -> Option<String> {
    ICON_CACHE.read_ignore_poison().entries.get(icon_id).cloned()
}

/// Snapshots the cache generation and variant; take it BEFORE asking macOS.
pub(super) fn epoch() -> CacheEpoch {
    ICON_CACHE.read_ignore_poison().epoch()
}

/// Caches an icon fetched under `epoch`, unless the cache was flushed since. Returns
/// whether it was cached, so the caller persists it only then.
pub(super) fn cache_icon(epoch: CacheEpoch, icon_id: String, data_url: String) -> bool {
    ICON_CACHE
        .write_ignore_poison()
        .insert_if_current(epoch, icon_id, data_url)
}

/// Switches the cache to the pixels for `variant`, dropping every cached icon if it
/// changed (file icons too: macOS draws documents differently under Increase contrast).
/// The on-disk tier needs no clear, since its entries are keyed by variant. Returns
/// whether it flushed.
pub(super) fn set_variant(variant: IconVariant) -> bool {
    ICON_CACHE.write_ignore_poison().set_variant(variant)
}

/// Clears all cached icons for extension-based entries.
/// Called when the "use app icons for documents" setting changes.
pub fn clear_extension_icon_cache() {
    // Only remove extension-based icons (ext:xxx), keep directory icons
    ICON_CACHE.write_ignore_poison().retain(|key| !key.starts_with("ext:"));
}

/// Clears all cached icons for directory entries (`dir`, `symlink-dir`,
/// `path:*`, `pkg:*`, `special:*`). Called when the system theme or accent color
/// changes, since macOS folder icons (including the special-folder glyphs) are
/// tinted by the current appearance. Package icons (`.app`, …) carry no folder
/// tint, but dropping them on a theme change is harmless — they re-fetch lazily —
/// and keeps the predicate simple.
pub fn clear_directory_icon_cache() {
    ICON_CACHE.write_ignore_poison().retain(|key| {
        key != "dir"
            && key != "symlink-dir"
            && !is_per_path_key(key)
            && !key.starts_with(special_folders::SPECIAL_KEY_PREFIX)
    });
    // The on-disk warm tier holds the same appearance-tinted `special:*` / `path:*`
    // / `pkg:*` icons. Its mtime token can't catch a theme/accent change (the
    // folder didn't change, the system did), so drop it wholesale too; the icons
    // re-fetch lazily with the new tint.
    disk_cache::clear_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_key(n: usize) -> String {
        format!("path:/folder/{n}")
    }

    #[test]
    fn pkg_keys_share_the_per_path_lru_cap() {
        let mut cache = IconCache::new();
        // Fill the cap entirely with pkg: keys.
        for n in 0..PATH_KEY_CAP {
            cache.insert(format!("pkg:/Applications/App{n}.app"), format!("url-{n}"));
        }
        // One more pkg: key evicts the oldest, keeping the cap.
        cache.insert("pkg:/Applications/Overflow.app".to_string(), "new".to_string());
        assert_eq!(cache.path_lru.len(), PATH_KEY_CAP);
        assert!(!cache.entries.contains_key("pkg:/Applications/App0.app"));
        assert!(cache.entries.contains_key("pkg:/Applications/Overflow.app"));
    }

    #[test]
    fn path_and_pkg_keys_share_one_lru_budget() {
        let mut cache = IconCache::new();
        // Mix path: and pkg: keys up to the cap.
        for n in 0..(PATH_KEY_CAP / 2) {
            cache.insert(path_key(n), format!("p-{n}"));
            cache.insert(format!("pkg:/A/B{n}.app"), format!("k-{n}"));
        }
        // Both kinds count toward the same budget, so total per-path keys == cap.
        let per_path = cache.entries.keys().filter(|k| is_per_path_key(k)).count();
        assert_eq!(per_path, PATH_KEY_CAP);
        assert_eq!(cache.path_lru.len(), PATH_KEY_CAP);
    }

    #[test]
    fn path_keys_respect_lru_cap_and_evict_oldest_first() {
        let mut cache = IconCache::new();

        // Insert one more than the cap.
        for n in 0..=PATH_KEY_CAP {
            cache.insert(path_key(n), format!("url-{n}"));
        }

        // Cap is respected.
        assert_eq!(cache.path_lru.len(), PATH_KEY_CAP);
        let path_entries = cache.entries.keys().filter(|k| k.starts_with(PATH_KEY_PREFIX)).count();
        assert_eq!(path_entries, PATH_KEY_CAP);

        // The very first (oldest) inserted key was evicted.
        assert!(
            !cache.entries.contains_key(&path_key(0)),
            "oldest path: key should evict"
        );
        // The newest key survives.
        assert!(cache.entries.contains_key(&path_key(PATH_KEY_CAP)));
    }

    #[test]
    fn non_path_keys_are_never_evicted_by_the_cap() {
        let mut cache = IconCache::new();

        // Seed a handful of inherently-bounded keys.
        cache.insert("dir".to_string(), "dir-url".to_string());
        cache.insert("symlink-dir".to_string(), "symlink-dir-url".to_string());
        cache.insert("ext:txt".to_string(), "txt-url".to_string());
        cache.insert("file".to_string(), "file-url".to_string());

        // Overflow the path: keys well past the cap.
        for n in 0..(PATH_KEY_CAP * 3) {
            cache.insert(path_key(n), format!("url-{n}"));
        }

        // None of the bounded keys got evicted, and none leaked into the LRU queue.
        assert_eq!(cache.entries.get("dir").map(String::as_str), Some("dir-url"));
        assert_eq!(
            cache.entries.get("symlink-dir").map(String::as_str),
            Some("symlink-dir-url")
        );
        assert_eq!(cache.entries.get("ext:txt").map(String::as_str), Some("txt-url"));
        assert_eq!(cache.entries.get("file").map(String::as_str), Some("file-url"));
        assert_eq!(cache.path_lru.len(), PATH_KEY_CAP);
    }

    #[test]
    fn reinserting_a_path_key_refreshes_its_recency() {
        let mut cache = IconCache::new();

        // Fill exactly to the cap.
        for n in 0..PATH_KEY_CAP {
            cache.insert(path_key(n), format!("url-{n}"));
        }

        // Touch the oldest key again — it should move to the back (most recent).
        cache.insert(path_key(0), "refreshed".to_string());

        // Insert a new key, forcing one eviction. The refreshed key must survive;
        // the now-oldest (key 1) should be the one evicted.
        cache.insert(path_key(PATH_KEY_CAP), "new".to_string());

        assert_eq!(cache.path_lru.len(), PATH_KEY_CAP);
        assert_eq!(
            cache.entries.get(&path_key(0)).map(String::as_str),
            Some("refreshed"),
            "refreshed key should survive eviction"
        );
        assert!(
            !cache.entries.contains_key(&path_key(1)),
            "the now-oldest key should be evicted"
        );
        // No duplicate LRU entries after the refresh.
        let occurrences = cache.path_lru.iter().filter(|k| **k == path_key(0)).count();
        assert_eq!(occurrences, 1, "refreshed key must appear exactly once in the LRU");
    }

    /// Regression anchor: a fetch that started before a flush must not land after it. On a
    /// macOS Increase contrast toggle, the accent-change flush's refetch was still in flight
    /// when the display-options flush ran, finished late, and put the previous look's pixels
    /// back over the fresh ones a few seconds after the switch.
    #[test]
    fn a_fetch_started_before_a_flush_is_not_cached_after_it() {
        let mut cache = IconCache::new();
        let before = cache.epoch();

        cache.retain(|key| key != "dir");

        assert!(!cache.insert_if_current(before, "dir".to_string(), "stale".to_string()));
        assert!(!cache.entries.contains_key("dir"));

        let after = cache.epoch();
        assert!(cache.insert_if_current(after, "dir".to_string(), "fresh".to_string()));
        assert_eq!(cache.entries.get("dir").map(String::as_str), Some("fresh"));
    }

    #[test]
    fn a_new_icon_variant_drops_every_key_and_invalidates_in_flight_fetches() {
        let mut cache = IconCache::new();
        let before = cache.epoch();
        for key in ["dir", "ext:txt", "file", "special:downloads", "pkg:/A.app"] {
            cache.insert(key.to_string(), "standard".to_string());
        }
        let contrast = IconVariant {
            increase_contrast: true,
            invert_colors: false,
        };

        assert!(cache.set_variant(contrast), "a changed variant flushes");

        assert!(cache.entries.is_empty(), "file icons change too, not just folders");
        assert!(cache.path_lru.is_empty());
        assert_eq!(cache.epoch().variant, contrast);
        assert!(!cache.insert_if_current(before, "dir".to_string(), "stale".to_string()));
    }

    #[test]
    fn the_same_icon_variant_again_keeps_the_cache() {
        let mut cache = IconCache::new();
        cache.insert("dir".to_string(), "url".to_string());
        let before = cache.epoch();

        assert!(!cache.set_variant(IconVariant::default()));

        assert!(cache.entries.contains_key("dir"));
        assert!(cache.insert_if_current(before, "file".to_string(), "url".to_string()));
    }

    #[test]
    fn retain_drops_path_lru_bookkeeping_too() {
        let mut cache = IconCache::new();
        cache.insert("dir".to_string(), "dir-url".to_string());
        for n in 0..10 {
            cache.insert(path_key(n), format!("url-{n}"));
        }

        // Mirror `clear_directory_icon_cache`: drop dir + path: keys.
        cache.retain(|key| key != "dir" && !key.starts_with(PATH_KEY_PREFIX));

        assert!(cache.entries.is_empty());
        assert!(
            cache.path_lru.is_empty(),
            "path_lru must not retain keys removed from entries"
        );
    }
}
