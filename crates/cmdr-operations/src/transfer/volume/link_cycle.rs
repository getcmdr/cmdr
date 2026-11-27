//! Whether a link the copy would follow leads back into a folder the walk is
//! already inside, which would make the tree endless (ERR-BGEKC).
//!
//! A copy onto a destination that can't hold links follows a link to a folder
//! and copies what it points at (`DETAILS.md` § "A copy carries a link as the
//! link"). A link to its own folder, or to one above it, holds itself: each pass
//! copies the same folder one level deeper, until the kernel's limit on links per
//! path or a full disk ends it. Two links that lead to each other's folders do
//! the same with one more step. `DETAILS.md` § "A followed link never leads back
//! up its own path".

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::os_error::OsErrorClass;
use cmdr_fs::volume::{EntryKind, Volume, VolumeError};
use std::sync::Arc;

use super::super::super::transfer_tree::NodeId;
use super::super::transfer_driver::SourceProgress;
use super::merge_ctx::CreatedPaths;

/// Leaves out the link `entry` at `link`, which [`leads_back_up`] said yes to: one
/// counted skip, credited as the scan counted it (one entry, its listed size), and
/// recorded so a move's sweep leaves it in the source.
pub(super) fn skip_link_back_up(
    entry: &FileEntry,
    link: AppPathBuf,
    created: &CreatedPaths,
    progress: &Arc<SourceProgress>,
    node: Option<NodeId>,
) {
    log::info!(
        "merge: leaving out {}, a link back to a folder this copy is already inside",
        LogPrivate::path(link.as_str())
    );
    let counted_bytes = entry.size.unwrap_or(0);
    created.record_skip(link, counted_bytes);
    progress.skip_leaf(counted_bytes, node);
}

/// The most links one path may have followed before following `link` too: past
/// it, the walk is a loop the source can't name (ERR-BGEKC follow-up).
///
/// The bound is the walk's STACK, ❌ not a path limit. Each folder level the
/// cross-volume walk descends costs stack, and a release build overflowed a 2 MiB
/// tokio worker (Tauri's default, which runs copies) between 48 and 63 levels deep
/// (verified on macOS 27, release `copy_volumes_with_progress` of a nested folder
/// onto `InMemoryVolume`, 2026-11-27), long before any path limit (macOS's 1,024-byte
/// `PATH_MAX` allows ~512 levels). A link to its own parent adds two levels a pass,
/// so eight keeps the shortest loop near 16 levels, with room for one a few levels
/// long. No real tree follows eight links down a single path.
const MAX_LINKS_FOLLOWED_DOWN_ONE_PATH: usize = 8;

/// Whether to follow `link` into the folder it reaches, skip it as a link back up
/// its own path, or stop the copy.
///
/// `Ok(true)`: the folder it reaches is `link`'s own folder or one above it, by
/// node (`Volume::reached_node`), each ancestor read through whatever links lie on
/// the way to it, so a second link back to a folder the walk came through is
/// caught as well as a direct `..`. Ancestors stop at the volume's root.
///
/// `Err`, the link-loop error (`OsErrorClass::LinkLoop`, which the transfer reports
/// as `SymlinkLoop`): more than [`MAX_LINKS_FOLLOWED_DOWN_ONE_PATH`] of the ancestors
/// are links. The backstop for a source with no node ids (an SFTP server), whose
/// loops the first check can't see.
pub(super) async fn leads_back_up(source_volume: &dyn Volume, link: &AppPath) -> Result<bool, VolumeError> {
    let reached = source_volume.reached_node(link).await;
    let mut links_on_the_way = 0;
    let mut ancestor = link.parent();
    while let Some(folder) = ancestor {
        if reached.is_some() && source_volume.reached_node(folder).await == reached {
            return Ok(true);
        }
        if matches!(source_volume.entry_kind(folder).await, Ok(EntryKind::Symlink)) {
            links_on_the_way += 1;
        }
        ancestor = folder.parent();
    }
    if links_on_the_way >= MAX_LINKS_FOLLOWED_DOWN_ONE_PATH {
        log::info!(
            "merge: stopping at {}, links followed down one path={links_on_the_way}, a loop this source can't name",
            LogPrivate::path(link.as_str())
        );
        return Err(VolumeError::IoError {
            message: "Too many links followed down one path".into(),
            raw_os_error: OsErrorClass::LinkLoop.host_code(),
        });
    }
    Ok(false)
}
