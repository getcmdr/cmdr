//! The oracle-aware subtree scan: one subtree's totals, cache-fed where a
//! watcher can vouch for the listing, else from `Volume::list_directory`. Any
//! volume's, so it serves the volume scan preview in every build; the local
//! walk over top-level sources is `scan.rs`.

use crate::transfer::volume::{into_std, listed_path};
use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cmdr_fs::os_junk_files::OsJunkFiles;
use cmdr_fs::volume::host::listings::ListingHost;
use cmdr_fs::volume::{CopyScanResult, ListingProgress, ScanRecorder, Volume, VolumeError};

/// Totals returned by `scan_subtree_with_oracle`.
///
/// `per_path` carries one entry per direct child of the scanned `path`, sized
/// to feed into a parent `BatchScanResult` upstream. The vec is empty when
/// `path` itself is a file (the caller knows it's a file in that case).
// DEFAULT-OK: zeroed totals are what a walk that hasn't counted anything has counted.
// This is an accumulator seed, not a finding: it only ever grows from here.
#[derive(Debug, Clone, Default)]
pub(super) struct SubtreeTotals {
    pub file_count: usize,
    pub dir_count: usize,
    pub total_bytes: u64,
    /// Source on-disk footprint, hardlinks counted once. See
    /// `CopyScanResult::dedup_bytes`. Equal to `total_bytes` when the cached
    /// `FileEntry`s carry no inode (non-local backends).
    pub dedup_bytes: u64,
    /// Per-direct-child results so the scan-preview can populate the
    /// `BatchScanResult::per_path` slot the copy engine reads later.
    pub per_path: Vec<(PathBuf, CopyScanResult)>,
}

/// Scans a subtree using the fresh-listing oracle at every level, falling back
/// to `volume.list_directory` on cache miss.
///
/// This is the oracle-aware analogue of the per-volume `scan_for_copy`. It's
/// designed for `run_volume_scan_preview` to call when the parent directory of
/// the selected sources is watcher-backed: top-level files come from the
/// cached listing directly, top-level directories are walked here.
///
/// Cancellation: the future polls `is_cancelled` between entries. Symlinks
/// (cached `is_symlink == true`) are counted as one entry and not descended,
/// matching the local-FS walker's policy.
///
/// `pause` parks the walk at those same boundaries when the owning operation is
/// paused (`super::scan_bridge::ScanPause`), so pause is exactly as responsive
/// as cancel here.
///
/// `recorder` gets `path` and every entry below it, for a transfer's live tree
/// (`super::transfer_tree`), named the way the listing names them.
///
/// `listings` is the oracle: the engine host's view of what the open panes keep
/// fresh.
///
/// The folders the walk is inside live on a `Vec`, outermost first, and one
/// loop runs them all, depth first in listing order: ❌ never a future (or a
/// progress closure wrapping its parent's) per level, which overflowed a scan
/// worker's stack on a deep enough tree (#1541,
/// `transfer/volume/DETAILS.md` § "The walk keeps its depth on the heap").
#[allow(
    clippy::too_many_arguments,
    reason = "one walk's inputs: where to walk, what stops it, and who hears about it"
)]
pub(super) async fn scan_subtree_with_oracle(
    listings: &dyn ListingHost,
    volume: &dyn Volume,
    volume_id: &str,
    path: &Path,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    pause: Option<&super::scan_bridge::ScanPause>,
    on_progress: Option<&(dyn Fn(ListingProgress) + Sync)>,
    recorder: Option<&dyn ScanRecorder>,
    seen_inodes: &mut HashSet<u64>,
    os_junk: OsJunkFiles,
) -> Result<SubtreeTotals, VolumeError> {
    let scan = OracleScan {
        listings,
        volume,
        volume_id,
        is_cancelled,
        pause,
        on_progress,
        recorder,
        os_junk,
    };
    let mut inside = vec![scan.open(path, ListingProgress::default()).await?];
    loop {
        let level = inside
            .last_mut()
            .expect("the walk returns once its root ends, so it is always inside a folder");
        if let Some(child) = scan.advance(level, seen_inodes).await? {
            let baseline = level.baseline_below();
            let child = scan.open(&child, baseline).await?;
            inside.push(child);
            continue;
        }
        let ended = inside.pop().expect("the folder that ended is on top");
        match inside.last_mut() {
            Some(parent) => scan.fold_child(parent, ended.totals),
            None => return Ok(ended.totals),
        }
    }
}

/// What every level of one oracle scan reads.
struct OracleScan<'s> {
    listings: &'s dyn ListingHost,
    volume: &'s dyn Volume,
    volume_id: &'s str,
    is_cancelled: &'s (dyn Fn() -> bool + Sync),
    pause: Option<&'s super::scan_bridge::ScanPause>,
    on_progress: Option<&'s (dyn Fn(ListingProgress) + Sync)>,
    recorder: Option<&'s dyn ScanRecorder>,
    os_junk: OsJunkFiles,
}

/// One folder the scan is inside: what is left of its listing, its totals so
/// far, and the counts its progress reports stand on.
struct ScanLevel {
    entries: std::vec::IntoIter<FileEntry>,
    /// `dir_count` counts descendant folders only.
    totals: SubtreeTotals,
    /// Running tally for the `on_progress` callback, so dirs and bytes climb
    /// alongside the file count as the walk reads this subtree. Unlike
    /// `totals.dir_count` it tells the frontend "N dirs so far in this
    /// subtree", the intuitive display.
    tally: ListingProgress,
    /// What every folder above has tallied, frozen while this one is read: each
    /// report adds it, so the display stays cumulative across sibling dirs.
    baseline: ListingProgress,
    /// The child folder being walked, with the dates its `per_path` row carries
    /// once it ends.
    descended: Option<DescendedChild>,
}

struct DescendedChild {
    path: PathBuf,
    modified_at: Option<u64>,
    created_at: Option<u64>,
}

impl ScanLevel {
    /// The baseline a child folder's reports stand on: this level's, plus what
    /// this level has tallied so far.
    fn baseline_below(&self) -> ListingProgress {
        shifted(self.baseline, self.tally)
    }
}

/// `progress` as the whole walk counts it, on top of `baseline`.
fn shifted(baseline: ListingProgress, progress: ListingProgress) -> ListingProgress {
    ListingProgress {
        files: baseline.files + progress.files,
        dirs: baseline.dirs + progress.dirs,
        bytes: baseline.bytes + progress.bytes,
        ..progress
    }
}

impl OracleScan<'_> {
    fn cancelled(&self) -> Result<(), VolumeError> {
        if (self.is_cancelled)() {
            return Err(VolumeError::Cancelled("Operation cancelled by user".into()));
        }
        Ok(())
    }

    fn report(&self, baseline: ListingProgress, progress: ListingProgress) {
        if let Some(cb) = self.on_progress {
            cb(shifted(baseline, progress));
        }
    }

    /// Records `path`, then reads it from the oracle or the volume.
    async fn open(&self, path: &Path, baseline: ListingProgress) -> Result<ScanLevel, VolumeError> {
        if let Some(recorder) = self.recorder {
            recorder.dir(path);
        }
        self.cancelled()?;
        if let Some(pause) = self.pause {
            pause.park_while_paused_async().await;
        }
        // Load entries from oracle or the volume itself. The listing's own
        // progress reports stand on the baseline too.
        let entries: Vec<FileEntry> = match self.listings.authoritative_listing(self.volume_id, path) {
            Some(e) => e,
            None => {
                let listing_progress = |p: ListingProgress| self.report(baseline, p);
                let on_progress = self
                    .on_progress
                    .map(|_| &listing_progress as &(dyn Fn(ListingProgress) + Sync));
                self.volume
                    .list_directory(&AppPath::legacy_unchecked(path), on_progress)
                    .await?
            }
        };
        Ok(ScanLevel {
            entries: entries.into_iter(),
            totals: SubtreeTotals::default(),
            tally: ListingProgress::default(),
            baseline,
            descended: None,
        })
    }

    /// Counts `level`'s files from where it left off, and answers the first
    /// folder it lists, which the walk opens next; `None` once every entry is
    /// counted.
    async fn advance(
        &self,
        level: &mut ScanLevel,
        seen_inodes: &mut HashSet<u64>,
    ) -> Result<Option<PathBuf>, VolumeError> {
        while let Some(entry) = level.entries.next() {
            self.cancelled()?;
            // An OS junk file the transfer leaves out is no part of it (#314, #1305).
            if self
                .os_junk
                .leaves_out_str(&entry.name, entry.is_directory && !entry.is_symlink)
            {
                continue;
            }
            // After the cancel check, never before: cancel outranks pause.
            if let Some(pause) = self.pause {
                pause.park_while_paused_async().await;
            }
            // The child as its listing row names it, typed (a row holding a `..` name, a
            // hostile `../x`, stops the scan rather than walk it); the walk and the
            // recorder still key on `std::path`, so they get the read's own buffer.
            let child_path: PathBuf = into_std(listed_path(&entry)?.into_owned());
            if entry.is_directory && !entry.is_symlink {
                // The oracle re-applies inside the child, whose counts start fresh
                // and fold in here once it ends (`fold_child`).
                level.descended = Some(DescendedChild {
                    path: child_path.clone(),
                    modified_at: entry.modified_at,
                    created_at: entry.created_at,
                });
                return Ok(Some(child_path));
            }
            let size = entry.size.unwrap_or(0);
            if let Some(recorder) = self.recorder {
                recorder.file(&child_path, size);
            }
            // Hardlink dedup for the source-footprint number. `FileEntry.inode`
            // is `Some` only for `LocalPosixVolume` files with `nlink > 1`;
            // non-local backends leave it `None` so every file counts as
            // unique. Mirrors `LocalPosixVolume::scan_for_copy`.
            let dedup_contribution = match entry.inode {
                Some(ino) if !seen_inodes.insert(ino) => 0,
                _ => size,
            };
            let ScanLevel { totals, tally, .. } = level;
            totals.file_count += 1;
            totals.total_bytes += size;
            totals.dedup_bytes += dedup_contribution;
            tally.files += 1;
            // Dedup'd climbing display, as for a folder (`fold_child`).
            tally.bytes += dedup_contribution;
            totals.per_path.push((
                child_path,
                CopyScanResult {
                    file_count: 1,
                    dir_count: 0,
                    total_bytes: size,
                    dedup_bytes: dedup_contribution,
                    top_level_is_directory: false,
                    top_level_modified_at: entry.modified_at,
                    top_level_created_at: entry.created_at,
                },
            ));
            self.report(level.baseline, level.tally);
        }
        Ok(None)
    }

    /// A child folder of `parent` ended with `child`'s totals: they join the
    /// parent's, the folder itself counted with its descendants.
    fn fold_child(&self, parent: &mut ScanLevel, child: SubtreeTotals) {
        let descended = parent
            .descended
            .take()
            .expect("a child ends only after its parent descended into it");
        let ScanLevel { totals, tally, .. } = parent;
        totals.file_count += child.file_count;
        // The directory itself plus all its descendant dirs.
        totals.dir_count += 1 + child.dir_count;
        totals.total_bytes += child.total_bytes;
        totals.dedup_bytes += child.dedup_bytes;
        tally.files += child.file_count;
        tally.dirs += 1 + child.dir_count;
        // The scan-phase climbing display is dedup'd so it converges with
        // the inode-dedup'd index estimate (the copy headline shows the
        // write footprint separately).
        tally.bytes += child.dedup_bytes;
        totals.per_path.push((
            descended.path,
            CopyScanResult {
                file_count: child.file_count,
                dir_count: child.dir_count,
                total_bytes: child.total_bytes,
                dedup_bytes: child.dedup_bytes,
                top_level_is_directory: true,
                top_level_modified_at: descended.modified_at,
                top_level_created_at: descended.created_at,
            },
        ));
        self.report(parent.baseline, parent.tally);
    }
}

#[cfg(test)]
#[path = "scan_oracle_tests.rs"]
mod tests;
