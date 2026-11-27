//! [`ChainVolume`]: a folder chain of any depth, for the cells that prove a walk
//! keeps its depth on the heap (#1541).
//!
//! A local tree stops near 500 levels (macOS's 1,024-byte `PATH_MAX`), and a
//! debug walk's frames often fit that on a 2 MiB worker. A remote has no such
//! limit, so these cells walk a chain deeper than any frame-per-level walk
//! survives. Each listing costs one row, where `InMemoryVolume`'s scans its whole
//! store, so the chain's cost is the walk's own.

use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{Volume, VolumeError};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The chain's top folder: `/deep`, then `depth` nested `d` folders, with one
/// [`BOTTOM_FILE`] in the deepest.
pub(crate) const TOP: &str = "/deep";
/// The one file, at the bottom of the chain.
pub(crate) const BOTTOM_FILE: &str = "bottom.txt";
/// Its size, so a scan's byte total names it.
pub(crate) const BOTTOM_SIZE: u64 = 6;

/// `/deep/d/d/…`, read-only but for counting deletes: a delete succeeds and is
/// counted, and the chain still lists as it did (each walk lists a folder before
/// it deletes what's in it).
pub(crate) struct ChainVolume {
    depth: usize,
    root: &'static AppPath,
    deletes: AtomicUsize,
}

/// What sits at a path of the chain.
enum Node {
    /// A folder this many levels below [`TOP`].
    Folder(usize),
    Bottom,
}

impl ChainVolume {
    pub(crate) fn new(depth: usize) -> Self {
        Self {
            depth,
            root: AppPath::from_normalized("/").expect("the root is canonical"),
            deletes: AtomicUsize::new(0),
        }
    }

    /// Deletes of every kind so far.
    pub(crate) fn deletes(&self) -> usize {
        self.deletes.load(Ordering::Relaxed)
    }

    fn node(&self, path: &AppPath) -> Option<Node> {
        let below = path.as_str().strip_prefix(TOP)?;
        let mut level = 0;
        let mut parts = below.split('/').skip(1).peekable();
        while let Some(part) = parts.next() {
            match part {
                "d" if level < self.depth => level += 1,
                BOTTOM_FILE if level == self.depth && parts.peek().is_none() => return Some(Node::Bottom),
                _ => return None,
            }
        }
        (below.is_empty() || below.starts_with('/')).then_some(Node::Folder(level))
    }

    fn entry(path: &AppPath, node: &Node) -> FileEntry {
        let name = path.as_str().rsplit('/').next().unwrap_or_default().to_owned();
        match node {
            Node::Folder(_) => FileEntry::new(name, path.as_str().to_owned(), true, false),
            Node::Bottom => FileEntry {
                size: Some(BOTTOM_SIZE),
                ..FileEntry::new(name, path.as_str().to_owned(), false, false)
            },
        }
    }

    fn deleted(&self, path: &AppPath) -> Result<(), VolumeError> {
        self.node(path)
            .ok_or_else(|| VolumeError::NotFound(path.as_str().to_owned()))?;
        self.deletes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Volume for ChainVolume {
    fn name(&self) -> &str {
        "Chain"
    }
    fn root(&self) -> &AppPath {
        self.root
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(cmdr_fs::volume::ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let child = match self.node(path) {
                Some(Node::Folder(level)) if level < self.depth => "d",
                Some(Node::Folder(_)) => BOTTOM_FILE,
                _ => return Err(VolumeError::NotFound(path.as_str().to_owned())),
            };
            let child_path = AppPath::from_normalized(&format!("{}/{child}", path.as_str()))
                .expect("a chain path is canonical")
                .to_owned();
            let node = self.node(&child_path).expect("the chain's own child");
            Ok(vec![Self::entry(&child_path, &node)])
        })
    }
    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            self.node(path)
                .map(|node| Self::entry(path, &node))
                .ok_or_else(|| VolumeError::NotFound(path.as_str().to_owned()))
        })
    }
    fn delete<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        Box::pin(async move { self.deleted(path) })
    }
}
