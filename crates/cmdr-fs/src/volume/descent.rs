//! [`Descent`]: whether a recursive walk may go into a folder it listed.
//!
//! A listing doesn't always say which folders are links: Samba lists a Unix
//! symlink as the folder it points to, so `loop -> ..` looks like any folder,
//! and a walk that descends it meets the same folder again one level deeper,
//! forever (a size count on such a share overflowed the stack). Two answers,
//! shared by every walk:
//!
//! - **By node**: a folder whose node id is already on the walk's path down is
//!   a loop. Catches it on the first lap, wherever the volume names its nodes
//!   (an SMB listing's index number, `Volume::reached_node`).
//! - **By depth**: past [`MAX_WALK_DEPTH`] levels below the walk's root, a
//!   folder the volume names no node for is refused. The backstop for a walk
//!   that can't see its links; ❌ never a cap on a folder whose node it checked,
//!   since real trees run deeper than any loop needs to be caught at (#1541).
//!   A walk that sees every link and bounds the links it follows has a better
//!   backstop, and descends through [`Descent::child_seen_through`].

use std::sync::Arc;

/// The most folder levels a walk descends below its root into folders the
/// volume names no node for.
///
/// A backstop for a loop no node id can catch, ❌ not a limit on real trees:
/// it only ever applies where a node check couldn't. At this depth a link loop
/// has cost 128 listings down one path, bounded, where unbounded it ran until
/// the stack or the server's path limit gave out.
pub const MAX_WALK_DEPTH: usize = 128;

/// Why a walk leaves a listed folder out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The folder is one the walk is already inside: a link back up its own path.
    LinkLoop,
    /// The folder has no node id and sits more than [`MAX_WALK_DEPTH`] levels
    /// below the walk's root.
    TooDeep,
}

/// The folders a walk passed through to reach one folder, as node ids (`N`: an
/// SMB index number, or a `FileId`), plus its depth below the walk's root.
/// Cheap to clone: each level shares its parent's chain.
#[derive(Debug, Clone)]
pub struct Descent<N = u64> {
    tip: Option<Arc<Step<N>>>,
    depth: usize,
}

#[derive(Debug)]
struct Step<N> {
    node: Option<N>,
    up: Option<Arc<Step<N>>>,
}

impl<N: PartialEq> Descent<N> {
    /// A walk's root, with its node id when the volume names one.
    pub fn root(node: Option<N>) -> Self {
        Self::root_under(std::iter::empty(), node)
    }

    /// A walk's root that sits under `ancestors` (outermost first, the volume's
    /// root included): a folder a link reaches above the walk's root is on its
    /// path too. Depth counts from the root, ❌ never from its ancestors.
    pub fn root_under(ancestors: impl IntoIterator<Item = Option<N>>, node: Option<N>) -> Self {
        let mut tip = None;
        for node in ancestors.into_iter().chain(std::iter::once(node)) {
            tip = Some(Arc::new(Step { node, up: tip }));
        }
        Self { tip, depth: 0 }
    }

    /// How many levels below the walk's root this folder is.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Tell this folder its own node id once its listing names it (an SMB
    /// listing's `.` row), when the step that reached it had none.
    pub fn named(self, node: Option<N>) -> Self {
        let Some(node) = node else { return self };
        match &self.tip {
            Some(step) if step.node.is_none() => Self {
                tip: Some(Arc::new(Step {
                    node: Some(node),
                    up: step.up.clone(),
                })),
                depth: self.depth,
            },
            _ => self,
        }
    }

    /// The child folder with node id `node`, one level down, or why the walk
    /// shouldn't go there: a loop by node, or past [`MAX_WALK_DEPTH`] with no
    /// node to check.
    pub fn child(&self, node: Option<N>) -> Result<Self, Refusal> {
        if node.is_none() && self.depth >= MAX_WALK_DEPTH {
            return Err(Refusal::TooDeep);
        }
        self.child_seen_through(node)
    }

    /// [`child`](Self::child) for a walk that sees every link it follows and
    /// bounds them itself (the copy walk's links-followed backstop): the node
    /// check alone, at any depth.
    pub fn child_seen_through(&self, node: Option<N>) -> Result<Self, Refusal> {
        if node.is_some() && self.passes_through(&node) {
            return Err(Refusal::LinkLoop);
        }
        Ok(Self {
            tip: Some(Arc::new(Step {
                node,
                up: self.tip.clone(),
            })),
            depth: self.depth + 1,
        })
    }

    fn passes_through(&self, node: &Option<N>) -> bool {
        let mut step = self.tip.as_deref();
        while let Some(current) = step {
            if current.node == *node {
                return true;
            }
            step = current.up.as_deref();
        }
        false
    }
}

/// The depth backstop alone, for a walk that holds only a folder's path: is
/// `depth` levels below the root past [`MAX_WALK_DEPTH`]?
pub fn too_deep(depth: usize) -> bool {
    depth > MAX_WALK_DEPTH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_already_on_the_path_down_is_a_loop() {
        let root = Descent::root(Some(1));
        let a = root.child(Some(2)).unwrap();

        assert_eq!(
            a.child(Some(1)).unwrap_err(),
            Refusal::LinkLoop,
            "a link back to the root"
        );
        assert_eq!(a.child(Some(2)).unwrap_err(), Refusal::LinkLoop, "a link to itself");
        assert!(a.child(Some(3)).is_ok(), "a new folder");
    }

    /// Two siblings may share nothing but a parent; a folder reached twice by
    /// different paths (a link to a sibling) isn't a loop, and still counts.
    #[test]
    fn a_folder_off_the_path_down_is_no_loop() {
        let root = Descent::root(Some(1));
        let a = root.child(Some(2)).unwrap();
        let b = root.child(Some(3)).unwrap();

        assert!(a.child(Some(3)).is_ok(), "b reached again from inside a");
        assert!(b.child(Some(2)).is_ok());
    }

    #[test]
    fn a_folder_named_only_by_its_own_listing_still_catches_the_loop() {
        let root = Descent::root(None).named(Some(1));

        assert_eq!(root.child(Some(1)).unwrap_err(), Refusal::LinkLoop);
    }

    #[test]
    fn without_node_ids_the_depth_backstop_ends_the_walk() {
        let mut folder = Descent::<u64>::root(None);
        for _ in 0..MAX_WALK_DEPTH {
            folder = folder.child(None).expect("within the backstop");
        }

        assert_eq!(folder.depth(), MAX_WALK_DEPTH);
        assert_eq!(folder.child(None).unwrap_err(), Refusal::TooDeep);
        assert!(!too_deep(MAX_WALK_DEPTH) && too_deep(MAX_WALK_DEPTH + 1));
    }

    /// A real tree deeper than the backstop walks whole wherever its folders
    /// have ids (#1541): the depth cap is for folders no node check covers.
    #[test]
    fn a_folder_with_a_node_id_goes_in_at_any_depth() {
        let mut folder = Descent::root(Some(0));
        for node in 1..=(MAX_WALK_DEPTH as u64 * 3) {
            folder = folder.child(Some(node)).expect("a new folder, however deep");
        }

        assert_eq!(folder.depth(), MAX_WALK_DEPTH * 3);
        assert_eq!(folder.child(Some(7)).unwrap_err(), Refusal::LinkLoop);
    }

    #[test]
    fn a_walk_with_its_own_backstop_descends_folders_without_ids_at_any_depth() {
        let mut folder = Descent::<u64>::root(None);
        for _ in 0..MAX_WALK_DEPTH * 2 {
            folder = folder.child_seen_through(None).expect("no cap of its own");
        }

        assert_eq!(folder.depth(), MAX_WALK_DEPTH * 2);
    }

    /// A link to a folder above the walk's root is a loop too.
    #[test]
    fn the_roots_ancestors_are_on_the_path() {
        let root = Descent::root_under([Some(1), Some(2)], Some(3));

        assert_eq!(root.depth(), 0);
        assert_eq!(root.child(Some(1)).unwrap_err(), Refusal::LinkLoop);
        assert_eq!(root.child(Some(3)).unwrap_err(), Refusal::LinkLoop);
        assert!(root.child(Some(4)).is_ok());
    }
}
