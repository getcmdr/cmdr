//! Patching one open listing in place: insert, remove, and update an entry at
//! its sorted position, apply what the deferred passes read (Finder tags,
//! packages macOS reports), and carry both across a re-stat.

use std::path::PathBuf;
use std::sync::Arc;

use cmdr_fs::ignore_poison::RwLockIgnorePoison;

use crate::Inner;
use crate::diff::{DiffChange, PaneRows};
use crate::sorting::{entry_comparator, sort_key_changed};
use cmdr_fs::entry::{FileEntry, TagRef};

impl Inner {
    /// Inserts a `FileEntry` into a cached listing at the correct sorted position.
    ///
    /// Uses `partition_point` with the listing's sort comparator to find the insertion index.
    /// Returns the row the pane shows it on (`after: None` when the pane doesn't show
    /// it), or `None` if the listing wasn't found or the entry already exists (checked
    /// by path).
    ///
    /// ❗ Every helper here reads the pane's rows off the map as it stood BEFORE its own
    /// patch, while that map is still standing: the patch drops it, and rebuilding it
    /// per patch would make a burst of adds rebuild it once per add.
    pub(crate) fn insert_entry_sorted(&self, listing_id: &str, entry: FileEntry) -> Option<PaneRows> {
        // Both resolved before the lock: each asks the host.
        let visibility = self.scratch_visibility();
        let collator = self.collator();
        let mut cache = self.write_cache();
        let (listing, publish) = cache.get_mut(listing_id)?;
        listing.reconcile_scratch(listing_id, visibility, &publish);
        listing.touch();
        listing.note_patch(&entry.name, self.read_generation_now());

        // Don't insert if an entry with this path already exists. Asked BEFORE
        // `entries_mut` drops the path map, so the guard rides one when it's there.
        if listing.index_of_path(&entry.path).is_some() {
            return None;
        }

        listing.admit_entry(&entry, visibility);
        let cmp = entry_comparator(Arc::clone(&collator), listing.effective());
        let pos = listing
            .entries()
            .partition_point(|existing| cmp(existing, &entry).is_lt());
        // The rows above `pos` are the same ones before and after the insert.
        let after = listing.shows(&entry).then(|| listing.pane_rows().rows_before(pos));
        listing.entries_mut().insert(pos, entry.clone());
        let rows = PaneRows { before: None, after };
        listing.publish_changes(
            &publish,
            listing_id,
            DiffChange::for_pane(entry, rows).into_iter().collect(),
        );
        Some(rows)
    }

    /// Removes every entry `paths` names, returning `(pane rows, entry)` for the ones
    /// the listing held, highest entry index first.
    ///
    /// **The batch form is the only by-path removal**, because its caller is always a
    /// batch: one coalesced watcher event carries up to 500 paths. Why that matters,
    /// and what removing them one at a time cost: `DETAILS.md` § "Entries by path".
    ///
    /// ❗ Rows are the PRE-removal pane's, which is the space a `directory-diff`
    /// removal speaks, and resolving plus removing under ONE write lock is what keeps
    /// them true — no other writer can move a row in between. Highest-first is the
    /// order that stops each removal shifting an entry a later one still points at.
    pub(crate) fn remove_entries_by_paths(&self, listing_id: &str, paths: &[PathBuf]) -> Vec<(PaneRows, FileEntry)> {
        let visibility = self.scratch_visibility();
        let mut cache = self.write_cache();
        let Some((listing, publish)) = cache.get_mut(listing_id) else {
            return Vec::new();
        };
        listing.reconcile_scratch(listing_id, visibility, &publish);
        listing.touch();

        let path_strings: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        let found = listing.indices_of_paths(path_strings.iter().map(String::as_str));
        // Every path, held or not: a re-read in flight may have seen one this
        // listing never held, and its landing must stat it again. A held one goes
        // by its entry's name, which a non-UTF-8 name needs.
        if listing.tracks_patches() {
            let names: Vec<String> = paths
                .iter()
                .zip(&found)
                .filter_map(|(path, index)| match index {
                    Some(index) => Some(listing.entries()[*index].name.clone()),
                    None => path.file_name().map(|name| name.to_string_lossy().into_owned()),
                })
                .collect();
            let now = self.read_generation_now();
            for name in &names {
                listing.note_patch(name, now);
            }
        }
        let mut doomed: Vec<usize> = found.into_iter().flatten().collect();
        // Highest first, and each row at most once: a repeated path in `paths` must
        // not take a second, innocent row down with it.
        doomed.sort_unstable_by(|a, b| b.cmp(a));
        doomed.dedup();
        // ❗ Return before `entries_mut`, which drops both maps. A watcher event whose
        // removals all landed in another listing must not cost this one its maps, and
        // an add-only event calls straight through here with an empty `paths`.
        if doomed.is_empty() {
            return Vec::new();
        }

        let rows: Vec<PaneRows> = {
            let pane = listing.pane_rows();
            doomed
                .iter()
                .map(|&index| PaneRows {
                    before: pane.row_of_entry(index),
                    after: None,
                })
                .collect()
        };
        let entries = listing.entries_mut();
        let removed: Vec<(PaneRows, FileEntry)> = rows
            .into_iter()
            .zip(doomed)
            .map(|(rows, index)| (rows, entries.remove(index)))
            .collect();
        for (_, entry) in &removed {
            listing.forget_entry(entry);
        }
        let changes = removed
            .iter()
            .filter_map(|(rows, entry)| DiffChange::for_pane(entry.clone(), *rows))
            .collect();
        listing.publish_changes(&publish, listing_id, changes);
        removed
    }

    /// Removes the entry whose filename equals `name` from a listing, returning the
    /// row the pane showed it on and the entry.
    ///
    /// A cached listing is exactly one directory, so entry names are unique, and
    /// matching by name (not full path) keeps the `Removed` patch independent of how
    /// the notifier spelled the parent. That matters for MTP: its entries carry the
    /// inner path (`/Documents/notes.txt`) while `notify_mutation` reports the parent
    /// at the `mtp://…` URL, so a full-path match would drop the removal. Local and
    /// SMB entries share the notifier's spelling, where name matching is equivalent.
    pub(crate) fn remove_entry_by_name(&self, listing_id: &str, name: &str) -> Option<(PaneRows, FileEntry)> {
        let visibility = self.scratch_visibility();
        let mut cache = self.write_cache();
        let (listing, publish) = cache.get_mut(listing_id)?;
        listing.reconcile_scratch(listing_id, visibility, &publish);
        listing.touch();
        listing.note_patch(name, self.read_generation_now());
        let idx = listing
            .entries()
            .iter()
            .position(|e| last_name(&e.path) == Some(name))?;
        let rows = PaneRows {
            before: listing.pane_rows().row_of_entry(idx),
            after: None,
        };
        let entry = listing.entries_mut().remove(idx);
        listing.forget_entry(&entry);
        listing.publish_changes(
            &publish,
            listing_id,
            DiffChange::for_pane(entry.clone(), rows).into_iter().collect(),
        );
        Some((rows, entry))
    }

    /// Checks whether a cached listing contains an entry with the given path.
    pub(crate) fn has_entry(&self, listing_id: &str, path: &str) -> bool {
        let cache = self.cache.read_ignore_poison();
        cache
            .get(listing_id)
            .is_some_and(|listing| listing.index_of_path(path).is_some())
    }

    /// Updates an existing entry in the cached listing.
    ///
    /// If the entry's key under the listing's effective sort changed
    /// (`sort_key_changed`), removes the old entry and re-inserts at the correct
    /// sorted position. Otherwise updates in place.
    /// Returns the rows the pane showed it on before and shows it on after, or `None` if
    /// the listing or entry wasn't found.
    pub(crate) fn update_entry_sorted(&self, listing_id: &str, new_entry: FileEntry) -> Option<PaneRows> {
        // Both resolved before the lock: each asks the host.
        let visibility = self.scratch_visibility();
        let collator = self.collator();
        let mut cache = self.write_cache();
        let (listing, publish) = cache.get_mut(listing_id)?;
        listing.reconcile_scratch(listing_id, visibility, &publish);
        listing.touch();
        listing.note_patch(&new_entry.name, self.read_generation_now());

        let sort = listing.effective();
        let cmp = entry_comparator(Arc::clone(&collator), sort);
        // ❗ Before `entries_mut`, which drops the path map: after it, this could only
        // walk. Still true when the mutation lands, because the cache write
        // lock is held across both, and a modify that finds nothing now leaves both
        // maps standing rather than dropping them for a listing it never touched.
        let idx = listing.index_of_path(&new_entry.path)?;
        let old = &listing.entries()[idx];
        let sort_relevant_changed = sort_key_changed(old, &new_entry, &sort);

        // Where the entry goes, and the pane's rows around it, both read off the
        // listing as it stands. Everything the old entry sorted below (itself too,
        // when it did) sits above the new position once the old one is out.
        let (new_pos, before, rows_above) = {
            let pane = listing.pane_rows();
            let before = pane.row_of_entry(idx);
            let (new_pos, rows_above) = if sort_relevant_changed {
                let below_new = listing
                    .entries()
                    .partition_point(|existing| cmp(existing, &new_entry).is_lt());
                let old_was_below = idx < below_new;
                (
                    below_new - usize::from(old_was_below),
                    pane.rows_before(below_new) - usize::from(old_was_below && before.is_some()),
                )
            } else {
                (idx, pane.rows_before(idx))
            };
            (new_pos, before, rows_above)
        };

        listing.admit_entry(&new_entry, visibility);
        let after = listing.shows(&new_entry).then_some(rows_above);
        let rows = PaneRows { before, after };
        let entries = listing.entries_mut();
        if sort_relevant_changed {
            entries.remove(idx);
            entries.insert(new_pos, new_entry.clone());
        } else {
            entries[idx] = new_entry.clone();
        }
        listing.publish_changes(
            &publish,
            listing_id,
            DiffChange::for_pane(new_entry, rows).into_iter().collect(),
        );
        Some(rows)
    }

    /// Restores what the deferred, visible-row passes learned about `entry`'s path
    /// and a re-stat can't see: its Finder tags (`enrich_tags`) and a package
    /// macOS reported (`enrich_packages`). A watcher re-stat builds entries via
    /// `get_single_entry`, which reads no xattr and only knows the package list;
    /// without this, any unrelated Modify event (a content edit, an mtime touch)
    /// would blank a file's tag dots, or turn a detected package back into a
    /// folder, until the next pass. Call this on a re-stat'd entry BEFORE it's
    /// stored and emitted, so the cache and the `directory-diff` payload stay
    /// consistent.
    ///
    /// Only ever restores: an entry that already has tags keeps them (the enrich
    /// path sets tags explicitly, including clearing to empty on an external
    /// removal, so it must never route through here), and a package is carried
    /// only onto a non-symlink folder of the same name. Revoking a package is
    /// `apply_packages_to_listing`'s job.
    pub(crate) fn carry_forward_deferred(&self, listing_id: &str, entry: &mut FileEntry) {
        let cache = self.cache.read_ignore_poison();
        let Some(listing) = cache.get(listing_id) else {
            return;
        };
        let Some(index) = listing.index_of_path(&entry.path) else {
            return;
        };
        let cached = &listing.entries()[index];
        if entry.tags.is_empty() && !cached.tags.is_empty() {
            entry.tags = cached.tags.clone();
        }
        if cached.is_package && !entry.is_package && cached.name == entry.name {
            entry.set_package(true);
        }
    }

    /// Writes what macOS said about which of `listing_id`'s folders are packages,
    /// by path, and re-places each changed row (a package has an extension, so
    /// the Ext sort can move it). Drives the deferred `enrich_packages` pass.
    ///
    /// Touches only rows the package list can't answer
    /// (`file_extension::needs_package_lookup`), so a listed `.app` is never
    /// unmarked by a lookup that failed. A `false` revokes an earlier `true`
    /// (the folder lost its bundle bit, or was renamed while the row stayed). A
    /// path not in the listing is skipped, and an unchanged row emits nothing, so
    /// re-asking a visible range is silent. The rows are few (dotted folders on
    /// screen that macOS calls packages), so each goes through
    /// `update_entry_sorted` rather than a batch of its own.
    pub(crate) fn apply_packages_to_listing(&self, listing_id: &str, updates: Vec<(String, bool)>) {
        for (path, is_package) in updates {
            let updated = {
                let cache = self.cache.read_ignore_poison();
                let Some(listing) = cache.get(listing_id) else {
                    return;
                };
                let Some(index) = listing.index_of_path(&path) else {
                    continue;
                };
                let mut entry = listing.entries()[index].clone();
                if !cmdr_fs::file_extension::needs_package_lookup(&entry.name) || !entry.set_package(is_package) {
                    continue;
                }
                entry
            };
            let _ = self.update_entry_sorted(listing_id, updated);
        }
    }

    /// Applies freshly-read Finder tags to cached entries by path and enqueues ONE
    /// coalesced `modify` diff for the rows that actually changed. Drives the deferred
    /// `enrich_tags` pass.
    ///
    /// Replaces tags **unconditionally** (including to empty), so an external removal
    /// (a user clearing all tags in Finder) propagates and clears the dots — this is
    /// the deliberate counterpart to `carry_forward_deferred`, which only ever restores.
    /// Tags are sort-irrelevant, so entries are mutated in place (no reorder). Paths
    /// not present in the listing are skipped (scrolled away, or already removed).
    /// Emits a diff only for rows whose tags genuinely changed, so re-enriching an
    /// unchanged visible range is silent (no diff storm on every scroll).
    ///
    /// **The whole batch is one pass.** Rows are found through the listing's path map
    /// (`path_index.rs`), so a 500-path enrichment chunk holds the write lock for the
    /// length of the chunk rather than 500 walks of the listing.
    pub(crate) fn apply_tags_to_listing(&self, listing_id: &str, updates: Vec<(String, Vec<TagRef>)>) {
        let visibility = self.scratch_visibility();
        {
            let mut cache = self.write_cache();
            let Some((listing, publish)) = cache.get_mut(listing_id) else {
                return;
            };
            listing.reconcile_scratch(listing_id, visibility, &publish);
            listing.touch();
            let changed = listing.set_tags_by_path(updates);
            // A tag moves no row and hides none, so a row answers both sides.
            let pane = listing.pane_rows();
            let changes = changed
                .into_iter()
                .filter_map(|index| {
                    let row = pane.row_of_entry(index)?;
                    Some(DiffChange::modified(listing.entries()[index].clone(), row))
                })
                .collect();
            drop(pane);
            listing.publish_changes(&publish, listing_id, changes);
        }
    }
}

/// The last name of an entry's path text, by `Path::file_name`'s rule over `/` alone (empty and
/// `.` names skipped, none after a `..`), so on Windows a remote name holding `\` stays one
/// name, where `Path::file_name` split it there. On macOS and Linux it answers `file_name`'s
/// exact text.
pub(crate) fn last_name(path: &str) -> Option<&str> {
    path.split('/')
        .rfind(|name| !name.is_empty() && *name != ".")
        .filter(|name| *name != "..")
}
