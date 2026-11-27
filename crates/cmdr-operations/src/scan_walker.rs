//! The shared local-FS directory walk every scan is built on.
//!
//! One walk, parameterized by a `WalkContext` of callbacks, so the copy
//! scan, the delete scan, and the dry run all descend the same way: same
//! symlink stance, same cancel checkpoints, same authoritative-listing fast
//! path. It knows nothing about copy, move, delete, or conflicts.
//!
//! `scan.rs` is what turns a walk into a `ScanResult`; `scan_dry_run.rs` turns
//! one into a conflict preview.

use crate::rt::Instant;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cmdr_fs::apple_double::SidecarRule;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::os_junk_files::OsJunkFiles;

use super::state::FileInfo;
use super::validation::is_symlink_loop;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::file_identity::{DeviceId, device_of, first_sighting};
use cmdr_fs::volume::CopyScanResult;
use cmdr_fs::volume::host::listings::ListingHost;
/// Per-regular-file hook fired by the walk, receiving the file path and size (the `WalkContext::on_file` field).
type OnFileHook<'a> = &'a dyn Fn(&Path, u64);

/// Watches a walk for a file macOS has evicted to its cloud provider
/// (`SF_DATALESS`, what Finder calls "online-only").
///
/// A selected FOLDER never carries the flag itself, so walking it is the only way
/// to learn whether trashing it would drag gigabytes back down from the provider.
/// The delete confirmation already runs this walk for its scan preview, which is
/// why the tally rides here instead of in a second pass.
///
/// Armed only when the scan's sources reach into a cloud drive
/// (`scan_preview.rs`), which keeps both the probe and the cached-listing path's
/// extra `lstat` off every ordinary walk. One hit is the whole answer (the
/// routing is all-or-nothing across the selection), so probing stops after it.
pub(super) struct OnlineOnlyWatch<'a> {
    /// Reads the flag off one entry, given its metadata when the walk already
    /// has it and `None` when the oracle handed over a cached entry instead.
    /// Injected because only a File Provider can really set `SF_DATALESS`, so no
    /// test can produce a file that carries it.
    pub(super) probe: &'a dyn Fn(&Path, Option<&fs::Metadata>) -> bool,
    /// Set by the first hit and never cleared.
    pub(super) seen: &'a AtomicBool,
}

impl OnlineOnlyWatch<'_> {
    fn note(&self, path: &Path, metadata: Option<&fs::Metadata>) {
        if self.seen.load(Ordering::Relaxed) {
            return;
        }
        if (self.probe)(path, metadata) {
            self.seen.store(true, Ordering::Relaxed);
        }
    }
}

/// Callbacks for customizing `walk_dir_recursive` behavior per caller.
///
/// `on_progress` is called as `(files_done, dirs_done, bytes_done, current_file, current_dir)`:
/// - `current_file` is just the filename component of the entry being processed
/// - `current_dir` is the absolute parent directory path, surfaced to the UI so the user sees "in
///   directory: …" alongside the filename
pub(super) struct WalkContext<'a, E> {
    pub(super) progress_interval: Duration,
    pub(super) is_cancelled: &'a dyn Fn() -> bool,
    pub(super) on_io_error: &'a dyn Fn(&Path, std::io::Error) -> E,
    pub(super) on_cancelled: &'a dyn Fn() -> E,
    pub(super) on_symlink_loop: &'a dyn Fn(&Path) -> E,
    /// `Some` stops the walk at a directory another filesystem is mounted on
    /// (`mount_boundary::is_mount_boundary`), failing with this error for that
    /// mount point. A delete's walks set it, its preview's included: a delete
    /// that walked into a mount would empty that volume. Copies leave it `None`.
    pub(super) on_mount_boundary: Option<&'a dyn Fn(&Path) -> E>,
    pub(super) on_progress: &'a dyn Fn(usize, usize, u64, Option<String>, Option<String>),
    /// Parks the walk while the owning operation is paused, called at every
    /// boundary the cancel check uses. The scan is the minutes-long part of a
    /// big transfer, so it is where Pause gets pressed; a walk that ran on
    /// regardless made the button a lie for exactly as long as it mattered.
    /// A walk with no operation behind it (a dry run, a test) passes a no-op.
    /// See `super::scan_bridge::ScanPause`.
    pub(super) park_while_paused: &'a dyn Fn(),
    /// Optional per-regular-file hook `(path, size)`, fired once for each file
    /// (not dirs or symlinks) as the walk discovers it. The compress-size
    /// estimator uses it to feed a sampling worker off the walk thread; all
    /// other callers pass `None`. Must stay cheap (a channel push) so it never
    /// lands on the walk's critical path.
    pub(super) on_file: Option<OnFileHook<'a>>,
    /// Tallies online-only files as the walk meets them. `None` for every scan
    /// whose sources don't reach into a cloud drive. See [`OnlineOnlyWatch`].
    pub(super) online_only: Option<&'a OnlineOnlyWatch<'a>>,
    /// Leaves out the AppleDouble sidecars (`._name`) a FAT, exFAT, or
    /// stream-less SMB volume keeps beside each item: the kernel serves one as
    /// its sibling's xattrs, which the copy already carries, so walking it too
    /// would land a stray hidden file. Asked of every LISTED entry and never of
    /// a selected source, so a `._` file the person picked still goes. Tests
    /// inject the volume probe to play a FAT volume on APFS.
    pub(super) sidecars: &'a SidecarRule,
    /// Leaves out the OS junk files (`cmdr_fs::os_junk_files`) a transfer that
    /// skips them won't write (#314). Like `sidecars`, asked of every LISTED
    /// entry and never of a selected source.
    pub(super) os_junk: OsJunkFiles,
    /// The fresh-listing oracle: a directory an open pane keeps watcher-fresh
    /// is walked from its listing instead of the disk. Asked only for a walk
    /// that names its volume.
    pub(super) listings: &'a dyn ListingHost,
}

/// Walks every top-level source in turn, accumulating the shared tallies AND
/// one `CopyScanResult` per source.
///
/// The per-source breakdown is what a cross-volume copy or a volume delete
/// reads back as its `source_hints` / fast-path data: whether each selected item
/// is a directory, and how big it is. Without it every source arrives at the
/// copy drivers as "unknown", and they pay a stat probe apiece to find out.
///
/// Everything else matches [`walk_dir_recursive`], which does the actual
/// walking; this only brackets each source with a before/after snapshot of the
/// shared counters.
#[allow(
    clippy::too_many_arguments,
    reason = "Mirrors walk_dir_recursive's parameter list: the shared tallies every source adds to"
)]
pub(super) fn walk_sources_with_per_path<E>(
    sources: &[PathBuf],
    files: &mut Vec<FileInfo>,
    dirs: &mut Vec<PathBuf>,
    total_bytes: &mut u64,
    dedup_bytes: &mut u64,
    last_progress_time: &mut Instant,
    visited: &mut HashSet<PathBuf>,
    seen_inodes: &mut HashSet<u64>,
    volume_id: Option<&str>,
    ctx: &WalkContext<'_, E>,
) -> Result<Vec<(PathBuf, CopyScanResult)>, E> {
    let mut per_path = Vec::with_capacity(sources.len());
    for source in sources {
        let source_root = source.parent().unwrap_or(source);
        let files_before = files.len();
        let dirs_before = dirs.len();
        let total_bytes_before = *total_bytes;
        let dedup_bytes_before = *dedup_bytes;

        walk_dir_recursive(
            source,
            source_root,
            files,
            dirs,
            total_bytes,
            dedup_bytes,
            last_progress_time,
            visited,
            seen_inodes,
            volume_id,
            ctx,
        )?;

        // `walk_dir_recursive` pushes a directory onto `dirs` before it
        // descends, so the slot at `dirs_before` holds this source itself
        // exactly when the source is a directory. A file leaves it untouched,
        // and so does a symlink to a directory, which is right for the local
        // engines: they copy the link. A cross-volume transfer streams what the
        // link points at, so its preflight rescans rather than trust this
        // (`transfer/volume/preflight.rs::counted_a_selected_link_as_a_link`).
        let top_level_is_directory = dirs.get(dirs_before).is_some_and(|dir| dir == source);
        // One `lstat` per selected item, local and cheap: the walk above kept
        // no handle on the top-level entry it read.
        let top_meta = fs::symlink_metadata(source).ok();
        let (top_level_modified_at, top_level_created_at) = CopyScanResult::local_top_level_dates(top_meta.as_ref());

        per_path.push((
            source.clone(),
            CopyScanResult {
                file_count: files.len() - files_before,
                // Descendants only. `Volume::scan_for_copy` excludes the
                // top-level path from its own `dir_count`, and these results
                // are read interchangeably with its.
                dir_count: dirs.len() - dirs_before - usize::from(top_level_is_directory),
                total_bytes: *total_bytes - total_bytes_before,
                // `seen_inodes` spans all sources (a hardlink crossing two
                // selected roots counts once, matching the aggregate), so a
                // later source's share can read low when it shares an inode
                // with an earlier one. Informational only; `total_bytes` is
                // what the copy reserves and fills against.
                dedup_bytes: *dedup_bytes - dedup_bytes_before,
                top_level_is_directory,
                top_level_modified_at,
                top_level_created_at,
            },
        ));
    }
    Ok(per_path)
}

/// Walks a directory tree, collecting files and directories.
///
/// Shared walker used by both scan preview and write operation scanning.
/// Behavior is customized via `WalkContext` callbacks for error handling and progress reporting.
///
/// **Oracle reuse**: when `volume_id` is provided and the listing cache holds a
/// watcher-backed listing for the directory currently being walked, the walker
/// hydrates that level's entries from the cache instead of touching the disk.
/// See `Listings::authoritative_listing` for the full
/// freshness contract. Pass `None` for `volume_id` to opt out (no listing
/// lookup is performed, behavior is identical to the pre-oracle walker).
///
/// **Two byte totals**: `total_bytes` is the **write footprint** — every file
/// at full size, including each hardlink, because hardlinks don't survive a
/// cross-volume copy. It's what a copy actually writes and what the
/// disk-space check must reserve. `dedup_bytes` is the **`du`-equivalent
/// source footprint** — each inode counted once. It's what a delete frees and
/// what the scan-phase progress bar compares against the indexer's inode-
/// dedup'd `dir_stats` estimate (so "X% of estimated" converges to ~100% on
/// hardlink-heavy trees like cargo's `target/`). Dedup is Unix-only (non-Unix
/// has no `nlink()`), where `dedup_bytes == total_bytes`. Copy consumes
/// `total_bytes`; delete consumes `dedup_bytes`; the Copy dialog shows both.
///
/// **Depth lives on the heap** (#1541): the folders the walk is inside sit on a
/// `Vec`, outermost first, and one loop runs them, depth first in listing
/// order, with every tick where a recursion put it (a folder's after its
/// children). ❌ Never a call per level: a debug build's frame overflowed the
/// 2 MiB thread a scan runs on short of the ~511 levels a macOS path allows
/// (`transfer/volume/DETAILS.md` § "The walk keeps its depth on the heap").
#[allow(
    clippy::too_many_arguments,
    reason = "The walk's shared tallies, each threaded in by the caller that reads them"
)]
pub(super) fn walk_dir_recursive<E>(
    path: &Path,
    source_root: &Path,
    files: &mut Vec<FileInfo>,
    dirs: &mut Vec<PathBuf>,
    total_bytes: &mut u64,
    dedup_bytes: &mut u64,
    last_progress_time: &mut Instant,
    visited: &mut HashSet<PathBuf>,
    seen_inodes: &mut HashSet<u64>,
    volume_id: Option<&str>,
    ctx: &WalkContext<'_, E>,
) -> Result<(), E> {
    let mut walk = Walk {
        source_root,
        files,
        dirs,
        total_bytes,
        dedup_bytes,
        last_progress_time,
        visited,
        seen_inodes,
        volume_id,
        ctx,
    };
    let mut inside: Vec<WalkedDir> = Vec::new();
    if let Some(dir) = walk.visit(path)? {
        inside.push(dir);
    }
    while let Some(dir) = inside.last_mut() {
        match walk.next_child(dir)? {
            Some(child) => {
                if let Some(child_dir) = walk.visit(&child)? {
                    inside.push(child_dir);
                }
            }
            None => {
                let dir = inside.pop().expect("the folder that ended is on top");
                walk.finish(dir);
            }
        }
    }
    Ok(())
}

/// Everything one walk tallies into, and what steers it.
struct Walk<'w, 'c, E> {
    source_root: &'w Path,
    files: &'w mut Vec<FileInfo>,
    dirs: &'w mut Vec<PathBuf>,
    total_bytes: &'w mut u64,
    dedup_bytes: &'w mut u64,
    last_progress_time: &'w mut Instant,
    visited: &'w mut HashSet<PathBuf>,
    seen_inodes: &'w mut HashSet<u64>,
    volume_id: Option<&'w str>,
    ctx: &'w WalkContext<'c, E>,
}

/// A folder the walk is inside, and what is left of its entries.
struct WalkedDir {
    path: PathBuf,
    device: Option<DeviceId>,
    children: Children,
}

enum Children {
    /// Read off the disk, the entries the walk keeps (sidecars and junk left
    /// out). Drained when the folder opens, so its handle closes before the walk
    /// goes deeper: ❌ never a `ReadDir` held per level, which held one file
    /// descriptor per level, and a GUI-launched macOS app's soft limit is often
    /// 256, well short of the ~511 levels a path allows. A Unix `DirEntry` keeps
    /// its directory open too, so only paths are kept.
    Disk(std::vec::IntoIter<PathBuf>),
    /// From a watcher-backed listing an open pane keeps fresh: files are
    /// recorded straight from it, no I/O. Cached symlinks (`is_symlink`) are
    /// recorded as files and never descended, the disk walk's symlink policy.
    Cached(std::vec::IntoIter<FileEntry>),
}

impl<E> Walk<'_, '_, E> {
    /// What each path costs before anything below it: the cancel check, the
    /// `lstat`, and the record of a file or link. A folder passes its guards,
    /// is recorded, and comes back to be walked; its tick waits for its end.
    fn visit(&mut self, path: &Path) -> Result<Option<WalkedDir>, E> {
        let ctx = self.ctx;
        if (ctx.is_cancelled)() {
            return Err((ctx.on_cancelled)());
        }
        // After the cancel check, never before it: cancel outranks pause, so a
        // cancelled walk must never park on its way out.
        (ctx.park_while_paused)();

        let metadata = fs::symlink_metadata(path).map_err(|e| (ctx.on_io_error)(path, e))?;
        let device = device_of(&metadata);

        if metadata.is_symlink() {
            // Symlinks contribute their own (tiny) target-string length, never
            // the target's bytes. No hardlink dedup: symlinks have distinct inodes.
            *self.total_bytes += metadata.len();
            *self.dedup_bytes += metadata.len();
            self.files.push(FileInfo::new(
                path.to_path_buf(),
                self.source_root.to_path_buf(),
                &metadata,
            ));
        } else if metadata.is_file() {
            // `total_bytes` is the write footprint (every file at full size, the
            // bytes a copy actually writes and the disk-space check must reserve).
            // `dedup_bytes` is the `du`-equivalent source footprint (each inode
            // once) — what delete frees and what the scan-phase progress bar
            // compares against the inode-dedup'd index estimate. `progress_bytes`
            // mirrors `dedup_bytes` per file so the delete active phase can sum a
            // running dedup'd total. See `CopyScanResult` for the consumer split.
            let counts = first_sighting(&metadata, self.seen_inodes);
            let size = metadata.len();
            *self.total_bytes += size;
            if counts {
                *self.dedup_bytes += size;
            }
            let info = FileInfo::new(path.to_path_buf(), self.source_root.to_path_buf(), &metadata)
                .with_progress_bytes(if counts { size } else { 0 });
            self.files.push(info);
            if let Some(on_file) = ctx.on_file {
                on_file(path, size);
            }
            // The flags ride on the `lstat` above, so an armed walk pays nothing
            // extra here. Files only: a symlink's own flags say nothing about the
            // cloud file it points at, and trashing one moves the link anyway.
            if let Some(watch) = ctx.online_only {
                watch.note(path, Some(&metadata));
            }
        } else if metadata.is_dir() {
            if is_symlink_loop(path, self.visited) {
                return Err((ctx.on_symlink_loop)(path));
            }
            if let Some(on_mount_boundary) = ctx.on_mount_boundary
                && super::mount_boundary::is_mount_boundary(path, device)
            {
                return Err(on_mount_boundary(path));
            }

            if let Ok(canonical) = path.canonicalize() {
                self.visited.insert(canonical);
            }

            self.dirs.push(path.to_path_buf());

            // Oracle short-circuit: if a watcher-backed listing exists for this
            // dir, walk it from cache instead of hitting the disk. Each child
            // folder is visited like any other, which re-applies the oracle.
            let children = match self
                .volume_id
                .and_then(|vid| ctx.listings.authoritative_listing(vid, path))
            {
                Some(cached) => Children::Cached(cached.into_iter()),
                None => {
                    let entries = fs::read_dir(path).map_err(|e| (ctx.on_io_error)(path, e))?;
                    let kept: Vec<PathBuf> = entries
                        .flatten()
                        .filter(|entry| {
                            let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
                            !is_left_out(ctx, path, device, &entry.file_name(), is_directory)
                        })
                        .map(|entry| entry.path())
                        .collect();
                    Children::Disk(kept.into_iter())
                }
            };
            return Ok(Some(WalkedDir {
                path: path.to_path_buf(),
                device,
                children,
            }));
        } else {
            log::debug!("scan: skipping special file: {}", LogPrivate::path(path));
        }
        self.tick_after(path);
        Ok(None)
    }

    /// The next entry of `dir` to visit; a cached file is recorded on the way.
    /// `None` once every entry is done.
    fn next_child(&mut self, dir: &mut WalkedDir) -> Result<Option<PathBuf>, E> {
        let ctx = self.ctx;
        match &mut dir.children {
            Children::Disk(entries) => Ok(entries.next()),
            Children::Cached(cached) => {
                for entry in cached.by_ref() {
                    if (ctx.is_cancelled)() {
                        return Err((ctx.on_cancelled)());
                    }
                    (ctx.park_while_paused)();
                    if is_left_out(
                        ctx,
                        &dir.path,
                        dir.device,
                        OsStr::new(&entry.name),
                        entry.is_directory && !entry.is_symlink,
                    ) {
                        continue;
                    }
                    let child_path = PathBuf::from(&entry.path);
                    if entry.is_directory && !entry.is_symlink {
                        return Ok(Some(child_path));
                    }
                    self.record_cached_file(&entry, child_path);
                }
                Ok(None)
            }
        }
    }

    /// Records a file or symlink from the cache, no I/O. We can't build a full
    /// `FileInfo` (no `std::fs::Metadata`), but we have everything the
    /// scan-preview caller actually consumes: path, size, the symlink flag, and
    /// inode for hardlink dedup.
    fn record_cached_file(&mut self, entry: &FileEntry, child_path: PathBuf) {
        let ctx = self.ctx;
        let size = entry.size.unwrap_or(0);
        // `total_bytes` (write footprint) counts every entry at full size.
        // `dedup_bytes` (source footprint) counts each inode once: when the
        // backend supplied an inode (`LocalPosixVolume` populates it for
        // files with `nlink > 1`), only the first occurrence contributes.
        // Backends without inode info leave `inode = None` so every entry
        // is unique. `progress_bytes` mirrors `dedup_bytes` per file.
        let counts = match entry.inode {
            Some(ino) => self.seen_inodes.insert(ino),
            None => true,
        };
        let progress_bytes = if counts { size } else { 0 };
        *self.total_bytes += size;
        if counts {
            *self.dedup_bytes += size;
        }
        if let Some(on_file) = ctx.on_file
            && !entry.is_symlink
        {
            on_file(&child_path, size);
        }
        // A cached entry carries no `st_flags` (the listing doesn't read
        // them), so the probe has to stat this one itself. That's the price
        // of asking the question at all in an oracle-served directory, and
        // only a cloud-drive scan ever pays it.
        if let Some(watch) = ctx.online_only
            && !entry.is_symlink
        {
            watch.note(&child_path, None);
        }
        self.files.push(FileInfo {
            path: child_path,
            source_root: self.source_root.to_path_buf(),
            size,
            progress_bytes,
            is_symlink: entry.is_symlink,
            tree_node: None,
        });
    }

    /// Every entry of `dir` is done: a cached level ticks for itself, then the
    /// folder ticks as any path does once it's walked.
    fn finish(&mut self, dir: WalkedDir) {
        if matches!(dir.children, Children::Cached(_))
            && crate::rt::since(*self.last_progress_time) >= self.ctx.progress_interval
        {
            let current_dir = Some(dir.path.display().to_string());
            // Dedup'd running total — see the matching note in `tick_after`.
            (self.ctx.on_progress)(self.files.len(), self.dirs.len(), *self.dedup_bytes, None, current_dir);
            *self.last_progress_time = crate::rt::now();
        }
        self.tick_after(&dir.path);
    }

    /// The progress tick once `path` (and everything below it) is walked.
    fn tick_after(&mut self, path: &Path) {
        if crate::rt::since(*self.last_progress_time) >= self.ctx.progress_interval {
            let current_file = path.file_name().map(|n| n.to_string_lossy().to_string());
            let current_dir = path.parent().map(|p| p.display().to_string());
            // Report the dedup'd running total: the scan-phase bar compares this
            // against the index's inode-dedup'd estimate, so reporting the raw
            // write footprint would overshoot 100% on hardlink-heavy trees.
            (self.ctx.on_progress)(
                self.files.len(),
                self.dirs.len(),
                *self.dedup_bytes,
                current_file,
                current_dir,
            );
            *self.last_progress_time = crate::rt::now();
        }
    }
}

/// Whether the walk leaves out `name`, listed in `dir`: an OS junk file the
/// transfer skips ([`WalkContext::os_junk`]), or an AppleDouble sidecar the
/// kernel already serves as its sibling's xattrs ([`WalkContext::sidecars`]).
fn is_left_out<E>(
    ctx: &WalkContext<'_, E>,
    dir: &Path,
    dir_device: Option<DeviceId>,
    name: &OsStr,
    is_directory: bool,
) -> bool {
    if ctx.os_junk.leaves_out(name, is_directory) {
        return true;
    }
    let sidecar = ctx.sidecars.is_sidecar(dir, dir_device, name);
    if sidecar {
        log::trace!(
            "scan: leaving out AppleDouble sidecar {}",
            LogPrivate::file_path(&dir.join(name))
        );
    }
    sidecar
}
