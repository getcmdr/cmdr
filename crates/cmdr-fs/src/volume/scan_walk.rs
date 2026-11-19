//! The scan every backend whose only tools are a stat and a listing gets for
//! free: the recursive walk behind `scan_for_copy`, the batch loop behind
//! `scan_for_copy_batch`, and the name matcher behind `scan_for_conflicts`.
//!
//! **Why it lives here rather than in each backend.** The walk is arithmetic
//! over two operations every `Volume` already has, so a per-backend copy differs
//! from its neighbours only in the type name, and then drifts. A dedup rule
//! fixed in one copy and not the others is a wrong byte total in a transfer
//! estimate, which is the number the user decides on.
//!
//! ❗ **The walk lists, it never stats a child.** A stat is a round trip and a
//! listing already carries every child's size and type, so a 1,000-file folder
//! costs one round trip per DIRECTORY instead of one per file. Over a 50 ms link
//! that is the difference between a second and a minute.
//!
//! ❌ **Nothing here consults a listing cache.** A backend whose watcher backs
//! the claim may short-circuit its own batch scan against
//! `authoritative_listing` (SMB and MTP do, in their own `scan.rs`); a backend
//! with no watcher must not, or a pre-flight conflict scan misses a file and a
//! copy overwrites it. Reaching the cache is opt-in per backend, above this
//! module, and there is nothing to opt into here.
//!
//! ❌ **A scan reports no listing change.** It crosses every entry in a tree, and
//! one seam call per entry would sweep every cached listing on the volume
//! thousands of times.
//!
//! ❗ **Every entry passes the [`ScanBoundary`].** `boundary.dir(path)` runs BEFORE
//! the listing round trip and `boundary.file(path, size)` on every leaf, so a walk over a
//! sleeping share stops within one round trip of a Cancel and stands still on a
//! Pause. A backend that takes its own walk owes the same two calls; nothing else
//! makes Cancel work for it, and nothing else puts its entries in a transfer's
//! tree. Every read (the top stat, each listing) goes through
//! `boundary.through_outages`, so a dropped share is waited out by the scan's
//! owner and the walk carries on from the directory it was listing.

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::app_path::{AppNameBuf, AppPath, AppPathBuf, ListedFolder, Namer};
use crate::entry::FileEntry;
use crate::error_detail::ErrorDetail;
use crate::log_error::{ErrorLogText, ErrorText, write_plain};
use crate::log_private::LogPrivate;
use crate::volume::{BatchScanResult, CopyScanResult, ScanBoundary, ScanConflict, ScannedFile, SourceItemInfo};
use crate::volume::{NotAChildName, VolumeError};

/// A future the walk can recurse through. `async fn` can't call itself, so every
/// step of the walk hands back a boxed one, and so does every [`ScanSource`]
/// method an implementor writes.
pub type Walking<'a, T> = Pin<Box<dyn Future<Output = Result<T, VolumeError>> + Send + 'a>>;

/// The two operations a tree walk needs from a backend.
///
/// ❗ Both must be the backend's OWN read path, ❌ never a listing-cache lookup:
/// the module docs say why. Implement it on the volume and the three
/// `scan_for_*` bodies come with it.
///
/// Both take an app path, as the `Volume` reads they forward to do, so an impl
/// hands its path straight on (`self.get_metadata(path)`). The walk joins each
/// listed name with the app path's own `join`, the same on every OS; only the
/// boundary's progress and recorder, which still take a `std::path`, get the
/// bridged spelling.
pub trait ScanSource: Sync {
    /// One entry's type and size. The top of a walk is the only place a scan
    /// stats anything.
    fn scan_stat<'a>(&'a self, path: &'a AppPath) -> Walking<'a, FileEntry>;

    /// One directory's children, each carrying its own type and size.
    fn scan_list<'a>(&'a self, path: &'a AppPath) -> Walking<'a, Vec<FileEntry>>;

    /// Whether [`scan_trees`] keeps every file's size and date in
    /// `BatchScanResult::files`, for a backend whose operations are billed per
    /// object. Off by default: a million-file walk would hold a million entries
    /// nobody reads.
    fn keeps_files(&self) -> bool {
        false
    }

    /// Whether the walk may DROP a [`scan_stat`](Self::scan_stat) or
    /// [`scan_list`](Self::scan_list) still in flight when the scan's owner
    /// stops it. When `true`, each one races the stop, so a Cancel that lands
    /// while a read waits on something slow (a restic repo's index load: minutes
    /// on a big repo) ends the scan at once.
    ///
    /// ❌ Off by default, and only for a backend whose reads survive their future
    /// being dropped (detached work, no half-done transaction): a USB transfer
    /// cut mid-flight wedges a phone. Without it, a stop lands at the next
    /// boundary, one read later.
    fn abandons_reads_on_stop(&self) -> bool {
        false
    }
}

/// `read`, raced against the scan's stop when `source` may abandon it.
async fn unless_stopped<T>(
    source: &dyn ScanSource,
    boundary: &ScanBoundary<'_>,
    read: Walking<'_, T>,
) -> Result<T, VolumeError> {
    if !source.abandons_reads_on_stop() {
        return read.await;
    }
    tokio::select! {
        result = read => result,
        () = boundary.stopped() => Err(super::scan_boundary::stopped()),
    }
}

/// One subtree's file count, directory count, and bytes.
///
/// `dedup_bytes` tracks `total_bytes` exactly: a backend reaching this walk has
/// no link count to dedupe by, so the source footprint IS the write footprint.
/// A backend that grows one takes its own walk rather than a flag here.
pub fn scan_tree<'a>(
    source: &'a dyn ScanSource,
    path: &'a Path,
    boundary: &'a ScanBoundary<'a>,
) -> Walking<'a, CopyScanResult> {
    Box::pin(async move { scan_tree_at(source, &bridged(path), boundary).await })
}

/// [`scan_tree`] for an app path.
pub fn scan_tree_at<'a>(
    source: &'a dyn ScanSource,
    path: &'a AppPath,
    boundary: &'a ScanBoundary<'a>,
) -> Walking<'a, CopyScanResult> {
    Box::pin(async move {
        let mut files = None;
        scan_tree_keeping(source, path, boundary, &mut files).await
    })
}

/// A `std::path` caller's path, as the app path the walk speaks.
fn bridged(path: &Path) -> Cow<'_, AppPath> {
    AppPath::legacy_unchecked(path)
}

/// The spelling the boundary's progress, recorder, and outage judge take: they still speak
/// `std::path`, and the bridge hands back the exact native bytes.
fn for_boundary(path: &AppPath) -> Cow<'_, Path> {
    path.as_std_legacy()
}

/// `name`, listed in `folder` (`dir`, its join decided once per level), as its child:
/// [`ListedFolder`]'s join, so a remote or in-memory path never gains a Windows `\`, and a
/// top with no canonical twin (a relative `src/album` a local engine hands its backend, a
/// `..`) joins as the `std::path` walk joined it. A listed name is text the backend handed
/// over, so an escape-range code point in it is literal, as the bridge reads it.
///
/// ❗ A name that isn't one plain name (`..`, a `/`, a `\` under a drive or UNC folder)
/// fails the walk as the typed `InvalidName`, naming the folder that listed it, ❌ never a
/// join that lands somewhere else. Failing beats skipping: a skip would let a delete or a
/// copy count a folder as whole that isn't.
fn child_of(folder: &ListedFolder<'_>, dir: &AppPath, name: &str) -> Result<AppPathBuf, VolumeError> {
    let refused = || {
        VolumeError::InvalidName(ErrorDetail::of(RefusedListedName {
            name: NotAChildName(name.to_string()),
            folder: dir.shown().into_owned(),
        }))
    };
    let typed = AppNameBuf::from_typed(name, Namer::HOST).map_err(|_| refused())?;
    folder.join(&typed).map_err(|_| refused())
}

/// A listed name the walk refused, and the folder that listed it: the technical details a
/// person needs to find it deep in a tree.
struct RefusedListedName {
    name: NotAChildName,
    folder: String,
}

impl fmt::Display for RefusedListedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_plain(self, f)
    }
}

impl ErrorLogText for RefusedListedName {
    fn write_log_text(&self, out: &mut ErrorText<'_>) -> fmt::Result {
        out.error(&self.name)?;
        fmt::Write::write_str(out, ", listed in ")?;
        out.quoted_value(LogPrivate::dir_path(&self.folder))
    }
}

/// [`scan_tree`], pushing every file onto `files` when it's `Some`.
fn scan_tree_keeping<'a>(
    source: &'a dyn ScanSource,
    path: &'a AppPath,
    boundary: &'a ScanBoundary<'a>,
    files: &'a mut Option<Vec<ScannedFile>>,
) -> Walking<'a, CopyScanResult> {
    Box::pin(async move {
        let native = for_boundary(path);
        let top = boundary
            .through_outages(&native, || unless_stopped(source, boundary, source.scan_stat(path)))
            .await?;
        if !top.is_directory {
            let size = top.size.unwrap_or(0);
            keep(files, &top);
            boundary.file(&native, size).await?;
            return Ok(CopyScanResult {
                file_count: 1,
                dir_count: 0,
                total_bytes: size,
                dedup_bytes: size,
                top_level_is_directory: false,
                top_level_modified_at: top.modified_at,
                top_level_created_at: top.created_at,
            });
        }
        let mut result = CopyScanResult {
            file_count: 0,
            dir_count: 0,
            total_bytes: 0,
            dedup_bytes: 0,
            top_level_is_directory: true,
            top_level_modified_at: top.modified_at,
            top_level_created_at: top.created_at,
        };
        walk_directory(source, path, boundary, &mut result, files).await?;
        Ok(result)
    })
}

/// Pushes `entry` onto `files` when the walk keeps them.
fn keep(files: &mut Option<Vec<ScannedFile>>, entry: &FileEntry) {
    if let Some(files) = files {
        files.push(ScannedFile {
            size: entry.size.unwrap_or(0),
            modified_at: entry.modified_at,
        });
    }
}

/// One subtree, with nothing to report to and nobody to answer to.
///
/// ❗ The single-path trait method takes neither a callback nor a stop, so a
/// boundary here would have nowhere to send its counts and nothing to ask.
/// [`scan_trees`] is the one a scan preview drives, and the one a person can
/// cancel.
pub fn scan_one<'a>(source: &'a dyn ScanSource, path: &'a Path) -> Walking<'a, CopyScanResult> {
    Box::pin(async move { scan_one_at(source, &bridged(path)).await })
}

/// [`scan_one`] for an app path.
pub fn scan_one_at<'a>(source: &'a dyn ScanSource, path: &'a AppPath) -> Walking<'a, CopyScanResult> {
    Box::pin(async move {
        let boundary = ScanBoundary::silent();
        scan_tree_at(source, path, &boundary).await
    })
}

/// Several subtrees, with the counts climbing across all of them.
///
/// ❗ One boundary for the whole call, so a preview's counters keep rising
/// through path two rather than restarting: cumulative-for-the-call is what the
/// trait promises. It is also what carries the stop, so every path in the batch
/// answers to the same Cancel.
pub fn scan_trees<'a>(
    source: &'a dyn ScanSource,
    paths: &'a [PathBuf],
    boundary: &'a ScanBoundary<'a>,
) -> Walking<'a, BatchScanResult> {
    Box::pin(async move {
        let paths: Vec<AppPathBuf> = paths.iter().map(|path| bridged(path).into_owned()).collect();
        scan_trees_at(source, &paths, boundary).await
    })
}

/// [`scan_trees`] for app paths.
pub fn scan_trees_at<'a>(
    source: &'a dyn ScanSource,
    paths: &'a [AppPathBuf],
    boundary: &'a ScanBoundary<'a>,
) -> Walking<'a, BatchScanResult> {
    Box::pin(async move {
        let mut per_path = Vec::with_capacity(paths.len());
        let mut files = source.keeps_files().then(Vec::new);
        for path in paths {
            let scan = scan_tree_keeping(source, path, boundary, &mut files).await?;
            per_path.push((path.clone(), scan));
        }
        Ok(BatchScanResult {
            files,
            ..fold_batch_at(per_path)
        })
    })
}

/// Folds per-path scans into the aggregate the batch method answers with.
///
/// ❗ `top_level_is_directory` and `top_level_modified_at` are only meaningful
/// for a single path: an aggregate over several has no one type or date, and
/// callers that need them read `per_path`. Public because a backend with its own
/// batch strategy (SMB's oracle short-circuit, MTP's parent grouping) still owes
/// the same fold.
pub fn fold_batch(per_path: Vec<(PathBuf, CopyScanResult)>) -> BatchScanResult {
    fold_batch_at(
        per_path
            .into_iter()
            .map(|(path, scan)| (bridged(&path).into_owned(), scan))
            .collect(),
    )
}

/// [`fold_batch`] for app paths.
pub fn fold_batch_at(per_path: Vec<(AppPathBuf, CopyScanResult)>) -> BatchScanResult {
    let mut aggregate = CopyScanResult {
        file_count: 0,
        dir_count: 0,
        total_bytes: 0,
        dedup_bytes: 0,
        top_level_is_directory: false,
        top_level_modified_at: None,
        top_level_created_at: None,
    };
    for (_, scan) in &per_path {
        aggregate.file_count += scan.file_count;
        aggregate.dir_count += scan.dir_count;
        aggregate.total_bytes += scan.total_bytes;
        aggregate.dedup_bytes += scan.dedup_bytes;
    }
    if let [(_, only)] = per_path.as_slice() {
        aggregate.top_level_is_directory = only.top_level_is_directory;
        aggregate.top_level_modified_at = only.top_level_modified_at;
        aggregate.top_level_created_at = only.top_level_created_at;
    }
    BatchScanResult {
        aggregate,
        per_path,
        files: None,
    }
}

/// Which of `source_items` already have a name among `dest_entries`.
///
/// ❗ Pure, and takes the listing rather than fetching it: a backend that lists
/// its destination differently (SMB and MTP go through their own cache-aware
/// paths) still owes the caller the same `ScanConflict` shape, and the shape is
/// what a conflict dialog renders. [`scan_conflicts`] is the wrapper for a
/// backend with nothing special to say about the listing.
///
/// A name held only under another Unicode form (`name_fold::differ_only_in_form`)
/// is a conflict too, naming the entry in the destination's own spelling: a
/// byte-exact backend would otherwise take the copy beside it as a second,
/// identical-looking entry. An exact match wins over a look-alike.
pub fn conflicts_against(source_items: &[SourceItemInfo], dest_entries: &[FileEntry]) -> Vec<ScanConflict> {
    let mut conflicts = Vec::new();
    for item in source_items {
        let existing = dest_entries.iter().find(|entry| entry.name == item.name).or_else(|| {
            dest_entries
                .iter()
                .find(|entry| crate::name_fold::differ_only_in_form(&entry.name, &item.name))
        });
        let Some(existing) = existing else {
            continue;
        };
        conflicts.push(ScanConflict {
            source_path: item.name.clone(),
            dest_path: existing.path.clone(),
            source_size: item.size,
            dest_size: existing.size.unwrap_or(0),
            source_modified: item.modified,
            dest_modified: existing.modified_at.map(|seconds| seconds as i64),
            source_is_directory: item.is_directory,
            dest_is_directory: existing.is_directory,
        });
    }
    conflicts
}

/// One listing of `dest_path`, matched against `source_items`.
///
/// ❗ This is where a walk-driven backend keeps
/// [`Volume::scan_for_conflicts`](crate::volume::Volume::scan_for_conflicts)'
/// promise that a destination which isn't there yet answers an empty list; that
/// method's doc says why the promise exists.
pub fn scan_conflicts<'a>(
    source: &'a dyn ScanSource,
    source_items: &'a [SourceItemInfo],
    dest_path: &'a Path,
) -> Walking<'a, Vec<ScanConflict>> {
    Box::pin(async move { scan_conflicts_at(source, source_items, &bridged(dest_path)).await })
}

/// [`scan_conflicts`] for an app path.
pub fn scan_conflicts_at<'a>(
    source: &'a dyn ScanSource,
    source_items: &'a [SourceItemInfo],
    dest_path: &'a AppPath,
) -> Walking<'a, Vec<ScanConflict>> {
    Box::pin(async move {
        let entries = match source.scan_list(dest_path).await {
            Ok(entries) => entries,
            Err(VolumeError::NotFound(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(conflicts_against(source_items, &entries))
    })
}

/// One directory's contents, folded into `into`, recursing into children.
///
/// Split from [`scan_tree`] so the top-level stat is paid once rather than once
/// per level: a child's type and size are already in its parent's listing.
fn walk_directory<'a>(
    source: &'a dyn ScanSource,
    dir: &'a AppPath,
    boundary: &'a ScanBoundary<'a>,
    into: &'a mut CopyScanResult,
    files: &'a mut Option<Vec<ScannedFile>>,
) -> Walking<'a, ()> {
    Box::pin(async move {
        into.dir_count += 1;
        let native = for_boundary(dir);
        boundary.dir(&native).await?;
        // How this level's names join, decided once for all of them.
        let folder = ListedFolder::new(dir);
        // A listing a dropped session cut off runs again once the owner has
        // waited it out; its entries are counted only after it answers.
        for entry in boundary
            .through_outages(&native, || unless_stopped(source, boundary, source.scan_list(dir)))
            .await?
        {
            // A Mac-only file the transfer leaves out is no part of it (#314).
            if boundary.leaves_out(&entry.name, entry.is_directory && !entry.is_symlink) {
                continue;
            }
            let child = child_of(&folder, dir, &entry.name)?;
            // ❗ A symlinked directory is ONE entry, never a subtree. Following
            // one double-counts its target (Android's `/sdcard` and
            // `/storage/emulated/0` are the same bytes) and a link pointing at
            // an ancestor turns the scan into a hang. `scan_preview.rs` makes
            // the same promise app-side, so a copy estimate reads the same
            // whichever walker produced it.
            if entry.is_directory && !entry.is_symlink {
                walk_directory(source, &child, boundary, into, files).await?;
            } else {
                let size = entry.size.unwrap_or(0);
                keep(files, &entry);
                into.file_count += 1;
                into.total_bytes += size;
                into.dedup_bytes += size;
                boundary.file(&for_boundary(&child), size).await?;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "scan_walk_test.rs"]
mod scan_walk_test;
