//! A snapshot's trees as listings: rustic tree nodes to `FileEntry` rows, and a
//! path inside a snapshot to its node.
//!
//! The walks run on a blocking thread over an [`IndexedRepo`], one tree blob
//! per folder. A name that isn't in the snapshot is `Ok(None)`, which the volume
//! answers as `NotFound`, ❌ never a damaged-repo error.

use crate::path::child_text;
use crate::repo::{IndexedRepo, ResticError};
use cmdr_fs::app_path::{AppNameBuf, AppPath};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::VolumeError;
use rustic_core::TreeId;
use rustic_core::repofile::{Node, NodeType, Tree};

/// The POSIX permission bits of a node's stored mode.
///
/// Restic stores Go's `os.FileMode`, and rustic hands it through raw (its own
/// mapper isn't public). Only the low nine bits are permissions; the type comes
/// from the node's kind, and Go keeps setuid, setgid, and sticky at other
/// positions, which this drops on purpose: a restore shouldn't recreate a setuid
/// binary. `0` (no permission concept) when the node recorded none.
pub(crate) fn permissions_of(mode: Option<u32>) -> u32 {
    mode.map_or(0, |mode| mode & 0o777)
}

/// A node's modification date in whole Unix seconds, the way every backend
/// lists one. A date before 1970 is left out rather than wrapped.
pub(crate) fn modified_secs(node: &Node) -> Option<u64> {
    u64::try_from(node.meta.mtime?.as_second()).ok()
}

/// The row a node lists as, inside the folder at `dir`.
///
/// The name is read through `Node::name()` (rustic stores names escaped) and
/// crosses into app spelling at `from_native_os_unicode`. A name that isn't
/// plain Unicode text (a Linux machine's backup) keeps today's lossy row, the
/// way the local volume lists one, until escapes go live; so does one a hostile
/// repo stores that isn't one plain name (`a/b`, `..`). A symlink is a row of
/// its own, ❌ never followed. Devices, fifos, and sockets are plain rows with no
/// bytes.
pub(crate) fn entry_of(node: &Node, dir: &AppPath) -> FileEntry {
    let stored = node.name();
    let name = match AppNameBuf::from_native_os_unicode(&stored) {
        Ok(name) => name.into_string(),
        Err(_) => stored.to_string_lossy().into_owned(),
    };
    let path = child_text(dir, &name);
    let size = match node.node_type {
        NodeType::Dir => None,
        NodeType::File | NodeType::Symlink { .. } => Some(node.meta.size),
        NodeType::Dev { .. } | NodeType::Chardev { .. } | NodeType::Fifo | NodeType::Socket => Some(0),
    };
    FileEntry {
        size,
        modified_at: modified_secs(node),
        permissions: permissions_of(node.meta.mode),
        owner: node.meta.user.clone().unwrap_or_default(),
        group: node.meta.group.clone().unwrap_or_default(),
        // A tree blob carries everything a row shows: no deferred metadata.
        ..FileEntry::new(name, path, node.is_dir(), node.is_symlink())
    }
}

/// The node named `name` in `tree`. `dir` is the folder's path, for the error.
///
/// Matches the stored name exactly. A sibling whose name isn't UTF-8 lists under
/// its lossy spelling, so when one lists as `name` too, the two are one row name
/// for two files: the lookup answers `AmbiguousName` rather than guess.
pub(crate) fn child<'t>(tree: &'t Tree, name: &str, dir: &AppPath) -> Result<Option<&'t Node>, VolumeError> {
    let found = tree.nodes.iter().find(|node| *node.name() == *name);
    let lossy_twin = || {
        tree.nodes.iter().any(|node| {
            let stored = node.name();
            stored.to_str().is_none() && stored.to_string_lossy() == name
        })
    };
    match found {
        Some(_) if lossy_twin() => Err(VolumeError::AmbiguousName(child_text(dir, name))),
        found => Ok(found),
    }
}

/// Reads one tree blob.
fn tree_at(repo: &IndexedRepo, id: TreeId) -> Result<Tree, ResticError> {
    repo.repo.get_tree(&id).map_err(|err| repo.classify(&err))
}

/// The node at `inner` below the snapshot tree `root`, or `None` when a name on
/// the way isn't there (or a file stands where a folder should). `inner` must
/// not be empty: the snapshot folder itself has no node. Blocking.
pub(crate) fn node_at(
    repo: &IndexedRepo,
    root: TreeId,
    inner: &[String],
    path: &AppPath,
) -> Result<Option<Node>, ResticError> {
    let Some((last, folders)) = inner.split_last() else {
        return Ok(None);
    };
    let Some(tree) = folder_tree(repo, root, folders, path)? else {
        return Ok(None);
    };
    Ok(child(&tree, last, path).map_err(ResticError::Volume)?.cloned())
}

/// The tree of the folder at `inner` below the snapshot tree `root` (`inner`
/// empty for the snapshot's own top level), or `None` when it isn't a folder in
/// this snapshot. Blocking. Typed as a [`ResticError`], so a caller tells a
/// pack gone under a stale index from any other failure.
pub(crate) fn folder_tree(
    repo: &IndexedRepo,
    root: TreeId,
    inner: &[String],
    path: &AppPath,
) -> Result<Option<Tree>, ResticError> {
    let mut tree = tree_at(repo, root)?;
    for name in inner {
        let subtree = match child(&tree, name, path).map_err(ResticError::Volume)? {
            Some(node) if node.is_dir() => node.subtree,
            _ => None,
        };
        let Some(id) = subtree else {
            return Ok(None);
        };
        tree = tree_at(repo, id)?;
    }
    Ok(Some(tree))
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
