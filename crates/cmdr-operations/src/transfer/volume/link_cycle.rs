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
use cmdr_fs::volume::Volume;
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

/// Whether the folder `link` reaches is `link`'s own folder or one above it, by
/// node (`Volume::reached_node`), each ancestor read through whatever links lie on
/// the way to it. So a second link back to a folder the walk came through is
/// caught as well as a direct `..`.
///
/// `false` when the backend can't say (no node ids), which leaves the link
/// followed as before. Ancestors stop at the volume's root: a link to a folder
/// above the volume, holding it, is beyond what the volume can name.
pub(super) async fn leads_back_up(source_volume: &dyn Volume, link: &AppPath) -> bool {
    let Some(reached) = source_volume.reached_node(link).await else {
        return false;
    };
    let mut ancestor = link.parent();
    while let Some(folder) = ancestor {
        if source_volume.reached_node(folder).await == Some(reached) {
            return true;
        }
        ancestor = folder.parent();
    }
    false
}
