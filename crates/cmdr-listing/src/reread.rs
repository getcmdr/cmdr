//! What a host's change watch asks the engine once it heard something: a full
//! re-read of one listing ([`refresh`], or [`begin_read`] + [`refresh_from_entries`]
//! when the host reads the directory itself), or an incremental patch of the
//! children it re-stat'd ([`observe_children`]). The `Listings` methods are in
//! `engine/reread.rs`.
//!
//! [`refresh`]: super::Listings::refresh
//! [`begin_read`]: super::Listings::begin_read
//! [`refresh_from_entries`]: super::Listings::refresh_from_entries
//! [`observe_children`]: super::Listings::observe_children
//!
//! The OS half (debouncing, rebasing event paths, the stat, when to fall back to
//! a full re-read, telling the pane its directory is gone) stays with the host.
//!
//! **No full re-read lands after a newer one did.** Every full re-read (this
//! file's two, a `FullRefresh`, a respell, a backend's `Replaced`) takes a
//! [`ReadGeneration`] when it STARTS reading, and [`Inner::update_listing_entries`],
//! the one step that stores a re-read, drops it under the cache write lock when
//! the listing already published a newer one. Incremental patches take none and
//! bump none: a re-read that started before a patch can still land after it and
//! carry the older state, since bumping would drop every re-read during a busy
//! burst. `DETAILS.md` § "Read generations".

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_error::LogError;
use cmdr_fs::volume::VolumeError;

use super::Inner;
use super::listing_id::ListingId;
use super::pipeline::{OverlayRows, Prepared, ReadBy};
use super::sorting::sort_entries;

/// How a full re-read of one listing ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The re-read replaced the listing's entries.
    Landed,
    /// A re-read that started later already published, so this one's older
    /// answer was dropped.
    Superseded,
    /// The directory itself is gone (`NotFound`). The host tells the pane and
    /// stops whatever watches it. Carries the listing's path.
    DirectoryGone(AppPathBuf),
    /// Nothing serves the listing's volume id any more. The host may read the
    /// directory itself: [`begin_read`](super::Listings::begin_read), then
    /// [`refresh_from_entries`](super::Listings::refresh_from_entries).
    NoVolume,
    /// The listing isn't open (any more).
    ListingGone,
    /// The re-read failed and the listing keeps what it had: silently on
    /// `PermissionDenied`, with a warning for anything else.
    Skipped,
}

/// A re-read the host does itself, begun before it reads: what to read, and
/// which listing the entries are for. Not `Clone`: one read, one hand-over.
#[derive(Debug)]
pub struct ReadTicket {
    id: ListingId,
    volume_id: String,
    path: AppPathBuf,
    generation: ReadGeneration,
}

/// When a full re-read started reading, from its engine's one counter: a later
/// start has a higher generation. A listing remembers the newest one it
/// published, and a re-read older than that is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReadGeneration(u64);

impl ReadGeneration {
    /// What the streaming read's cache insert publishes, so any later re-read wins.
    pub(crate) const INSERT: Self = Self(0);
}

impl ReadTicket {
    /// The directory to read, in the listing's spelling.
    pub fn path(&self) -> &AppPath {
        &self.path
    }
}

impl Inner {
    /// The generation for a full re-read starting NOW. ❗ Take it immediately
    /// before the read, after any turn or volume lookup it waits on: one taken
    /// while queued would lose to a read that started later.
    pub(crate) fn next_read_generation(&self) -> ReadGeneration {
        ReadGeneration(self.read_generations.fetch_add(1, Ordering::Relaxed) + 1)
    }

    pub(crate) async fn refresh(&self, id: &ListingId) -> RefreshOutcome {
        let Some((volume_id, path)) = self.get_listing_volume_id_and_path(id.as_str()) else {
            return RefreshOutcome::ListingGone;
        };

        // Resolve (not plain `get`) so a `.zip`-crossing listing re-reads through the
        // same ArchiveVolume the listing used, re-registering it if the LRU evicted it.
        let resolved = self.volumes.resolve(&volume_id, &path).await;
        let is_routed = resolved.is_routed();
        let Some(volume) = resolved.volume else {
            return RefreshOutcome::NoVolume;
        };

        let generation = self.next_read_generation();
        let entries = match volume.list_directory(&path, None).await {
            Ok(entries) => entries,
            Err(VolumeError::NotFound(_)) => return RefreshOutcome::DirectoryGone(path),
            Err(VolumeError::PermissionDenied { .. }) => return RefreshOutcome::Skipped,
            Err(e) => {
                log::warn!("Watcher: Failed to re-read directory: {}", LogError(&e));
                return RefreshOutcome::Skipped;
            }
        };

        // `list_directory` answers with the real directory alone, so without the
        // overlays a ⌘R (or the top-up every copy, move, and delete fires when it
        // settles) would empty a repo's six portal rows out of an open `.git/` pane.
        let read_by = ReadBy::Volume {
            volume: &volume,
            routed: is_routed,
        };
        let prepared = self.prepare(read_by, &volume_id, &path, entries).await;
        self.update_listing_entries(id.as_str(), prepared, generation)
    }

    pub(crate) fn begin_read(&self, id: &ListingId) -> Option<ReadTicket> {
        let (volume_id, path) = self.get_listing_volume_id_and_path(id.as_str())?;
        Some(ReadTicket {
            id: id.clone(),
            volume_id,
            path,
            // Taken here, before the host reads, so it dates the read rather
            // than the hand-over.
            generation: self.next_read_generation(),
        })
    }

    pub(crate) async fn refresh_from_entries(&self, ticket: ReadTicket, entries: Vec<FileEntry>) -> RefreshOutcome {
        let ReadTicket {
            id,
            volume_id,
            path,
            generation,
        } = ticket;
        let prepared = self.prepare(ReadBy::StdFs, &volume_id, &path, entries).await;
        self.update_listing_entries(id.as_str(), prepared, generation)
    }

    /// Replaces a listing's entries with a re-read's (`pipeline.rs`), re-sorted
    /// the listing's way, and publishes what changed: THE step every full
    /// re-read lands through. Drops the re-read when the listing already
    /// published one that started later than `generation`.
    ///
    /// A volume re-read also refreshes the dates the listing's volume records,
    /// BEFORE the sort, so a backend swap that gains or loses a date re-sorts
    /// into the new effective column here, and the batch says so.
    pub(crate) fn update_listing_entries(
        &self,
        listing_id: &str,
        prepared: Prepared,
        generation: ReadGeneration,
    ) -> RefreshOutcome {
        let indexed = prepared.indexed();
        let (entries, overlay_rows, dates) = prepared.into_parts();
        // Both resolved before the lock: each asks the host.
        let visibility = self.scratch_visibility();
        let collator = self.collator();
        let mut cache = self.write_cache();
        let Some((listing, publish)) = cache.get_mut(listing_id) else {
            return RefreshOutcome::ListingGone;
        };
        if listing.published_generation > generation {
            log::debug!(
                target: "listing",
                "listing {listing_id}: dropping a re-read that a newer one already superseded"
            );
            return RefreshOutcome::Superseded;
        }
        listing.published_generation = generation;
        listing.reconcile_scratch(listing_id, visibility, &publish);
        listing.touch();
        let effective_moved = listing.set_listed_dates(dates);
        let mut entries = entries;
        sort_entries(&collator, &mut entries, listing.effective());
        let changes = listing.replace_entries(entries, visibility);
        if let OverlayRows::Recounted(count) = overlay_rows {
            listing.set_overlay_rows(count);
        }
        listing.publish_changes_and_sort(&publish, listing_id, changes, effective_moved);
        drop(cache);

        // The pipeline enriched these rows before the read landed; an index event
        // that came in between asked about the rows this replaced, and its answer is
        // now dropped as stale (`index_sizes.rs`). So ask again for the rows that
        // landed, with no lock held, and publish the sizes that moved since: the
        // diff above carried the pipeline's. A routed volume's rows have no index
        // to ask.
        if indexed {
            self.enrich_index_sizes(listing_id);
        }
        RefreshOutcome::Landed
    }

    pub(crate) fn observe_children(&self, id: &ListingId, children: Vec<(PathBuf, Option<FileEntry>)>) {
        let listing_id = id.as_str();
        let Some((volume_id, _)) = self.get_listing_volume_id_and_path(listing_id) else {
            return;
        };

        let mut adds: Vec<FileEntry> = Vec::new();
        let mut removes: Vec<PathBuf> = Vec::new();
        let mut modifies: Vec<FileEntry> = Vec::new();
        for (path, stat) in children {
            let in_cache = self.has_entry(listing_id, &path.to_string_lossy());
            match (in_cache, stat) {
                (true, Some(entry)) => modifies.push(entry),
                (true, None) => removes.push(path),
                (false, Some(entry)) => adds.push(entry),
                // Not in the listing and gone from disk: nothing to do.
                (false, None) => {}
            }
        }
        if adds.is_empty() && removes.is_empty() && modifies.is_empty() {
            return;
        }

        // ONE enrich for the batch, since a seam is called per mutation, never per
        // entry. Every entry is a direct child of the listing's directory, so the
        // batch resolves one parent.
        let added = adds.len();
        let mut batch: Vec<FileEntry> = adds.into_iter().chain(modifies).collect();
        self.index.enrich(&volume_id, &mut batch);
        let modifies = batch.split_off(added);
        let adds = batch;

        // Removes first (their rows are the OLD pane's), then adds, then modifies.
        // `remove_entries_by_paths` resolves every entry against the pre-removal
        // listing and drops them highest-index-first, so no removal shifts the next
        // one's entry. Every patch lands in the cache; only the ones the pane shows
        // on some side become a change (`DiffChange::for_pane`), so a dotfile write
        // in `~` with hidden files off emits nothing.
        let _ = self.remove_entries_by_paths(listing_id, &removes);
        for entry in adds {
            let _ = self.insert_entry_sorted(listing_id, entry);
        }
        for mut entry in modifies {
            // A re-stat reads no xattr, so a bare modify would blank the file's
            // Finder tag dots.
            self.carry_forward_deferred(listing_id, &mut entry);
            let _ = self.update_entry_sorted(listing_id, entry);
        }
    }
}
