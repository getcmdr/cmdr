//! The volume delete's scan phase: the walk that lists what a delete will
//! remove, deepest first, with what each entry is (a link is a leaf, #505) and
//! what a re-run after an outage must still find (`outage.rs`).

use crate::rt::Instant;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::event_sinks::OperationEventSink;
use super::super::scan_preview::entry_in_parent_listing;
use super::super::state::WriteOperationState;
use super::super::types::{WriteOperationError, WriteOperationPhase, WriteOperationType, WriteProgressEvent};
use super::outage::{Listed, read_through_outages};
use crate::transfer::volume::{PathRole, listed_path, map_volume_error};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::{EntryKind, Volume, VolumeError};

/// Entry collected during the volume scan phase. `progress_bytes` is the
/// progress contribution: equals the size for the first occurrence of an inode
/// in the scan, `0` for hardlink dupes. Mirrors `FileInfo::progress_bytes` on
/// the local-FS walker. Volume backends without inode info (MTP, SMB) populate
/// this as the size for every entry — they never produce hardlinks, so no
/// dedup happens and the count is correct. `listed` is what a delete re-run
/// after an outage must still find at `path` (`outage.rs`).
pub(super) struct VolumeDeleteEntry {
    pub(super) path: PathBuf,
    pub(super) progress_bytes: u64,
    pub(super) is_dir: bool,
    pub(super) listed: Listed,
}

/// Tracks the running tally across the whole recursive scan so the per-entry
/// `list_directory` callback (which can fire while a single dir is still
/// streaming entries from a slow MTP USB roundtrip) reads a coherent total.
pub(super) struct VolumeScanTracker {
    /// Last emit timestamp for the global throttle (shared across recursion levels).
    last_emit: Mutex<Instant>,
    progress_interval: Duration,
    /// Files committed to `entries` so far. The per-entry callback adds the
    /// in-flight `loaded_count` on top to form the displayed tally.
    pub(super) files_so_far: std::sync::atomic::AtomicUsize,
    pub(super) dirs_so_far: std::sync::atomic::AtomicUsize,
    pub(super) bytes_so_far: std::sync::atomic::AtomicU64,
}

impl VolumeScanTracker {
    pub(super) fn new(progress_interval: Duration) -> Self {
        Self {
            last_emit: Mutex::new(crate::rt::now()),
            progress_interval,
            files_so_far: std::sync::atomic::AtomicUsize::new(0),
            dirs_so_far: std::sync::atomic::AtomicUsize::new(0),
            bytes_so_far: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Returns true if enough time has passed since the last emit AND atomically
    /// resets the timer. Single-call gate so callers don't accidentally double-emit.
    pub(super) fn should_emit(&self) -> bool {
        // A throttle timestamp: a poisoned one is still a timestamp.
        let mut last = self.last_emit.lock_ignore_poison();
        if crate::rt::since(*last) < self.progress_interval {
            return false;
        }
        *last = crate::rt::now();
        true
    }
}

/// Enumerates a directory tree via `volume.list_directory()`, collecting files
/// and directories. Directories are appended after their children, so the
/// resulting list is already in deepest-first order for safe deletion.
///
/// The folders the walk is inside live on a `Vec`, ❌ never in a future per
/// level: that nests a poll per level, and a deep enough tree overflowed the
/// 2 MiB tokio worker the delete runs on (#1541). Only the listings on the
/// current path are held.
///
/// **A symlink is a leaf: the link goes, ❌ never its target's contents.** Only
/// an [`EntryKind::Directory`] is listed and descended. A listing sets
/// `is_directory` on a link to a folder (panes want it to navigate like one), and
/// some backends' `is_directory` follows the link, so neither may decide a
/// descent: `hint` carries an [`EntryKind`], read off an entry with
/// [`EntryKind::of`] or asked of [`Volume::entry_kind`], plus the size a
/// listing already told (the outage re-check reads it, so nothing stats again).
///
/// Emits scan progress two ways:
/// 1. **Per-entry** via `list_directory`'s `on_progress` callback, so the FE sees the tally climb
///    mid-listing — important for MTP, where one `list_directory` call on `/DCIM/Camera` with 1k+
///    photos can take ~17 s of USB roundtrips.
/// 2. **After each subtree** (as each folder ends), as a final snapshot for that branch. Throttled
///    by the shared `VolumeScanTracker` so it doesn't race with the per-entry callback.
///
/// **Cancel contract**: this function does NOT emit `write-cancelled` before returning
/// `Err(Cancelled)`. The cancel check fires before every folder, so emitting here would
/// multi-fire. Top-level callers must call `emit_cancelled_if_aborted` (or otherwise emit
/// `write-cancelled`) on the returned result before propagating it, so the FE sees the proper
/// terminal event for the cancel flow.
#[allow(
    clippy::too_many_arguments,
    reason = "Matches the parameter pattern of other write operation functions"
)]
pub(super) async fn scan_volume_recursive(
    volume: &Arc<dyn Volume>,
    volume_id: &str,
    path: &Path,
    hint: Option<Listed>,
    entries: &mut Vec<VolumeDeleteEntry>,
    total_bytes: &mut u64,
    seen_inodes: &mut std::collections::HashSet<u64>,
    state: &Arc<WriteOperationState>,
    events: &dyn OperationEventSink,
    operation_id: &str,
    tracker: &Arc<VolumeScanTracker>,
) -> Result<(), WriteOperationError> {
    use std::sync::atomic::Ordering;

    let scan = DeleteScan {
        volume,
        volume_id,
        state,
        events,
        operation_id,
        tracker,
    };
    scan.check_cancel()?;

    // Resolve what `path` is. Prefer the caller's hint (a pane's listing of the
    // parent at the top level). Without one, probe (`top_level_entry`, a read,
    // so safe to re-run after an outage), which on MTP lists the parent dir, so
    // we avoid it whenever the oracle can answer.
    // The one crossing for `path`: the walk carries `std::path` values until it's typed.
    let path_app = AppPath::legacy_unchecked(path);
    let probed = match hint {
        Some(listed) => listed,
        None => read_through_outages(volume, state, || top_level_entry_at(volume.as_ref(), &path_app))
            .await
            .map_err(|stop| stop.into_error(path))?,
    };

    if probed.kind != EntryKind::Directory {
        // Top-level file or link without listing context: size unknown, use 0.
        // Progress still tracks file count accurately.
        tracker.files_so_far.fetch_add(1, Ordering::Relaxed);
        entries.push(VolumeDeleteEntry {
            path: path.to_path_buf(),
            progress_bytes: 0,
            is_dir: false,
            listed: listed_top_level(volume.as_ref(), &path_app, probed).await,
        });
        scan.snapshot(path);
        return Ok(());
    }

    // Each folder the walk is inside, with the listed entries of it still to go.
    let mut inside: Vec<(PathBuf, std::vec::IntoIter<FileEntry>)> = Vec::new();
    let children = scan.open_folder(path, &path_app).await?;
    inside.push((path.to_path_buf(), children.into_iter()));
    let listings_mark_links = volume.backend_kind().listings_mark_links();
    while let Some((dir, children)) = inside.last_mut() {
        // Children first. list_directory returns FileEntry with size, so we use
        // child.size directly instead of calling get_metadata (which returns
        // NotSupported on MTP). A child folder's kind comes from its own row, so
        // descending it doesn't re-probe (avoids another parent listing on MTP).
        // `EntryKind::of`, ❌ never `child.is_directory`: that's true for a link to
        // a folder, and descending it deletes the folder it points at. Where a
        // listing can list a link as a folder (Samba, #613), each folder is asked
        // `entry_kind` before the walk descends it.
        let mut descend = None;
        for child in children.by_ref() {
            // The child as its listing row names it, typed. A row holding a `..` name
            // (a hostile `../x`) stops the delete before anything
            // goes, ❌ never walked: it would delete outside the selection.
            let child_path_app =
                listed_path(&child).map_err(|e| map_volume_error(&dir.display().to_string(), PathRole::Source, e))?;
            // The same child for the delete's entries, which key on `std::path`
            // (the bridge's way out, until they're typed).
            let child_path = child_path_app.as_std_legacy().into_owned();
            let mut child_kind = EntryKind::of(&child);
            if child_kind == EntryKind::Directory && !listings_mark_links {
                child_kind = read_through_outages(volume, state, || volume.entry_kind(&child_path_app))
                    .await
                    .map_err(|stop| stop.into_error(&child_path))?;
            }
            if child_kind == EntryKind::Directory {
                descend = Some((child_path, child_path_app.into_owned()));
                break;
            }
            // A link frees only itself, whatever size the listing reports
            // for what it points at.
            let size = if child_kind == EntryKind::Symlink {
                0
            } else {
                child.size.unwrap_or(0)
            };
            // Hardlink dedup: when the backend reports an inode, only
            // count the first occurrence toward `total_bytes` and the
            // tracker's running byte total. Subsequent hardlinks
            // contribute `progress_bytes = 0` so the delete loop's
            // numerator stays aligned with the denominator. Backends
            // without inode info (MTP, SMB) leave `inode = None` and
            // every entry is treated as unique — they never produce
            // hardlinks anyway. Mirrors `FileInfo::progress_bytes` on
            // the local-FS walker.
            let counts = match child.inode {
                Some(ino) => seen_inodes.insert(ino),
                None => true,
            };
            let progress_bytes = if counts { size } else { 0 };
            if counts {
                *total_bytes += size;
                tracker.bytes_so_far.fetch_add(size, Ordering::Relaxed);
            }
            tracker.files_so_far.fetch_add(1, Ordering::Relaxed);
            entries.push(VolumeDeleteEntry {
                path: child_path,
                progress_bytes,
                is_dir: false,
                listed: Listed {
                    kind: child_kind,
                    size: child.size,
                },
            });
        }
        match descend {
            Some((child_path, child_path_app)) => {
                scan.check_cancel()?;
                let children = scan.open_folder(&child_path, &child_path_app).await?;
                inside.push((child_path, children.into_iter()));
            }
            None => {
                let (dir, _) = inside.pop().expect("the folder that ended is on top");
                // Add directory after its children (already deepest-first order)
                tracker.dirs_so_far.fetch_add(1, Ordering::Relaxed);
                entries.push(VolumeDeleteEntry {
                    path: dir.clone(),
                    progress_bytes: 0,
                    is_dir: true,
                    listed: Listed {
                        kind: EntryKind::Directory,
                        size: None,
                    },
                });
                scan.snapshot(&dir);
            }
        }
    }

    Ok(())
}

/// What every folder of one delete scan reads and reports to.
struct DeleteScan<'s> {
    volume: &'s Arc<dyn Volume>,
    volume_id: &'s str,
    state: &'s Arc<WriteOperationState>,
    events: &'s dyn OperationEventSink,
    operation_id: &'s str,
    tracker: &'s Arc<VolumeScanTracker>,
}

impl DeleteScan<'_> {
    /// Before each folder: a cancelled delete stops scanning.
    fn check_cancel(&self) -> Result<(), WriteOperationError> {
        if self.state.intent().is_cancelled() {
            return Err(WriteOperationError::Cancelled {
                message: "Operation cancelled by user".into(),
            });
        }
        Ok(())
    }

    /// Refuses a volume's mount point, then lists the folder at `path`
    /// (`path_app` in the volume's spelling: the root's, or a child's own listing
    /// row), with progress climbing while the listing streams.
    async fn open_folder(&self, path: &Path, path_app: &AppPath) -> Result<Vec<FileEntry>, WriteOperationError> {
        use std::sync::atomic::Ordering;
        let DeleteScan {
            volume,
            volume_id,
            state,
            events,
            operation_id,
            tracker,
        } = *self;
        super::super::mount_boundary::refuse_a_volume_mount_point(&**volume, path, state).await?;

        // Snapshot the cumulative tallies BEFORE the (potentially slow) listing
        // call. The per-entry callback shows `files_before + loaded_count` so the
        // FE sees a climbing number even when one `list_directory` takes seconds.
        // We don't know mid-stream whether each entry is a file or dir, so
        // optimistically attribute them to `files`; the walk corrects the split
        // via the `dirs_so_far` counter.
        let files_before = tracker.files_so_far.load(Ordering::Relaxed);
        let dirs_before = tracker.dirs_so_far.load(Ordering::Relaxed);
        let bytes_before = tracker.bytes_so_far.load(Ordering::Relaxed);
        let current_dir_str = path.display().to_string();
        let op_id_for_cb = operation_id.to_string();
        let tracker_for_cb = Arc::clone(tracker);
        let state_for_cb = Arc::clone(state);

        let on_progress = move |p: cmdr_fs::volume::ListingProgress| {
            if !tracker_for_cb.should_emit() {
                return;
            }
            // The delete scan tally tracks files + bytes from already-walked
            // subdirs; the volume's per-listing callback adds the local
            // (files, bytes) it's enumerating right now.
            let files_now = files_before + p.files;
            let bytes_now = bytes_before + p.bytes;
            state_for_cb.emit_progress_via_sink(
                events,
                WriteProgressEvent::new(
                    op_id_for_cb.clone(),
                    WriteOperationType::Delete,
                    WriteOperationPhase::Scanning,
                    None,
                    files_now,
                    0,
                    bytes_now,
                    0,
                )
                .with_scan_meta(Some(current_dir_str.clone()), dirs_before + p.dirs, None),
            );
            state.scope().status().update(
                &op_id_for_cb,
                WriteOperationPhase::Scanning,
                None,
                files_now,
                0,
                bytes_now,
                0,
            );
        };

        // Oracle-first: if this directory is watcher-backed in the listing cache,
        // reuse the cached entries and skip the volume round-trip entirely.
        // Falls through to `volume.list_directory` on miss, preserving the
        // per-entry progress callback for slow MTP listings.
        Ok(
            match state.scope().host().listings().authoritative_listing(volume_id, path) {
                Some(cached) => {
                    // The cached listing is already complete, so synthesize a
                    // single end-of-listing progress tick (with the tally we'd
                    // have built incrementally) to keep the FE counter climbing
                    // during the cache-fed pass too.
                    let mut tally = cmdr_fs::volume::ListingProgress::default();
                    for e in &cached {
                        if e.is_directory {
                            tally.dirs += 1;
                        } else {
                            tally.files += 1;
                            tally.bytes += e.size.unwrap_or(0);
                        }
                    }
                    on_progress(tally);
                    cached
                }
                None => read_through_outages(volume, state, || {
                    volume.list_directory_with_cancel(path_app, Some(&on_progress), Some(&state.backend_cancel))
                })
                .await
                .map_err(|stop| stop.into_error(path))?,
            },
        )
    }

    /// The final snapshot for the subtree at `path`, which just ended
    /// (throttled so it doesn't race the per-entry callback).
    fn snapshot(&self, path: &Path) {
        use std::sync::atomic::Ordering;
        let tracker = self.tracker;
        if !tracker.should_emit() {
            return;
        }
        let files_done = tracker.files_so_far.load(Ordering::Relaxed);
        let dirs_done = tracker.dirs_so_far.load(Ordering::Relaxed);
        let bytes_done = tracker.bytes_so_far.load(Ordering::Relaxed);
        let current_dir_str = path.display().to_string();
        self.state.emit_progress_via_sink(
            self.events,
            WriteProgressEvent::new(
                self.operation_id.to_string(),
                WriteOperationType::Delete,
                WriteOperationPhase::Scanning,
                None,
                files_done,
                0,
                bytes_done,
                0,
            )
            .with_scan_meta(Some(current_dir_str), dirs_done, None),
        );
        self.state.scope().status().update(
            self.operation_id,
            WriteOperationPhase::Scanning,
            None,
            files_done,
            0,
            bytes_done,
            0,
        );
    }
}

/// What `source` is per a pane's listing of its parent (`entry_in_parent_listing`):
/// its kind, link-aware, and a file's size, with no I/O.
pub(super) fn listed_in_parent_listing(
    state: &WriteOperationState,
    volume: &dyn Volume,
    volume_id: &str,
    source: &Path,
) -> Option<Listed> {
    entry_in_parent_listing(state.scope().host().listings(), volume, volume_id, source).map(|entry| {
        let kind = EntryKind::of(&entry);
        Listed {
            kind,
            size: if kind == EntryKind::File { entry.size } else { None },
        }
    })
}

/// What a top-level entry is, with a link reported AS a link, in ONE request:
/// `entry_kind` on a backend that can hold links ([`BackendKind::has_symlinks`]),
/// where a following stat would call a link to a folder a folder (#505).
/// Elsewhere a plain `get_metadata` (what `entry_kind` asks there anyway), whose
/// size also serves a re-run's re-check (`outage.rs`), so an object store spends
/// one HEAD per selected file, the one its cost estimate counts.
///
/// [`BackendKind::has_symlinks`]: cmdr_fs::volume::BackendKind::has_symlinks
pub(super) async fn top_level_entry(volume: &dyn Volume, path: &Path) -> Result<Listed, VolumeError> {
    top_level_entry_at(volume, &AppPath::legacy_unchecked(path)).await
}

/// [`top_level_entry`] for an app path.
async fn top_level_entry_at(volume: &dyn Volume, path: &AppPath) -> Result<Listed, VolumeError> {
    if volume.backend_kind().has_symlinks() {
        return volume.entry_kind(path).await.map(|kind| Listed { kind, size: None });
    }
    volume.get_metadata(path).await.map(|entry| Listed {
        kind: EntryKind::of(&entry),
        size: if entry.is_directory { None } else { entry.size },
    })
}

/// What a top-level file or link is, for a delete re-run after an outage to
/// check against: what the walk learned of it (`probed`), plus a file's size
/// when the walk didn't learn it, by one `get_metadata`, and only on a volume
/// with a session: no other volume is ever waited for, so nothing would read
/// it. Best effort: an unanswered stat leaves the size unknown, and a re-run
/// then leaves the file rather than delete it unproven.
async fn listed_top_level(volume: &dyn Volume, path: &AppPath, probed: Listed) -> Listed {
    if probed.kind != EntryKind::File || probed.size.is_some() || volume.connection_state().is_none() {
        return probed;
    }
    match volume.get_metadata(path).await {
        Ok(entry) => Listed {
            kind: EntryKind::of(&entry),
            size: entry.size,
        },
        Err(_) => probed,
    }
}

#[cfg(test)]
#[path = "volume_scan_tests.rs"]
mod tests;
