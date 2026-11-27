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
//! its listing marks. The path identity itself is the shared
//! `cmdr_fs::volume::descent::Descent`, kept on the walk's own frames. `DETAILS.md`
//! § "A followed link never leads back up its own path".

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::file_identity::FileId;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::os_error::OsErrorClass;
use cmdr_fs::volume::descent::{Descent, Refusal};
use cmdr_fs::volume::{EntryKind, Volume, VolumeError};
use std::sync::Arc;

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

/// The most links one path may have followed before following `link` too: past
/// it, the walk is a loop the source can't name (ERR-BGEKC follow-up).
///
/// The bound is what a loop gets to write, ❌ not a path limit: each pass through
/// one copies the same folder once more. Depth costs the walk no stack
/// (`DETAILS.md` § "The walk keeps its depth on the heap"), so a path limit
/// (macOS's 1,024-byte `PATH_MAX` allows ~512 levels) would let a loop copy
/// hundreds of passes before it stopped. A link to its own parent adds two levels
/// a pass, so eight stops the shortest loop near 16 levels, with room for one a
/// few levels long. No real tree follows eight links down a single path.
const MAX_LINKS_FOLLOWED_DOWN_ONE_PATH: usize = 8;

/// Where a level sits on the walk's path: the folders above it by node, and how
/// many links the walk followed below its root to reach it. Each level holds its
/// own, so a check costs one call however deep the walk is.
#[derive(Debug, Clone)]
pub(super) struct WalkPath {
    descent: Descent<FileId>,
    links_below_root: usize,
}

/// Whether to descend into a child folder.
pub(super) enum Descending {
    /// Into it, at this place on the path.
    Into(WalkPath),
    /// It's a link back to a folder the walk is already inside: skip it.
    LoopsBack,
}

/// What the walk learns once about its root: the links at the root and above
/// it, asked only when the first marked link needs the count.
// DEFAULT-OK: nothing known yet is the honest start of every walk.
#[derive(Default)]
pub(super) struct RootLinks(Option<usize>);

/// The path at the walk's root `root`: its own node and every folder's above
/// it up to the volume's root, each read through its own path (a link on the
/// way is followed). One call per folder there, once per walk; none on a source
/// that names no nodes.
pub(super) async fn walk_root(volume: &dyn Volume, root: &AppPath) -> WalkPath {
    let mut above = Vec::new();
    let mut folder = root.parent();
    while let Some(path) = folder {
        above.push(volume.reached_node(path).await);
        folder = path.parent();
    }
    above.reverse();
    WalkPath {
        descent: Descent::root_under(above, volume.reached_node(root).await),
        links_below_root: 0,
    }
}

/// Whether to descend from `here` into the child folder at `child`
/// (`marked_link`: the listing calls it a link).
///
/// `LoopsBack` when the folder it reaches is one the walk is already inside,
/// by node (`Volume::reached_node`, one call), which catches a link straight
/// back up and a pair leading to each other's folders. Where the listing didn't
/// mark the folder a link (a Samba share lists one as a plain folder), the node
/// match is confirmed by `entry_kind` before the folder is left out: a share
/// spanning filesystems can repeat a node number, and a real folder is never
/// dropped on a guess.
///
/// `Err`, the link-loop error (`OsErrorClass::LinkLoop`, which the transfer reports
/// as `SymlinkLoop`): more than [`MAX_LINKS_FOLLOWED_DOWN_ONE_PATH`] links on the
/// way, or (on a source whose listings hide links) a folder with no node past the
/// shared depth backstop. The backstops for a source with no node ids (an SFTP
/// server), whose loops the node check can't see.
pub(super) async fn descend_into(
    volume: &dyn Volume,
    child: &AppPath,
    marked_link: bool,
    here: &WalkPath,
    root: &AppPath,
    root_links: &mut RootLinks,
) -> Result<Descending, VolumeError> {
    let node = volume.reached_node(child).await;
    let listings_hide_links = !volume.backend_kind().listings_mark_links();
    let step = if listings_hide_links {
        here.descent.child(node)
    } else {
        here.descent.child_seen_through(node)
    };
    let descent = match step {
        Ok(descent) => descent,
        Err(Refusal::LinkLoop) if marked_link || volume.entry_kind(child).await? == EntryKind::Symlink => {
            return Ok(Descending::LoopsBack);
        }
        // The same node with no link to reach it: a real folder (a repeated node
        // number), copied like any other.
        Err(Refusal::LinkLoop) => here
            .descent
            .child_seen_through(None)
            .unwrap_or_else(|_| here.descent.clone()),
        Err(Refusal::TooDeep) => {
            log::info!(
                "merge: stopping at {}, deeper than the walk backstop with no node to check",
                LogPrivate::path(child.as_str())
            );
            return Err(link_loop_error());
        }
    };
    let mut links_below_root = here.links_below_root;
    if marked_link {
        let links_on_the_way = here.links_below_root + root_links.count(volume, root).await;
        if links_on_the_way >= MAX_LINKS_FOLLOWED_DOWN_ONE_PATH {
            log::info!(
                "merge: stopping at {}, links followed down one path={links_on_the_way}, a loop this source can't name",
                LogPrivate::path(child.as_str())
            );
            return Err(link_loop_error());
        }
        links_below_root += 1;
    }
    Ok(Descending::Into(WalkPath {
        descent,
        links_below_root,
    }))
}

impl RootLinks {
    /// How many of the root and the folders above it are links (`entry_kind`,
    /// which every backend answers), asked once.
    async fn count(&mut self, volume: &dyn Volume, root: &AppPath) -> usize {
        if let Some(count) = self.0 {
            return count;
        }
        let mut count = 0;
        let mut folder = Some(root);
        while let Some(path) = folder {
            if matches!(volume.entry_kind(path).await, Ok(EntryKind::Symlink)) {
                count += 1;
            }
            folder = path.parent();
        }
        self.0 = Some(count);
        count
    }
}

fn link_loop_error() -> VolumeError {
    VolumeError::IoError {
        message: "Too many links followed down one path".into(),
        raw_os_error: OsErrorClass::LinkLoop.host_code(),
    }
}
