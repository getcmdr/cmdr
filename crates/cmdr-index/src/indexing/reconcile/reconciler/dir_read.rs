//! Reading one directory's children off the local filesystem, for the reconcile
//! walks.
//!
//! Both local walks come through here — the small-scope live `reconcile_subtree`
//! and the full-tree `local_reconcile` rescan — so the two share one definition of
//! what belongs in a listing and one set of exclusion gates.

use crate::indexing::paths::app_text::{native_name, native_text};
use std::path::Path;

use cmdr_fs::log_private::LogPrivate;

use crate::indexing::IndexPathSpace;
use crate::indexing::metadata::{MetadataSnapshot, extract_metadata};
use crate::indexing::scanner;

/// One filesystem child of a reconciled directory: its name, its classification
/// without following symlinks, and its metadata snapshot. Carrying the snapshot
/// (rather than a `std::fs::Metadata`) is what lets the read come from a batched
/// `getattrlistbulk`, which can't synthesize a `Metadata`.
pub(crate) struct FsChild {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub snap: MetadataSnapshot,
}

/// One directory's children, and whether the read saw all of them.
///
/// The distinction is load-bearing for deletes: `diff_dir_against_db` reaps every DB
/// row the live listing lacks, so a listing that silently came back SHORT would reap
/// rows for children that are really there. ❌ Never delete against an incomplete
/// listing; still upsert what it did see.
pub(crate) struct Listing {
    pub children: Vec<FsChild>,
    /// False when an entry-iteration or a child stat failed for any reason other
    /// than the child being gone, so this is a SUBSET of what the directory holds.
    pub complete: bool,
}

/// Whether a failed child stat means the child is genuinely ABSENT — removed between
/// `readdir` and `stat` — rather than simply unobserved.
///
/// Typed on the errno, ❌ never on the message. A race with a delete is the common,
/// harmless case and keeps the listing complete; anything else (a permission wall, an
/// I/O error, a drive that went away mid-listing) means we didn't get to look.
pub(crate) fn child_is_absent(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

/// Does this child survive the exclusion gates? Both reads run it, so the two
/// share one definition of what belongs in the listing.
///
/// - `scanner::should_exclude` (system/virtual prefixes, `/Volumes/`, the E2E
///   allowlist), and
/// - `scanner::is_canonicalization_alias` — the macOS root symlinks `/tmp`, `/var`,
///   `/etc` normalize to `/private/...`, so they're aliases of a real directory the
///   fresh scan stored under the canonical path. The fresh scan skips the alias
///   (`scanner::run_scan`); a reconcile that DIDN'T would find them absent from the
///   DB and re-add them every pass, diverging from the fresh-scan tree.
///   Skipping here keeps fresh and reconcile in lock-step.
fn child_is_indexable(dir_path: &Path, name: &str, space: &IndexPathSpace) -> bool {
    let child_path = dir_path.join(name);
    let child_path_str = native_text(&child_path);
    // The canonical absolute child path (firmlink-normalized only for the boot
    // disk); the scope comes from the volume kind so a mount-rooted drive skips
    // only junk basenames, not its own `/Volumes/X` subtree.
    let normalized_child = space.absolute(&child_path_str);
    !scanner::should_exclude(&normalized_child, space.exclusion_scope())
        && !scanner::is_canonicalization_alias(&child_path_str, &normalized_child)
}

/// Read and filter filesystem children of a directory.
///
/// Shared by both local reconcile walks — the small-scope live `reconcile_subtree`
/// and the full-tree [`local_reconcile`](crate::indexing::reconcile::local_reconcile) rescan.
/// Returns `None` when the directory itself can't be listed (a permission wall or a
/// vanished dir), distinct from `Some(vec![])` for an empty-but-readable dir.
///
/// On macOS the read is the fresh scan's batched `getattrlistbulk`
/// (`scanner::bulk_read_dir_unwatched`), which delivers each child's attributes
/// with the directory entry itself. The per-entry `lstat` it replaces is the
/// dominant cost of the serial rescan walk: over 771k directories / 6.6M entries on
/// a boot disk, `readdir` cost 106.3 s against 238.4 s for the `lstat`s, ~36 µs per
/// entry and 69% of the read time, at ~10% CPU (verified on macOS 15 with a
/// standalone single-threaded walk mimicking this function, 2026-07-27).
pub(crate) fn read_fs_children(dir_path: &Path, space: &IndexPathSpace) -> Option<Listing> {
    #[cfg(target_os = "macos")]
    {
        match scanner::bulk_read_dir_unwatched(dir_path) {
            // The whole-listing fallback, and the reason a read in doubt (an
            // unusable record, an interrupted stream) is reported at all:
            // `diff_dir_against_db` DELETES every index row the live listing
            // lacks, so a child the read missed would be reaped from the index. A
            // short listing is never diffed — re-read the directory the slow,
            // complete way instead.
            Ok(read) => match read.doubt() {
                Some(doubt) => scanner::log_bulk_reread("reconcile", dir_path, doubt),
                None => return Some(bulk_children(dir_path, space, read)),
            },
            Err(e) => {
                // TRACE: this is the EXPECTED race with whatever is writing the
                // tree (a compiler deleting its target dir mid-walk), not a
                // diagnosis, and it fired ~750 times an hour. The caller counts
                // it into `ReconcileSummary::unreadable_dirs`, which is what
                // reaches the bundle.
                log::trace!("reconcile: can't read {}: {e}", LogPrivate::dir_path(&dir_path));
                return None;
            }
        }
    }
    read_fs_children_via_read_dir(dir_path, space)
}

/// Turn a bulk read into [`FsChild`]s, applying the exclusion gates and stating
/// the entries the reader couldn't attribute (see `scanner`'s `bulk_read`). An
/// entry that vanished between the read and its fallback stat is dropped, exactly
/// as `read_dir`'s own stat-failure branch drops it.
#[cfg(target_os = "macos")]
fn bulk_children(dir_path: &Path, space: &IndexPathSpace, read: scanner::BulkDirRead) -> Listing {
    let mut children = Vec::with_capacity(read.entries.len());
    let mut complete = true;
    for entry in read.entries {
        // Name it exactly as the `read_dir` path does — lossily, from the path — so
        // the two reads agree on a non-UTF-8 name instead of one of them indexing a
        // child the other can't.
        let name = entry
            .path
            .file_name()
            .map(|n| native_name(n).into_owned())
            .unwrap_or_default();
        if !child_is_indexable(dir_path, &name, space) {
            continue;
        }
        let (is_dir, is_symlink, snap) = match entry.stat {
            Some(s) => {
                let is_dir = entry.file_type == scanner::RawFileType::Dir;
                let is_symlink = entry.file_type == scanner::RawFileType::Symlink;
                let snap = crate::indexing::metadata::metadata_from_raw(
                    s.logical_size,
                    s.physical_size,
                    s.modified_at,
                    s.inode,
                    s.nlink,
                    is_dir,
                    is_symlink,
                );
                (is_dir, is_symlink, snap)
            }
            // No inline attributes for this entry (a missing size attr, or a type
            // with no sizes at all): pay the stat for this one child. Its
            // classification comes from the stat too, not from the earlier bulk
            // read — the same "the stat is the observation" rule the `read_dir`
            // path follows, so a type that changed between the two can't leave the
            // snapshot and the flags describing different things.
            None => match std::fs::symlink_metadata(dir_path.join(&name)) {
                Ok(meta) => {
                    let (is_dir, is_symlink) = (meta.is_dir(), meta.is_symlink());
                    (is_dir, is_symlink, extract_metadata(&meta, is_dir, is_symlink))
                }
                Err(e) => {
                    // Gone between the bulk read and this stat: genuinely absent, so
                    // the listing is still whole. Anything else means we never saw
                    // this child, and a delete pass must not treat that as "gone".
                    if !child_is_absent(&e) {
                        complete = false;
                    }
                    continue;
                }
            },
        };
        children.push(FsChild {
            name,
            is_dir,
            is_symlink,
            snap,
        });
    }
    Listing { children, complete }
}

/// The portable read: `read_dir` plus a `symlink_metadata` per child. The macOS
/// path uses it only to re-read a directory whose bulk read lost a record.
pub(super) fn read_fs_children_via_read_dir(dir_path: &Path, space: &IndexPathSpace) -> Option<Listing> {
    let read_dir = match std::fs::read_dir(dir_path) {
        Ok(rd) => rd,
        Err(e) => {
            // TRACE for the same reason as the bulk read's arm above.
            log::trace!("reconcile: can't read {}: {e}", LogPrivate::dir_path(&dir_path));
            return None;
        }
    };

    let mut children = Vec::new();
    let mut complete = true;
    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            // The iteration itself faltered, so this listing is a subset.
            Err(_) => {
                complete = false;
                continue;
            }
        };
        let name = native_name(&entry.file_name()).into_owned();
        if !child_is_indexable(dir_path, &name, space) {
            continue;
        }
        match std::fs::symlink_metadata(dir_path.join(&name)) {
            Ok(meta) => {
                let (is_dir, is_symlink) = (meta.is_dir(), meta.is_symlink());
                children.push(FsChild {
                    name,
                    is_dir,
                    is_symlink,
                    snap: extract_metadata(&meta, is_dir, is_symlink),
                });
            }
            Err(e) => {
                if !child_is_absent(&e) {
                    complete = false;
                }
            }
        }
    }
    Some(Listing { children, complete })
}
