//! Full re-reads of a directory: the per-directory turnstile that bounds a storm
//! of `FullRefresh` changes to two reads, the re-read itself, and the archive
//! listings a rewritten archive re-reads.

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use cmdr_fs::ignore_poison::RwLockIgnorePoison;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;

use crate::Inner;
use crate::pipeline::{Prepared, ReadBy};
use crate::reread::ReadGeneration;

/// One directory's refresh turnstile: an async lock plus a flag saying whether a
/// refresh is already queued behind the running one.
pub(crate) struct RefreshSlot {
    lock: Arc<tokio::sync::Mutex<()>>,
    pub(super) queued: Arc<std::sync::atomic::AtomicBool>,
}

impl Inner {
    /// Dispatches a `FullRefresh` re-read onto the host's runtime.
    ///
    /// Every `FullRefresh` producer (the notify-rs debouncer, the git watcher, the SMB
    /// and MTP watcher threads) runs on a plain OS thread with no Tokio runtime in
    /// thread-local context, so a bare `tokio::spawn` panics with "there is no reactor
    /// running" and takes the app down. The engine's own runtime handle works from
    /// any thread instead of inheriting a thread-local one.
    /// Same rule as `file_system::watcher` and `cmdr_archive::watch`.
    pub(crate) fn spawn_full_refresh(
        self: &Arc<Self>,
        volume_id: String,
        parent_path: AppPathBuf,
        listings: Vec<String>,
    ) {
        // A strong handle: the refresh ends.
        let engine = Arc::clone(self);
        self.runtime
            .spawn(async move { engine.notify_full_refresh(volume_id, parent_path, listings).await });
    }

    /// Hands out (creating on first use) the turnstile for one directory.
    fn refresh_slot(&self, volume_id: &str, parent_path: &AppPath) -> RefreshSlot {
        let key = (volume_id.to_string(), parent_path.to_owned());
        if let Some(slot) = self.refresh_slots.read_ignore_poison().get(&key) {
            return RefreshSlot {
                lock: slot.lock.clone(),
                queued: slot.queued.clone(),
            };
        }
        let mut slots = self.refresh_slots.write_ignore_poison();
        // Drop turnstiles nobody holds any more. A slot is one `String` + `AppPathBuf` + two
        // `Arc`s, but there is one per directory that ever saw a refresh, so without this a
        // long session browsing a big tree accumulates them for directories it left hours
        // ago. `strong_count == 1` means this map holds the only reference: no refresh is
        // running or queued there, so removing it can't strand a waiter.
        slots.retain(|_, slot| Arc::strong_count(&slot.lock) > 1);
        let slot = slots.entry(key).or_insert_with(|| RefreshSlot {
            lock: Arc::new(tokio::sync::Mutex::new(())),
            queued: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        RefreshSlot {
            lock: slot.lock.clone(),
            queued: slot.queued.clone(),
        }
    }

    /// Holds `parent_path`'s refresh turn while the guard lives, for a re-read that
    /// isn't a watcher's `FullRefresh` (a backend swap's respell) but must not
    /// interleave with one either.
    pub(crate) async fn refresh_turn(
        &self,
        volume_id: &str,
        parent_path: &AppPath,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.refresh_slot(volume_id, parent_path).lock.lock_owned().await
    }

    /// Re-reads a directory via the Volume trait, computes a diff, and queues it.
    ///
    /// ❗ Refreshes of the SAME directory are serialized, and read-then-write is atomic
    /// against each other because of it. Without that, a burst of writes fires several
    /// refreshes at once and each does its own read before replacing the cached listing
    /// wholesale, so a read that STARTED earlier can LAND later and reinstate a directory
    /// state that has already been superseded. Nothing re-reads afterwards, so the pane
    /// keeps showing the older truth until an unrelated event corrects it: files that are
    /// on disk read as missing for as long as the folder stays quiet. That is reachable by
    /// any heavy external burst — an unzip, a `git checkout`, an rsync into a watched
    /// folder — and it cost an E2E flake before it was understood
    /// (`a_slow_refresh_cannot_overwrite_the_listing_a_newer_one_already_wrote`).
    ///
    /// At most one refresh runs and one waits per directory: a third arrival returns
    /// immediately, because the one already queued will start its read after the running
    /// one finishes and so cannot answer with anything staler. That bounds a storm to two
    /// reads instead of one per event, which is also why this is cheaper than what it
    /// replaces, not just safer.
    ///
    /// `pub(super)` so a test can await one directly rather than through the spawn.
    pub(crate) async fn notify_full_refresh(&self, volume_id: String, parent_path: AppPathBuf, listings: Vec<String>) {
        let slot = self.refresh_slot(&volume_id, &parent_path);
        // Claim the single waiting berth. Losing the race means somebody is already queued
        // to read AFTER the current refresh, which covers this request too.
        if slot.queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let _turn = slot.lock.lock().await;
        // The berth is free again the moment this refresh OWNS the turn: its own read is
        // still ahead, so a request arriving now must be allowed to queue behind it.
        slot.queued.store(false, Ordering::Release);
        self.notify_full_refresh_locked(volume_id, parent_path, listings).await;
    }

    /// Makes `entries` the contents of `listing_id` and queues what changed for the
    /// next coalesced flush.
    ///
    /// Sorts the way that listing sorts BEFORE diffing, which is the whole reason a
    /// backend hands entries over instead of patching the cache itself: a protocol
    /// answers in its own order (MTP by object handle), and a diff computed against
    /// that order carries indices that point at the wrong rows in a pane sorted any
    /// other way.
    ///
    /// The entries arrive [`Prepared`]: enriched, so the rows the frontend
    /// receives carry the recursive sizes the cache is about to hold, and
    /// decorated, with the count of contributed rows the listing has to keep: a
    /// `.git/` listing that regained its six virtual rows in this refresh must not
    /// read as authoritative to a walker afterwards (`host/overlays.rs`). Callers
    /// prepare ONCE per directory rather than once per listing. Dropped when the
    /// listing already published a re-read newer than `generation`.
    pub(crate) fn publish_replacement(&self, listing_id: &str, prepared: Prepared, generation: ReadGeneration) {
        self.update_listing_entries(listing_id, prepared, generation);
    }

    /// The body of one refresh, with the directory's turn already held.
    async fn notify_full_refresh_locked(&self, volume_id: String, parent_path: AppPathBuf, listings: Vec<String>) {
        // Re-resolve from `(volume_id, parent_path)` so a `.zip`-crossing listing hits
        // the same `ArchiveVolume` the read used (the cache keys on the parent drive
        // id, and a re-resolve re-registers a lazily-evicted archive).
        let resolved = self.volumes.resolve(&volume_id, &parent_path).await;
        let is_routed = resolved.is_routed();
        let vol = match resolved.volume {
            Some(v) => v,
            None => {
                log::warn!("notify_directory_changed: volume `{}` not found", volume_id);
                return;
            }
        };

        // After the turn, right before the read (`reread.rs`).
        let generation = self.next_read_generation();
        let new_entries = match vol.list_directory(&parent_path, None).await {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!(
                    "notify_directory_changed: failed to re-read {}: {}",
                    LogPrivate::dir_path(parent_path.as_str()),
                    LogError(&e)
                );
                return;
            }
        };

        let read_by = ReadBy::Volume {
            volume: &vol,
            routed: is_routed,
        };
        let prepared = self.prepare(read_by, &volume_id, &parent_path, new_entries).await;

        for listing_id in &listings {
            self.publish_replacement(listing_id, prepared.clone(), generation);
        }
    }

    /// Finds every cached listing at or inside `root` on `volume_id`.
    ///
    /// `AppPath::starts_with` is name-wise, so `/a/foo.zip` matches the archive
    /// root listing (`/a/foo.zip`) and any inner listing (`/a/foo.zip/sub`) but never
    /// a prefix-similar sibling (`/a/foo.zipper`) or the containing directory (`/a`).
    fn find_listings_under_path_on_volume(&self, volume_id: &str, root: &AppPath) -> Vec<(String, AppPathBuf)> {
        let root = self.listing_path(volume_id, root);
        let cache = self.cache.read_ignore_poison();
        cache
            .iter()
            .filter(|(_, listing)| listing.volume_id == volume_id && listing.path.as_path().starts_with(root.as_path()))
            .map(|(id, listing)| (id.clone(), listing.path.as_path().to_owned()))
            .collect()
    }

    /// Refreshes every open listing at or inside the archive at `archive_path` on its
    /// parent drive `volume_id`, re-reading each through the freshly re-resolved
    /// `ArchiveVolume`.
    ///
    /// Three callers fire this when the backing `.zip` changes: the app's own archive
    /// edit once it commits (`archive_edit::engine::show_the_edit`), the local archive
    /// content watch (`archive::watch`, a `notify` watch on a LOCAL parent), and the
    /// SMB share watcher (`smb_watcher`, for a REMOTE parent that has no local
    /// `notify` transport — see `crates/cmdr-smb/DETAILS.md` § "SMB archive push-refresh").
    /// It deliberately does NOT go through `notify_directory_changed`: that
    /// function runs the drive-index sync (`apply_smb_change`) up front, and an
    /// archive-inner path (`/…/foo.zip/dir`) isn't a real filesystem path, so feeding
    /// it to the index would be meaningless. Here we only refresh the pane listings.
    ///
    /// A listing whose re-read fails (a mid-write, truncated central directory) is
    /// left untouched by `notify_full_refresh` — it keeps its previous entries
    /// rather than blanking the pane, and the next change event retries. The FE only
    /// surfaces the damaged-archive banner at navigation time, never from this
    /// refresh path.
    pub(crate) async fn refresh_archive_listings(&self, volume_id: &str, archive_path: &AppPath) {
        let listings = self.find_listings_under_path_on_volume(volume_id, archive_path);
        for (listing_id, path) in listings {
            // Each inner listing lives at its own path, so refresh per listing path
            // (two panes on the same inner dir share a path and coalesce naturally).
            self.notify_full_refresh(volume_id.to_string(), path, vec![listing_id])
                .await;
        }
    }
}
