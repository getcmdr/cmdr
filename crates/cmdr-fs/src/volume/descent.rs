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
//!   (an SMB listing's index number).
//! - **By depth**: past [`MAX_WALK_DEPTH`] levels below the walk's root, stop.
//!   The backstop for a volume that names no nodes.

use std::sync::Arc;

/// The most folder levels a walk descends below its root.
///
/// A backstop for a loop no node id can catch, ❌ not a limit on real trees,
/// which rarely pass 40 levels. At this depth a link loop has cost 128 listings
/// down one path, bounded, where unbounded it ran until the stack or the
/// server's path limit gave out.
pub const MAX_WALK_DEPTH: usize = 128;

/// Why a walk leaves a listed folder out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The folder is one the walk is already inside: a link back up its own path.
    LinkLoop,
    /// The folder sits more than [`MAX_WALK_DEPTH`] levels below the walk's root.
    TooDeep,
}

/// The folders a walk passed through to reach one folder, as node ids, plus
/// its depth below the walk's root. Cheap to clone: each level shares its
/// parent's chain.
#[derive(Debug, Clone)]
pub struct Descent {
    tip: Option<Arc<Step>>,
    depth: usize,
}

#[derive(Debug)]
struct Step {
    node: Option<u64>,
    up: Option<Arc<Step>>,
}

impl Descent {
    /// A walk's root, with its node id when the volume names one.
    pub fn root(node: Option<u64>) -> Self {
        Self {
            tip: Some(Arc::new(Step { node, up: None })),
            depth: 0,
        }
    }

    /// How many levels below the walk's root this folder is.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Tell this folder its own node id once its listing names it (an SMB
    /// listing's `.` row), when the step that reached it had none.
    pub fn named(self, node: Option<u64>) -> Self {
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
    /// shouldn't go there.
    pub fn child(&self, node: Option<u64>) -> Result<Self, Refusal> {
        if node.is_some() && self.passes_through(node) {
            return Err(Refusal::LinkLoop);
        }
        if self.depth >= MAX_WALK_DEPTH {
            return Err(Refusal::TooDeep);
        }
        Ok(Self {
            tip: Some(Arc::new(Step {
                node,
                up: self.tip.clone(),
            })),
            depth: self.depth + 1,
        })
    }

    fn passes_through(&self, node: Option<u64>) -> bool {
        let mut step = self.tip.as_deref();
        while let Some(current) = step {
            if current.node == node {
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
        let mut folder = Descent::root(None);
        for _ in 0..MAX_WALK_DEPTH {
            folder = folder.child(None).expect("within the backstop");
        }

        assert_eq!(folder.depth(), MAX_WALK_DEPTH);
        assert_eq!(folder.child(None).unwrap_err(), Refusal::TooDeep);
        assert!(!too_deep(MAX_WALK_DEPTH) && too_deep(MAX_WALK_DEPTH + 1));
    }
}
