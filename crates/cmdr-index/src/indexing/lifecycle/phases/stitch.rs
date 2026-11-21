//! The shallow stitch: the one thing that makes phases compose.
//!
//! A cover walk marks only the directories it READS. Bootstrap creates the
//! ancestor chain at `listed_epoch = 0` and claims nothing, and the coverage
//! descent cuts at the first unlisted directory without descending past it. So
//! after a phase covers `~/Downloads`, `coverage("$HOME")` still answers
//! `["$HOME"]`: the frontier for an ancestor scope never shrinks on its own, and a
//! later phase would re-walk everything the earlier ones covered — over rows that
//! already exist, which is the `ScanError::NotVirgin` refusal and the serial
//! repair behind it.
//!
//! So each phase is preceded by this: read each ancestor of the phase root, upsert
//! what is in it, and mark THAT ONE DIRECTORY listed. No descent, no recursion. It
//! is honest (we really did list those directories) and cheap (a handful of
//! `readdir`s). Afterwards the descent walks THROUGH the stitched ancestors and
//! cuts at each genuinely unlisted child, so a covered subtree is skipped, every
//! frontier root is virgin, and a big phase becomes many small walks instead of
//! one huge one.
//!
//! ## Three things it must keep doing
//!
//! 1. **Upsert files, not only directories.** `listed_children_on` serves a
//!    directory's rows as its FULL contents the moment `listed_epoch` is non-zero,
//!    and `Index::list_children` feeds the agent-facing `list_dir` tool. A
//!    directories-only stitch would report a folder as holding no files, that same
//!    instant.
//! 2. **Flush between the upserts and the mark.** `MarkDirsListed` is a PK-keyed
//!    `UPDATE`, so marking a row still pending in an unflushed batch leaves it at
//!    `listed_epoch = 0` forever. The stitch creates the deeper ancestor rows
//!    itself, so this is mandatory rather than an optimization.
//! 3. **Stamp the CURRENT epoch, never a new one.** A stitch is a listing, exactly
//!    like a walk's; bumping would make every previously covered row read stale.
//!
//! ❌ It never marks a directory it couldn't read. Recording ground no walk could
//! read is the WALK's job (`UnreadableCause::Abandoned`), and a stitch that marked
//! an unreadable directory listed would claim a listing it never made — which
//! `min_subtree_epoch` then absorbs all the way up to `/`.

use crate::indexing::paths::app_text::native_text;
use std::path::Path;

use cmdr_fs::log_private::LogPrivate;
use rusqlite::Connection;

use crate::indexing::IndexPathSpace;
use crate::indexing::deletes;
use crate::indexing::hold::VolumeWork;
use crate::indexing::reconcile::reconciler::{self, LiveChild, MissingRows};
use crate::indexing::store::{IndexStore, ROOT_ID, resolve_path};
use crate::indexing::writer::{IndexWriter, WriteMessage};

/// Stitch every ancestor of `path` from the volume root down, then `path` itself.
///
/// Top-down on purpose: each directory's row is created by the stitch of its
/// parent, so the chain resolves as it goes and nothing needs bootstrapping.
/// Reports the id of `path` when it ends up stitched (or already listed).
pub(super) fn down_to(space: &IndexPathSpace, writer: &IndexWriter, path: &Path, work: &VolumeWork) -> Option<i64> {
    let mut chain: Vec<&Path> = path.ancestors().skip(1).collect();
    chain.reverse();
    for ancestor in chain {
        directory(space, writer, ancestor, work);
    }
    directory(space, writer, path, work)
}

/// Read one directory, upsert everything in it, and mark that directory alone
/// listed. Reports its id, or `None` when the index has no row to stitch onto.
pub(super) fn directory(space: &IndexPathSpace, writer: &IndexWriter, dir: &Path, work: &VolumeWork) -> Option<i64> {
    let db_path = writer.db_path();
    let conn = IndexStore::open_read_connection(&db_path)
        .inspect_err(|e| {
            log::warn!(
                "Phases: can't read the index to stitch {}: {e}",
                LogPrivate::dir_path(dir)
            )
        })
        .ok()?;
    let absolute = space.absolute(&native_text(dir));
    let id = resolve(space, &conn, &absolute)?;
    // Already listed by a walk, or by an earlier phase's stitch. Re-listing it
    // would cost a `readdir` to write the same rows, and the walk that covered it
    // is a better answer than ours anyway.
    if IndexStore::get_listed_epoch_by_id(&conn, id)
        .ok()
        .flatten()
        .unwrap_or(0)
        > 0
    {
        return Some(id);
    }
    let db_children = IndexStore::list_children_on(id, &conn).unwrap_or_default();
    drop(conn);

    let Some(listing) = reconciler::read_fs_children(Path::new(&absolute), space) else {
        log::debug!("Phases: couldn't read {absolute} while stitching, so it stays unlisted");
        return Some(id);
    };
    // ⚠️ Presence AFTER the read, ❌ never before: an unmounted `/Volumes/X` whose
    // mount-point folder survives lists as empty and complete.
    let drive_listed = work.drive_is_listed();
    if drive_listed {
        // Also the presence half of the delete generation's reset.
        deletes::drive_seen(work.volume_id());
    } else {
        // And the batches already sent are the ones nothing can take back, so the
        // index says so on disk.
        deletes::note_the_drive_left(work.volume_id(), writer);
    }
    let missing = if listing.complete && drive_listed {
        MissingRows::Delete
    } else {
        MissingRows::Keep
    };
    let live: Vec<LiveChild> = listing
        .children
        .into_iter()
        .map(|child| LiveChild {
            name: child.name,
            is_directory: child.is_dir,
            is_symlink: child.is_symlink,
            // Null the inode on FAT/exFAT, so a value this writes can never feed a
            // false rename match. Every local write path funnels through this.
            snap: crate::indexing::metadata::MetadataSnapshot {
                inode: space.trust_inode(child.snap.inode),
                ..child.snap
            },
        })
        .collect();
    let diff = reconciler::diff_dir_against_db(id, &live, &db_children, missing, writer);
    if diff.removed > 0 {
        deletes::batch_sent(work.volume_id());
    }

    // Mandatory, not an optimization: see the module docs.
    if let Err(e) = writer.flush_blocking() {
        log::warn!("Phases: the stitched children of {absolute} may not have landed: {e}");
        return Some(id);
    }
    let conn = IndexStore::open_read_connection(&db_path).ok()?;
    let epoch = IndexStore::read_current_epoch(&conn).unwrap_or(1);
    drop(conn);
    let _ = writer.send(WriteMessage::MarkDirsListed { ids: vec![id], epoch });
    // So the coverage query that follows sees the mark. A stitch is a handful of
    // directories per phase, so this pays for itself in one query.
    if let Err(e) = writer.flush_blocking() {
        log::warn!("Phases: the stitch mark for {absolute} may not have landed: {e}");
    }
    Some(id)
}

/// Resolve an absolute path to its entry id, the space's own root being the
/// sentinel.
///
/// ❌ The volume-root shortcut can't come before `index_relative`: on a
/// mount-rooted space that would root a path from OUTSIDE the volume at `ROOT_ID`,
/// and the stitch would invent another drive's top level inside this index.
fn resolve(space: &IndexPathSpace, conn: &Connection, absolute: &str) -> Option<i64> {
    let index_path = space.index_relative(absolute)?;
    if index_path == "/" || index_path.is_empty() {
        return Some(ROOT_ID);
    }
    resolve_path(conn, &index_path).ok().flatten()
}
