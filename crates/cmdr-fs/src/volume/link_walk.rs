//! Whether a walk that follows links into folders may go into the next one: the
//! policy every such walk shares, on top of [`Descent`]'s path identity.
//!
//! A walk that copies what a link points at (the cross-volume copy onto a
//! destination that can't hold links, a drag-out to Finder) meets an endless
//! tree when a link leads to its own folder or one above it: each pass copies the
//! same folder one level deeper, until the kernel's limit on links per path or a
//! full disk ends it (ERR-BGEKC). A Samba share lists such a link as a plain
//! folder, so the walk asks about every folder it descends, ❌ never only the ones
//! its listing marks. `crate::volume::descent` holds the path identity; this
//! module decides with it, and adds the backstop for a source that names no
//! nodes. Each walk keeps a [`WalkPath`] on its own frames, so a check costs one
//! call however deep the walk is. `cmdr-fs/DETAILS.md` § "A walk never loops
//! through a link it can't see".

use crate::app_path::{AppPath, PathArithmetic};
use crate::file_identity::FileId;
use crate::log_private::LogPrivate;
use crate::os_error::OsErrorClass;
use crate::volume::descent::{Descent, Refusal};
use crate::volume::{EntryKind, Volume, VolumeError};

/// The most links one path may have followed before following one more: past
/// it, the walk is a loop the source can't name (ERR-BGEKC follow-up).
///
/// The bound is what a loop gets to write, ❌ not a path limit: each pass through
/// one copies the same folder once more. Depth costs a heap-held walk no stack,
/// so a path limit (macOS's 1,024-byte `PATH_MAX` allows ~512 levels) would let a
/// loop copy hundreds of passes before it stopped. A link to its own parent adds
/// two levels a pass, so eight stops the shortest loop near 16 levels, with room
/// for one a few levels long. No real tree follows eight links down a single path.
pub const MAX_LINKS_FOLLOWED_DOWN_ONE_PATH: usize = 8;

/// Where a folder sits on the walk's path: the folders above it by node, and how
/// many links the walk followed below its root to reach it.
#[derive(Debug, Clone)]
pub struct WalkPath {
    descent: Descent<FileId>,
    links_below_root: usize,
}

/// Whether to descend into a child folder.
pub enum Descending {
    /// Into it, at this place on the path.
    Into(WalkPath),
    /// It's a link back to a folder the walk is already inside: leave it out.
    LoopsBack,
}

/// What a walk learns once about its root: the links at the root and above it,
/// asked only when the first marked link needs the count.
// DEFAULT-OK: nothing known yet is the honest start of every walk.
#[derive(Default)]
pub struct RootLinks(Option<usize>);

/// The path at the walk's root `root`: its own node and every folder's above it
/// up to the volume's root, each read through its own path (a link on the way
/// is followed). One call per folder there, once per walk; none on a source that
/// names no nodes.
pub async fn walk_root(volume: &dyn Volume, root: &AppPath) -> WalkPath {
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
/// (`marked_link`: the listing calls it a link), on a walk rooted at `root`.
///
/// `LoopsBack` when the folder it reaches is one the walk is already inside,
/// by node (`Volume::reached_node`, one call), which catches a link straight
/// back up and a pair leading to each other's folders. Where the listing didn't
/// mark the folder a link (a Samba share lists one as a plain folder), the node
/// match is confirmed by `entry_kind` before the folder is left out: a share
/// spanning filesystems can repeat a node number, and a real folder is never
/// dropped on a guess.
///
/// `Err`, the link-loop error (an `IoError` carrying `OsErrorClass::LinkLoop`'s
/// code): more than [`MAX_LINKS_FOLLOWED_DOWN_ONE_PATH`] links on the way, or
/// (on a source whose listings hide links) a folder with no node past the
/// shared depth backstop. The backstops for a source with no node ids (an SFTP
/// server), whose loops the node check can't see.
pub async fn descend_into(
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
        // number), walked like any other.
        Err(Refusal::LinkLoop) => here
            .descent
            .child_seen_through(None)
            .unwrap_or_else(|_| here.descent.clone()),
        Err(Refusal::TooDeep) => {
            log::info!(
                "walk: stopping at {}, deeper than the walk backstop with no node to check",
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
                "walk: stopping at {}, links followed down one path={links_on_the_way}, a loop this source can't name",
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
