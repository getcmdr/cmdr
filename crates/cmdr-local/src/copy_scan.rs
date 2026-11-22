//! The copy scan of a local tree, for both local volumes: one subtree's file count, folder
//! count, and bytes (`Volume::scan_for_copy`), and the batch over a selection that asks its
//! boundary between sources and inside each walk (`Volume::scan_for_copy_batch_with_boundary`).
//!
//! One walk for `LocalPosixVolume` and `LocalWindowsVolume`: each sibling hands in the OS
//! path it crossed to, the caller's spelling the recorder names entries under, and its
//! AppleDouble rule (a FAT folder on a Mac; Windows keeps no sidecars, so its rule never
//! matches). Counting a tree while staying cancelable is its own subject, as `mtp/scan.rs`
//! and `crates/cmdr-smb/src/volume/scan.rs` are.

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::apple_double::SidecarRule;
use cmdr_fs::file_identity::{device_of, first_sighting};
use cmdr_fs::mac_only_files::MacOnlyFiles;
use cmdr_fs::volume::{BatchScanResult, CopyScanResult, ScanBoundary, ScanRecorder, ScanStop, VolumeError};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tokio::task::spawn_blocking;
use walkdir::WalkDir;

/// A boxed future a scan body hands back, as the `Volume` trait's methods return it.
type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, VolumeError>> + Send + 'a>>;

/// How a walked entry is named to the recorder: the caller's spelling of the scanned path,
/// with the entry's path below the walk's OS root (empty for the root itself).
pub(crate) type SpellBelow = fn(as_called: &Path, below: &Path) -> PathBuf;

/// One subtree's walk, as a sibling hands it in.
pub(crate) struct Subtree {
    /// The path the OS opens.
    pub(crate) os: PathBuf,
    /// The scanned path as the caller spelled it, which the recorder names entries under.
    /// `ScanRecorder` still takes `std::path` (it lives in the write engine, typed with it,
    /// app-path plan milestone 10a), so the caller's spelling crosses back once per scan.
    pub(crate) as_called: PathBuf,
    /// How an entry below the root joins onto `as_called`.
    pub(crate) spell_below: SpellBelow,
    /// Which `._name` files the walk leaves out as a FAT folder's sidecars.
    pub(crate) sidecars: SidecarRule,
}

/// One subtree's file count, directory count, and bytes, stopping when `stop`
/// says to.
///
/// ❗ The walk runs inside `spawn_blocking`, so it asks
/// [`ScanStop::should_stop_blocking`](cmdr_fs::volume::ScanStop::should_stop_blocking)
/// — the twin that parks this pool THREAD. The async one has nothing to await
/// on in here. `ScanStop` is `Arc`-held precisely so it can cross into the
/// closure.
///
/// ❗ Per entry, ❌ not per directory. On a mounted network share (`smbfs`,
/// NFS, a mapped drive) a single `readdir` can take seconds and a subtree
/// minutes, and `stop` costs two atomic loads when nobody has pressed anything.
///
/// `recorder` gets every entry the walk counts, named under the caller's spelling.
/// Handed in directly because the walk can't await the boundary from in here (see
/// `ScanBoundary::counted`).
///
/// `mac_only` leaves out the Mac-only files a transfer won't write (#314),
/// below the selection only, like the sidecars.
pub(crate) fn scan_subtree<'a>(
    subtree: Subtree,
    stop: ScanStop,
    recorder: Option<Arc<dyn ScanRecorder>>,
    mac_only: MacOnlyFiles,
) -> Boxed<'a, CopyScanResult> {
    let Subtree {
        os: abs_path,
        as_called,
        spell_below,
        sidecars,
    } = subtree;
    Box::pin(async move {
        spawn_blocking(move || {
            // The entry as the caller spells it, for the recorder.
            let as_called_path = |entry_path: &Path| match entry_path.strip_prefix(&abs_path) {
                Ok(relative) if !relative.as_os_str().is_empty() => spell_below(&as_called, relative),
                _ => as_called.clone(),
            };

            let mut file_count = 0;
            let mut dir_count = 0;
            let mut total_bytes = 0u64;
            // `dedup_bytes` is the source's on-disk (`du`) footprint:
            // each inode counted once. `total_bytes` keeps counting every
            // hardlink (the cross-volume write footprint). The set is
            // scoped to this one top-level source; cross-source hardlinks
            // aren't deduped (rare; see `CopyScanResult::dedup_bytes`).
            // Windows' `Metadata` carries no link count, so every name
            // counts there (`first_sighting`).
            let mut dedup_bytes = 0u64;
            let mut seen_inodes: std::collections::HashSet<u64> = std::collections::HashSet::new();

            // The same walk a transfer makes, so the count leaves out what
            // `list_directory_as_source` leaves out. The top level is the
            // person's selection, never filtered.
            let walk = WalkDir::new(&abs_path).min_depth(0).into_iter().filter_entry(|entry| {
                entry.depth() == 0
                    || !(is_walked_sidecar(&sidecars, entry)
                        || mac_only.leaves_out(entry.file_name(), entry.file_type().is_dir()))
            });
            for entry in walk {
                if stop.should_stop_blocking() {
                    return Err(cmdr_fs::volume::scan_stopped());
                }
                let entry = entry.map_err(|e| VolumeError::IoError {
                    message: e.to_string().into(),
                    raw_os_error: None,
                })?;
                let ft = entry.file_type();
                if ft.is_file() {
                    file_count += 1;
                    let meta = entry.metadata();
                    if let Some(recorder) = &recorder {
                        let len = meta.as_ref().map(|meta| meta.len()).unwrap_or(0);
                        recorder.file(&as_called_path(entry.path()), len);
                    }
                    if let Ok(meta) = meta {
                        let len = meta.len();
                        total_bytes += len;
                        if first_sighting(&meta, &mut seen_inodes) {
                            dedup_bytes += len;
                        }
                    }
                } else if ft.is_dir() {
                    if let Some(recorder) = &recorder {
                        recorder.dir(&as_called_path(entry.path()));
                    }
                    // Don't count the root itself if it's the starting point
                    if entry.depth() > 0 {
                        dir_count += 1;
                    }
                } else if ft.is_symlink() && entry.depth() > 0 {
                    // One entry worth no bytes, as `scan_walk` and the preview
                    // count it (a junction too, on Windows). Not a SELECTED link
                    // (depth 0): `WalkDir` walks through it, and the top-level
                    // stat below answers for it.
                    file_count += 1;
                    if let Some(recorder) = &recorder {
                        recorder.file(&as_called_path(entry.path()), 0);
                    }
                }
            }

            // Top-level stat (also fills in single-file / empty-dir edge cases).
            // This runs regardless so we can populate `top_level_is_directory`
            // without re-statting downstream.
            let top_meta = std::fs::metadata(&abs_path).ok();
            let top_level_is_directory = top_meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

            // If the path is a single file, count it
            if let Some(ref meta) = top_meta {
                if meta.is_file() && file_count == 0 {
                    file_count = 1;
                    total_bytes = meta.len();
                    dedup_bytes = meta.len();
                } else if meta.is_dir() && dir_count == 0 && file_count == 0 {
                    dir_count = 1;
                }
            }

            let (top_level_modified_at, top_level_created_at) =
                CopyScanResult::local_top_level_dates(top_meta.as_ref());
            Ok(CopyScanResult {
                file_count,
                dir_count,
                total_bytes,
                dedup_bytes,
                top_level_is_directory,
                top_level_modified_at,
                top_level_created_at,
            })
        })
        .await
        .expect("spawn_blocking scan_for_copy closure doesn't panic and the task is uncancelable")
    })
}

/// Walks each source in turn, asking `boundary` between paths and handing each walk the
/// stop so it also asks inside one. `subtree` is the sibling's crossing of one source.
pub(crate) fn scan_batch<'a>(
    paths: &'a [AppPathBuf],
    boundary: &'a ScanBoundary<'a>,
    subtree: impl Fn(&AppPath) -> Result<Subtree, VolumeError> + Send + Sync + 'a,
) -> Boxed<'a, BatchScanResult> {
    Box::pin(async move {
        let mut per_path = Vec::with_capacity(paths.len());
        for path in paths {
            boundary.check().await?;
            let scan = scan_subtree(
                subtree(path)?,
                boundary.stop(),
                boundary.recorder().cloned(),
                boundary.mac_only(),
            )
            .await?;
            boundary.counted(&scan).await?;
            per_path.push((path.clone(), scan));
        }
        Ok(cmdr_fs::volume::scan_walk::fold_batch_at(per_path))
    })
}

/// Whether the scan walk leaves `entry` out as an AppleDouble sidecar. Only a
/// `._` name pays for the `lstat`; a sidecar is a plain file, so its device is
/// its folder's.
fn is_walked_sidecar(sidecars: &SidecarRule, entry: &walkdir::DirEntry) -> bool {
    if !entry.file_name().as_encoded_bytes().starts_with(b"._") {
        return false;
    }
    let (Some(dir), Ok(meta)) = (entry.path().parent(), entry.metadata()) else {
        return false;
    };
    sidecars.is_sidecar(dir, device_of(&meta), entry.file_name())
}
