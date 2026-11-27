//! Drag-out file-promise fulfillment: the plain-Rust service that actually
//! downloads a virtual file (MTP/SMB) to the exact destination Finder chose.
//!
//! This module has NO AppKit dependency. The delegate in [`super::promises`]
//! drives it from the promise operation-queue thread; everything here is plain
//! async Rust that unit tests exercise with no `NSFilePromiseProvider`, no
//! `block2`, and no main-thread runloop. That split is deliberate: all the real
//! logic (volume resolution, streaming, partial-file cleanup, error→friendly
//! mapping) lives here where it's testable; the Objective-C surface stays
//! paper-thin.
//!
//! ## Sequence
//!
//! [`fulfill`] resolves the source volume from the registry, marks it busy for
//! the eject guard, notes the destination as a Cmdr-own write (so dropping a
//! phone photo into `~/Downloads` doesn't pop a spurious "Downloaded …" toast),
//! then streams the bytes:
//!
//! - **File**: `open_read_stream` → `write_from_stream(dest, …)` to the EXACT
//!   Finder-chosen leaf path. Finder uniquifies collisions ("sunset 2.jpg") and
//!   we honor that leaf, never the source basename.
//! - **Folder**: `create_dir` at the dest, then a walk (list → mkdir →
//!   per-file `write_from_stream`). The cross-volume copy engine derives landed
//!   names from source basenames and can't be pointed at a Finder-renamed root,
//!   so the walk is hand-rolled on the same per-file primitive.
//!
//! ## Cleanup contract (load-bearing)
//!
//! On ANY `Err`, the destination this fulfillment created is removed before the
//! error is returned. The "ANY `Err`" wording matters:
//! `LocalPosixVolume::write_from_stream` self-cleans its partial ONLY on the
//! cancel branch (`ControlFlow::Break`), NOT on a propagated source-read error
//! — and a source-read error (device unplugged mid-stream) is exactly the
//! promise failure mode. So a single-file fulfillment removes the partial dest
//! itself; a folder fulfillment removes the whole tree it created. The dest is
//! a fresh Finder-created path (Finder hands us a brand-new URL it just made),
//! so wholesale removal of what we created never touches pre-existing user
//! content.
//!
//! ## Main-thread invariant
//!
//! The service NEVER performs synchronous main-thread work. Volume I/O runs on
//! the tokio runtime; `note_pending_write_for_cmdr` is a cheap prefix-scoped
//! mutex (no main-thread hop, usually a no-op since Finder destinations are
//! rarely inside Downloads). The delegate calls `fulfill` from the promise
//! queue thread via `block_on`; if the service hopped synchronously to the main
//! thread there, and the main thread were itself busy or waiting, it would
//! deadlock. It doesn't, by construction — there is no `run_on_main_thread`
//! anywhere below.

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::link_walk::{Descending, RootLinks, WalkPath, descend_into, walk_root};
use std::path::{Path, PathBuf};

use crate::file_system::volume::Volume;
use crate::file_system::volume::friendly_error::{ErrorCategory, ListingError, listing_error_from_volume_error};
use crate::file_system::volume::{ChildName, VolumeError, VolumeReadStream, WriteMode};
use cmdr_fs::log_private::LogPrivate;
use cmdr_operation_log::types::OperationId;

/// A drag-out fulfillment failure, carrying the typed [`ListingError`]
/// classification so the delegate can surface a title through the promise
/// completion handler's `NSError` (Finder shows its own alert). The exact
/// title is a native-OS surface (not Cmdr's webview), so we derive a short
/// category-keyed string here rather than crossing IPC to the FE copy.
#[derive(Debug, Clone)]
pub struct FulfillError {
    /// Typed classification (category drives the NSError title).
    pub error: ListingError,
    /// Whether this was a user/system cancellation (app quit, device
    /// disconnect mid-stream surfaces as a read error, not this). The delegate
    /// maps a cancel to a Cancelled-shaped `NSError` so Finder doesn't shout.
    pub cancelled: bool,
}

impl FulfillError {
    /// Builds a `FulfillError` from a `VolumeError` and the destination path
    /// (used for provider-aware classification). The path is the DEST so the
    /// provider detector can pick the destination provider; for a source-read
    /// error the dest is still the most useful path.
    fn from_volume_error(err: &VolumeError, dest: &Path) -> Self {
        let cancelled = matches!(err, VolumeError::Cancelled(_));
        Self {
            error: listing_error_from_volume_error(err, dest),
            cancelled,
        }
    }

    /// Short, native-OS title for the Finder `NSError`. The detailed copy lives
    /// on the FE; Finder only needs a one-line summary, keyed off the category.
    pub fn nserror_title(&self) -> &'static str {
        match self.error.category {
            ErrorCategory::Transient => "Couldn't copy the item right now",
            ErrorCategory::NeedsAction => "Couldn't copy the item",
            ErrorCategory::Serious => "Couldn't copy the item",
        }
    }
}

/// The volume side of a fulfillment, abstracted so unit tests can drive the
/// service without the global `VolumeManager`. Production resolves through
/// [`RegistryResolver`]; tests pass a fixed `InMemoryVolume`. `Send + Sync`
/// because the delegate drives `fulfill` from the promise queue thread.
pub trait VolumeResolver: Send + Sync {
    /// Returns the volume that can serve `source_path` on `volume_id`, or `None`
    /// if it's gone (unmounted / disconnected since the drag started).
    ///
    /// ❗ The PATH is part of the question, not decoration. A dragged item can
    /// live in a namespace a ROUTE serves rather than on the volume the pane
    /// named: inside a `.zip`, or inside a repo's virtual `.git` trees. Resolving
    /// by id alone hands back the parent drive, whose `open_read_stream` then
    /// meets a path with no inode and the promise fails on the drop.
    fn resolve(&self, volume_id: &str, source_path: &AppPath) -> Option<std::sync::Arc<dyn Volume>>;
}

/// Production resolver: the global `VolumeManager`.
pub struct RegistryResolver;

impl VolumeResolver for RegistryResolver {
    fn resolve(&self, volume_id: &str, source_path: &AppPath) -> Option<std::sync::Arc<dyn Volume>> {
        let manager = crate::file_system::volume::manager::get_volume_manager();
        // `resolve_local_only` is the SYNC sibling that routes a local `.zip` and a
        // repo's `.git` snapshots; the async `resolve` would need this trait to be
        // async for the one case it adds (a REMOTE archive parent), whose own
        // `open_read_stream` can't serve an inner path either.
        manager
            .resolve_local_only(volume_id, source_path)
            .volume
            .or_else(|| manager.get(volume_id))
    }
}

/// What a successful fulfillment produced, so the session-summary accounting can
/// split the completion toast by kind ("Copied 2 files and 1 folder.").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FulfillOutcome {
    /// Whether the dragged item was a directory (recursively downloaded) vs a
    /// single file.
    pub is_dir: bool,
}

/// Fulfills one dragged item: downloads `source_path` from the volume identified
/// by `source_volume_id` to the exact `dest_path` Finder supplied.
///
/// Marks the source volume busy for the eject guard for the whole transfer
/// (released on every exit via an RAII guard). Returns the resolved
/// [`FulfillOutcome`] (file vs folder) on success; on any failure removes the
/// partial/created destination and returns a [`FulfillError`] carrying friendly
/// copy.
pub async fn fulfill(
    source_volume_id: &str,
    source_path: &Path,
    dest_path: &Path,
) -> Result<FulfillOutcome, FulfillError> {
    fulfill_with_resolver(&RegistryResolver, source_volume_id, source_path, dest_path).await
}

/// RAII guard that marks a volume busy on construction and releases it on drop,
/// so the eject guard clears no matter how the fulfillment exits (success,
/// error, or a panic unwinding through the `.await`s).
struct BusyGuard {
    op_id: OperationId,
}

impl BusyGuard {
    fn new(volume_id: &str) -> Self {
        let op_id = OperationId::from(format!("drag-out-{}", uuid::Uuid::new_v4()));
        // Before `install` nothing can be ejected mid-transfer either, so there's
        // nothing to guard.
        if let Ok(engine) = crate::operations_host::operations() {
            engine.register_external_volume_op(&op_id, vec![volume_id.to_string()]);
        }
        Self { op_id }
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Ok(engine) = crate::operations_host::operations() {
            engine.release_external_volume_op(&self.op_id);
        }
    }
}

/// Testable core of [`fulfill`] with an injectable volume resolver.
pub(crate) async fn fulfill_with_resolver(
    resolver: &dyn VolumeResolver,
    source_volume_id: &str,
    source_path: &Path,
    dest_path: &Path,
) -> Result<FulfillOutcome, FulfillError> {
    // The drag's source is the `std::path` value the drag session carries: crossed
    // once, for the route and the kind question both.
    let source = AppPath::legacy_unchecked(source_path);
    let Some(volume) = resolver.resolve(source_volume_id, &source) else {
        // The source vanished between drag-start and fulfillment.
        let err = VolumeError::DeviceDisconnected(format!("Volume '{source_volume_id}' is no longer available").into());
        return Err(FulfillError::from_volume_error(&err, dest_path));
    };

    // Busy-guard the source for the whole transfer (eject guard). Released on
    // every exit path below via Drop.
    let _busy = BusyGuard::new(source_volume_id);

    // Suppress the downloads watcher for this destination (a no-op unless dest
    // is inside ~/Downloads). Cheap mutex, never a main-thread hop.
    crate::downloads::note_pending_write_for_cmdr(dest_path);

    log::info!(
        target: "drag_out",
        "fulfilling promise: {} {} -> {}",
        source_volume_id,
        LogPrivate::path(source_path),
        LogPrivate::path(dest_path)
    );

    let is_dir = volume
        .is_directory(&source)
        .await
        .map_err(|e| FulfillError::from_volume_error(&e, dest_path))?;

    let result = if is_dir {
        fulfill_directory(volume.as_ref(), source_path, dest_path).await
    } else {
        fulfill_file(volume.as_ref(), source_path, dest_path).await
    };

    if let Err(ref err) = result {
        log::warn!(
            target: "drag_out",
            "promise fulfillment failed for {} -> {}: {}",
            LogPrivate::path(source_path),
            LogPrivate::path(dest_path),
            err.nserror_title()
        );
    }
    result.map(|()| FulfillOutcome { is_dir })
}

/// Streams one source file to the EXACT `dest_path`. On any error, removes the
/// partial destination (the local writer leaves it on a read error — see the
/// module-level cleanup contract).
async fn fulfill_file(volume: &dyn Volume, source_path: &Path, dest_path: &Path) -> Result<(), FulfillError> {
    let result = stream_one_file(volume, source_path, dest_path).await;
    if result.is_err() {
        // Best-effort: remove whatever partial landed at the Finder-chosen path.
        let _ = remove_file_best_effort(dest_path).await;
    }
    result
}

/// The happy-path stream of one file: open the source reader, write it to the
/// destination, no cleanup (the caller owns cleanup on error).
async fn stream_one_file(volume: &dyn Volume, source_path: &Path, dest_path: &Path) -> Result<(), FulfillError> {
    let source = AppPath::legacy_unchecked(source_path);
    let size_hint = volume.get_metadata(&source).await.ok().and_then(|m| m.size);

    let stream: Box<dyn VolumeReadStream> = volume
        .open_read_stream(&source)
        .await
        .map_err(|e| FulfillError::from_volume_error(&e, dest_path))?;
    let size = match size_hint {
        Some(size) => size,
        None => stream
            .total_size()
            .known()
            .ok_or_else(|| FulfillError::from_volume_error(&VolumeError::NotSupported, dest_path))?,
    };

    // No cancel from this path in v1 (Finder owns the gesture, no progress UI);
    // always Continue. App-quit / device-disconnect aborts arrive as the source
    // stream dropping mid-flight or `next_chunk` erroring, handled by cleanup.
    let on_progress = &|_: crate::file_system::volume::StreamWriteProgress| std::ops::ControlFlow::<()>::Continue(());

    write_to_local_dest(dest_path, size, stream, on_progress)
        .await
        .map_err(|e| FulfillError::from_volume_error(&e, dest_path))?;
    Ok(())
}

/// Writes a source stream to a LOCAL destination path. The destination is
/// always a local FS path (Finder hands us a `file://` URL on the user's disk),
/// so we resolve through the local-FS write primitive directly rather than the
/// VolumeManager: the dest "volume" is whatever local disk Finder picked, and
/// `LocalVolume` rooted at `/` writes to any absolute local path.
async fn write_to_local_dest(
    dest_path: &Path,
    size: u64,
    stream: Box<dyn VolumeReadStream>,
    on_progress: &(dyn Fn(crate::file_system::volume::StreamWriteProgress) -> std::ops::ControlFlow<()> + Sync),
) -> Result<u64, VolumeError> {
    let local = cmdr_local::LocalVolume::local_folder("Local", PathBuf::from("/"), crate::volume_host::host());
    // Finder created `dest_path` as a placeholder for us to fill, so it's ours
    // to replace.
    let written = local
        .write_from_stream(
            &AppPath::legacy_unchecked(dest_path),
            WriteMode::CreateOrReplace,
            crate::file_system::volume::StreamLength::Known(size),
            stream,
            on_progress,
        )
        .await?;
    // The file's device barrier before Finder hears it's done: the write only
    // handed it to the drive (`Volume::flush_writes`).
    local.flush_writes().await?;
    Ok(written.bytes)
}

/// Recursively downloads a source directory into the Finder-created `dest_path`.
///
/// Creates the dest dir, then walks the source (list → mkdir per subdir →
/// per-file stream). On any error, removes the ENTIRE created tree
/// (`remove_dir_all`): the dest is a freshly Finder-created directory, so
/// wholesale removal of what we created can't touch pre-existing user content.
async fn fulfill_directory(volume: &dyn Volume, source_path: &Path, dest_path: &Path) -> Result<(), FulfillError> {
    let result = populate_directory(volume, source_path, dest_path).await;
    if result.is_err() {
        let _ = remove_dir_all_best_effort(dest_path).await;
    }
    result
}

/// The happy-path populate. No cleanup (the caller removes the whole created
/// tree on error).
///
/// The folders the walk is inside live on a `Vec`, outermost first, and one
/// loop runs them all, depth first in listing order: ❌ never a future per level.
/// This polls on the promise queue's own thread (`block_on`), a GCD thread with
/// a 512 KiB stack, which a future per level overflowed on a deep enough folder
/// (#1541). Every folder it descends asks `cmdr_fs::volume::link_walk` first, as
/// the copy walk does: a listing's `is_directory` follows links on some backends
/// (a Samba share lists `loop -> ..` as a plain folder), and following one back
/// up its own path would write the same folder into Finder's again and again. A
/// folder that leads back up is left out, with nothing created for it.
async fn populate_directory(volume: &dyn Volume, source_path: &Path, dest_path: &Path) -> Result<(), FulfillError> {
    let mut walk = DragWalk {
        volume,
        root: None,
        root_links: RootLinks::default(),
    };
    let mut inside: Vec<DragFolder> = Vec::new();
    let mut opening = Some(FolderToOpen {
        source: source_path.to_path_buf(),
        dest: dest_path.to_path_buf(),
        reached: Reached::Root,
    });
    loop {
        if let Some(next) = opening.take()
            && let Some(folder) = walk.open(next).await?
        {
            inside.push(folder);
        }
        let Some(folder) = inside.last_mut() else {
            return Ok(());
        };
        let Some(entry) = folder.entries.next() else {
            inside.pop();
            continue;
        };
        // ❗ The phone or server named this child: `../x` or `/x` joined raw
        // would write outside the folder Finder made.
        let name = ChildName::new(&entry.name).map_err(|e| FulfillError::from_volume_error(&e.into(), &folder.dest))?;
        let child_source = name.under(&folder.source);
        let child_dest = name.under(&folder.dest);
        if entry.is_directory {
            opening = Some(FolderToOpen {
                source: child_source,
                dest: child_dest,
                reached: Reached::Below {
                    parent: folder.path.clone(),
                    marked_link: entry.is_symlink,
                },
            });
        } else {
            stream_one_file(volume, &child_source, &child_dest).await?;
        }
    }
}

/// What every folder of one drag-out walk reads.
struct DragWalk<'v> {
    volume: &'v dyn Volume,
    /// The walk's root as the volume spells it, once its folder is open.
    root: Option<AppPathBuf>,
    root_links: RootLinks,
}

/// A source folder the walk is about to open, and the Finder-side folder it fills.
struct FolderToOpen {
    source: PathBuf,
    dest: PathBuf,
    reached: Reached,
}

/// How the walk got to a folder: it's the root, or a child its parent listed.
enum Reached {
    Root,
    Below {
        parent: WalkPath,
        /// The parent's listing called it a link.
        marked_link: bool,
    },
}

/// One folder the walk is inside: what is left of its listing, and where it sits.
struct DragFolder {
    source: PathBuf,
    dest: PathBuf,
    path: WalkPath,
    entries: std::vec::IntoIter<FileEntry>,
}

impl DragWalk<'_> {
    /// Asks whether to go into `folder` (`None`: it leads back up the walk's
    /// path, so it's left out), then creates its Finder-side folder and lists it.
    async fn open(&mut self, folder: FolderToOpen) -> Result<Option<DragFolder>, FulfillError> {
        let FolderToOpen { source, dest, reached } = folder;
        let source_app = AppPath::legacy_unchecked(&source);
        let path = match reached {
            Reached::Below { parent, marked_link } => {
                let root = self.root.as_ref().expect("the root opens before any folder below it");
                let descending = descend_into(
                    self.volume,
                    &source_app,
                    marked_link,
                    &parent,
                    root,
                    &mut self.root_links,
                )
                .await
                .map_err(|e| FulfillError::from_volume_error(&e, &dest))?;
                match descending {
                    Descending::Into(path) => path,
                    Descending::LoopsBack => {
                        log::info!(
                            target: "drag_out",
                            "leaving out {}, a link back to a folder this drag is already inside",
                            LogPrivate::path(&source)
                        );
                        return Ok(None);
                    }
                }
            }
            Reached::Root => {
                let path = walk_root(self.volume, &source_app).await;
                self.root = Some(source_app.clone().into_owned());
                path
            }
        };
        create_local_dir(&dest)
            .await
            .map_err(|e| FulfillError::from_volume_error(&e, &dest))?;
        let entries = self
            .volume
            .list_directory(&source_app, None)
            .await
            .map_err(|e| FulfillError::from_volume_error(&e, &dest))?;
        Ok(Some(DragFolder {
            source,
            dest,
            path,
            entries: entries.into_iter(),
        }))
    }
}

/// Creates a single local directory (the dest tree's leaf-by-leaf mkdir). Uses
/// `create_dir_all` so an intermediate level that an earlier recursion already
/// made doesn't error.
async fn create_local_dir(dest_path: &Path) -> Result<(), VolumeError> {
    let dest = dest_path.to_path_buf();
    let dest_for_error = dest.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&dest))
        .await
        .map_err(|e| VolumeError::IoError {
            message: e.to_string().into(),
            raw_os_error: None,
        })?
        .map_err(|e| VolumeError::from_io_at(&e, &dest_for_error))
}

/// Best-effort removal of a partial destination file. Never fails the caller.
async fn remove_file_best_effort(dest_path: &Path) -> std::io::Result<()> {
    let dest = dest_path.to_path_buf();
    match tokio::task::spawn_blocking(move || std::fs::remove_file(&dest)).await {
        Ok(r) => r,
        Err(_) => Ok(()),
    }
}

/// Best-effort removal of the entire created destination tree. Safe because the
/// dest is a fresh Finder-created directory (no pre-existing user content).
async fn remove_dir_all_best_effort(dest_path: &Path) -> std::io::Result<()> {
    let dest = dest_path.to_path_buf();
    match tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dest)).await {
        Ok(r) => r,
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
#[path = "fulfillment_test.rs"]
mod tests;

#[cfg(test)]
#[path = "fulfillment_walk_test.rs"]
mod walk_tests;
