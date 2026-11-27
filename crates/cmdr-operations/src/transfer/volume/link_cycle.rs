//! Whether a folder the copy would descend into is one the walk is already
//! inside, which would make the tree endless (ERR-BGEKC).
//!
//! A copy onto a destination that can't hold links follows a link to a folder
//! and copies what it points at (`DETAILS.md` § "A copy carries a link as the
//! link"). A link to its own folder, or to one above it, holds itself: each pass
//! copies the same folder one level deeper, until the kernel's limit on links per
//! path or a full disk ends it. Two links that lead to each other's folders do
//! the same with one more step. A Samba share lists such a link as a plain
//! folder, so the walk asks about every folder it descends, ❌ never only the ones
//! its listing marks. Whether to descend is the shared
//! `cmdr_fs::volume::link_walk` policy (drag-out to Finder walks with it too),
//! its `WalkPath` kept on the walk's own frames; what the COPY does with a link
//! it leaves out lives here. `DETAILS.md` § "A followed link never leads back up
//! its own path".

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_private::LogPrivate;
use std::sync::Arc;

pub(super) use cmdr_fs::volume::link_walk::{Descending, RootLinks, WalkPath, descend_into, walk_root};

use super::super::super::transfer_tree::NodeId;
use super::super::transfer_driver::SourceProgress;
use super::merge_ctx::CreatedPaths;

/// Leaves out the link `entry` at `link`, which [`descend_into`] found leads back
/// up the walk's path: one counted skip, credited as the scan counted it (one
/// entry, its listed size), and recorded so a move's sweep leaves it in the
/// source.
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
