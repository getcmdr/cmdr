//! Building the arena, from whichever walk saw the selection.
//!
//! Three inputs, one builder:
//! - [`TreeRecorder`]: a volume scan's `ScanRecorder`, fed entry by entry in
//!   whatever order the backend walks (paths in the caller's form).
//! - [`from_local_scan`]: the local walk's `dirs` and per-file list.
//! - [`from_archive_plan`]: a compress plan's root-relative entry names.
//!
//! All three end in [`TreeBuilder::finish`], which sorts every folder's
//! children into tree order and lays the arena out breadth-first.
//!
//! ❗ Every public way in runs under [`built`] and answers `None` for a tree
//! bug, so a caller can't build a tree that fails its transfer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{CopyScanResult, ScanRecorder};

use super::built;
use super::model::{NodeId, NodeKind, NodeSeed, TreeModel, clip_name};
use super::order::TreeOrder;
use cmdr_fs::ignore_poison::IgnorePoison;

/// A node's id while the tree is being built, before breadth-first renumbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BuilderId(u32);

impl BuilderId {
    pub(super) const ROOT: BuilderId = BuilderId(0);
}

#[derive(Debug, Clone, Copy)]
struct BuilderNode {
    parent: u32,
    name_offset: u32,
    name_len: u16,
    kind: NodeKind,
    size: u64,
}

/// Collects nodes in any order; [`finish`](Self::finish) puts them in tree order.
pub(super) struct TreeBuilder {
    nodes: Vec<BuilderNode>,
    names: String,
    /// What an opaque folder's scan counted: `(files, bytes)`.
    opaque_totals: HashMap<u32, (u32, u64)>,
}

impl TreeBuilder {
    pub(super) fn new() -> Self {
        Self {
            nodes: vec![BuilderNode {
                parent: 0,
                name_offset: 0,
                name_len: 0,
                kind: NodeKind::Folder,
                size: 0,
            }],
            names: String::new(),
            opaque_totals: HashMap::new(),
        }
    }

    pub(super) fn child(&mut self, parent: BuilderId, name: &str, kind: NodeKind, size: u64) -> BuilderId {
        #[cfg(test)]
        assert_ne!(name, super::test_support::TREE_BUG_NAME, "an injected tree bug");
        let name = clip_name(name);
        let name_offset = self.names.len() as u32;
        self.names.push_str(name);
        self.nodes.push(BuilderNode {
            parent: parent.0,
            name_offset,
            name_len: name.len() as u16,
            kind,
            size,
        });
        BuilderId(self.nodes.len() as u32 - 1)
    }

    /// Settles what a node turned out to be: a source registered before the
    /// scan said whether it's a file or a folder, or a folder the scan only
    /// totalled.
    pub(super) fn set(&mut self, id: BuilderId, kind: NodeKind, size: u64) {
        let node = &mut self.nodes[id.0 as usize];
        node.kind = kind;
        node.size = size;
    }

    pub(super) fn set_opaque_totals(&mut self, id: BuilderId, files: u32, bytes: u64) {
        self.set(id, NodeKind::Opaque, 0);
        self.opaque_totals.insert(id.0, (files, bytes));
    }

    fn kind(&self, id: BuilderId) -> NodeKind {
        self.nodes[id.0 as usize].kind
    }

    /// Sorts every folder's children into tree order and lays the arena out
    /// breadth-first. Returns the final id of every builder id.
    pub(super) fn finish(self, order: &TreeOrder) -> (TreeModel, Vec<NodeId>) {
        let count = self.nodes.len();
        // Children grouped by parent (CSR), the root's own parent link skipped.
        let mut starts = vec![0u32; count + 1];
        for node in &self.nodes[1..] {
            starts[node.parent as usize + 1] += 1;
        }
        for index in 0..count {
            starts[index + 1] += starts[index];
        }
        let mut children = vec![0u32; count.saturating_sub(1)];
        let mut fill = starts.clone();
        for (index, node) in self.nodes.iter().enumerate().skip(1) {
            let slot = &mut fill[node.parent as usize];
            children[*slot as usize] = index as u32;
            *slot += 1;
        }
        let mut group_facts: Vec<(u32, bool, &str)> = Vec::new();
        for parent in 0..count {
            let group = &mut children[starts[parent] as usize..starts[parent + 1] as usize];
            if group.len() < 2 {
                continue;
            }
            group_facts.clear();
            group_facts.extend(group.iter().map(|&id| {
                let node = &self.nodes[id as usize];
                let start = node.name_offset as usize;
                (
                    id,
                    node.kind.is_folder(),
                    &self.names[start..start + node.name_len as usize],
                )
            }));
            order.sort(&mut group_facts, |(_, is_folder, name)| (*is_folder, *name));
            for (slot, (id, _, _)) in group.iter_mut().zip(&group_facts) {
                *slot = *id;
            }
        }
        drop(group_facts);

        // Breadth-first numbering: each folder's children become one range.
        let mut final_id = vec![0u32; count];
        let mut bfs: Vec<u32> = Vec::with_capacity(count);
        bfs.push(0);
        let mut cursor = 0;
        while cursor < bfs.len() {
            let parent = bfs[cursor] as usize;
            for &child in &children[starts[parent] as usize..starts[parent + 1] as usize] {
                final_id[child as usize] = bfs.len() as u32;
                bfs.push(child);
            }
            cursor += 1;
        }

        let seeds = bfs
            .iter()
            .map(|&old| {
                let node = self.nodes[old as usize];
                let own = &children[starts[old as usize] as usize..starts[old as usize + 1] as usize];
                NodeSeed {
                    parent: final_id[node.parent as usize],
                    name_offset: node.name_offset,
                    name_len: node.name_len,
                    kind: node.kind,
                    file_size: node.size,
                    first_child: own.first().map_or(0, |first| final_id[*first as usize]),
                    child_count: own.len() as u32,
                    opaque_totals: self.opaque_totals.get(&old).copied().unwrap_or((0, 0)),
                }
            })
            .collect();
        drop(children);
        let model = TreeModel::from_seeds(seeds, self.names);
        (model, final_id.into_iter().map(NodeId).collect())
    }
}

// ---- From paths ---------------------------------------------------------------

/// Places absolute paths under a selection's sources.
///
/// The root is the sources' longest common ancestor (their shared parent in
/// the usual case). Each source is a top-level node labelled by its path
/// relative to the root, so a selection from search results reads
/// `photos/2024/IMG_1.jpg` with ❌ no synthesized `photos` row: it wasn't
/// selected.
struct PathTree {
    builder: TreeBuilder,
    /// Every folder by path, sources included: a child finds its parent here.
    /// Files are left out, which keeps the map to the folders of a 1M-file job.
    folders: HashMap<PathBuf, BuilderId>,
    /// The sources, which are nodes before the scan says what they are.
    sources: HashMap<PathBuf, BuilderId>,
}

impl PathTree {
    fn new(sources: &[PathBuf]) -> Self {
        let root = common_ancestor(sources);
        let mut builder = TreeBuilder::new();
        let mut by_path = HashMap::with_capacity(sources.len());
        for source in sources {
            if by_path.contains_key(source) {
                continue;
            }
            // A file until the scan says otherwise: a source the scan never
            // reported still shows, as the one row it is.
            let id = builder.child(BuilderId::ROOT, &source_label(&root, source), NodeKind::File, 0);
            by_path.insert(source.clone(), id);
        }
        Self {
            builder,
            folders: by_path.clone(),
            sources: by_path,
        }
    }

    fn dir(&mut self, path: &Path) -> Option<BuilderId> {
        if let Some(&id) = self.folders.get(path) {
            if self.builder.kind(id) == NodeKind::File {
                self.builder.set(id, NodeKind::Folder, 0);
            }
            return Some(id);
        }
        let parent = self.parent_of(path)?;
        let id = self.builder.child(parent, &file_name(path), NodeKind::Folder, 0);
        self.folders.insert(path.to_path_buf(), id);
        Some(id)
    }

    fn file(&mut self, path: &Path, size: u64) -> Option<BuilderId> {
        if let Some(&id) = self.sources.get(path) {
            self.builder.set(id, NodeKind::File, size);
            return Some(id);
        }
        let parent = self.parent_of(path)?;
        Some(self.builder.child(parent, &file_name(path), NodeKind::File, size))
    }

    fn subtree(&mut self, path: &Path, scan: &CopyScanResult) {
        if !scan.top_level_is_directory {
            self.file(path, scan.total_bytes);
            return;
        }
        if let Some(id) = self.dir(path) {
            let files = u32::try_from(scan.file_count).unwrap_or(u32::MAX);
            self.builder.set_opaque_totals(id, files, scan.total_bytes);
        }
    }

    /// The folder `path` sits in, creating any folder between it and the
    /// nearest known one: a backend may report an entry before its folder.
    /// `None` for a path under no source.
    fn parent_of(&mut self, path: &Path) -> Option<BuilderId> {
        let parent = path.parent()?;
        if let Some(&id) = self.folders.get(parent) {
            return self.as_folder(id);
        }
        let mut missing = vec![parent];
        let mut at = parent.parent();
        let known = loop {
            let ancestor = at?;
            if let Some(&id) = self.folders.get(ancestor) {
                break self.as_folder(id)?;
            }
            missing.push(ancestor);
            at = ancestor.parent();
        };
        let mut parent_id = known;
        for folder in missing.into_iter().rev() {
            parent_id = self.builder.child(parent_id, &file_name(folder), NodeKind::Folder, 0);
            self.folders.insert(folder.to_path_buf(), parent_id);
        }
        Some(parent_id)
    }

    /// Lays the arena out and hands it each source's row. Returns the final id
    /// of every builder id, as [`TreeBuilder::finish`] does.
    fn finish(self, order: &TreeOrder) -> (TreeModel, Vec<NodeId>) {
        let (mut model, final_ids) = self.builder.finish(order);
        model.set_sources(
            self.sources
                .into_iter()
                .map(|(path, id)| (path, final_ids[id.0 as usize]))
                .collect(),
        );
        (model, final_ids)
    }

    /// A known path made a folder: something turned up inside it.
    fn as_folder(&mut self, id: BuilderId) -> Option<BuilderId> {
        match self.builder.kind(id) {
            NodeKind::File => self.builder.set(id, NodeKind::Folder, 0),
            NodeKind::Folder => {}
            // Its children were never walked, and the totals it carries already
            // count them. Nothing may land beside that count.
            NodeKind::Opaque => return None,
        }
        Some(id)
    }
}

/// A source's top-level row label: its path relative to the selection's root,
/// as a person reads it (a twin's pick taken out, `cmdr_fs::twin_path::shown`).
fn source_label<'a>(root: &Path, source: &'a Path) -> std::borrow::Cow<'a, str> {
    let label = match source.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_string_lossy(),
        _ => source.to_string_lossy(),
    };
    match label {
        std::borrow::Cow::Borrowed(text) => cmdr_fs::twin_path::shown(text),
        std::borrow::Cow::Owned(text) => std::borrow::Cow::Owned(cmdr_fs::twin_path::shown(&text).into_owned()),
    }
}

/// Puts a selection's sources in tree order, the order its top-level rows list
/// in: ranked by the same labels, folders first. `is_folder` says what the scan
/// found each one to be; a source it knows nothing about ranks as the file row
/// it shows as. Sources with one label keep their given order, and all of them
/// do if ranking them panics.
///
/// `path_of` reads a source as the tree's `std::path` rows key it (a volume
/// driver holds its sources as app paths).
pub(crate) fn sort_sources<S: Clone>(
    sources: &mut [S],
    path_of: impl Fn(&S) -> std::borrow::Cow<'_, Path>,
    is_folder: impl Fn(&S) -> bool,
    order: &TreeOrder,
) {
    let ranked = built(order, "ranking the selection", || {
        let paths: Vec<PathBuf> = sources.iter().map(|source| path_of(source).into_owned()).collect();
        let root = common_ancestor(&paths);
        let mut ranked: Vec<(bool, String, S)> = sources
            .iter()
            .zip(&paths)
            .map(|(source, path)| {
                (
                    is_folder(source),
                    source_label(&root, path).into_owned(),
                    source.clone(),
                )
            })
            .collect();
        order.sort(&mut ranked, |(is_folder, label, _)| (*is_folder, label.as_str()));
        ranked
    });
    let Some(ranked) = ranked else { return };
    for (slot, (_, _, source)) in sources.iter_mut().zip(ranked) {
        *slot = source;
    }
}

/// The deepest folder every source sits in. A twin's row path sits in the
/// folder past its pick (`cmdr_fs::twin_path::folder_of`).
fn common_ancestor(sources: &[PathBuf]) -> PathBuf {
    let mut parents = sources
        .iter()
        .map(|source| cmdr_fs::twin_path::folder_of(source).unwrap_or(source));
    let Some(first) = parents.next() else {
        return PathBuf::new();
    };
    let mut common = first.to_path_buf();
    for parent in parents {
        while !parent.starts_with(&common) {
            if !common.pop() {
                return PathBuf::new();
            }
        }
    }
    common
}

fn file_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| path.to_string_lossy())
}

/// A volume scan's recorder: hang it on the scan's `ScanBoundary`
/// (`recording_to`), then [`finish`](Self::finish) it once the scan is done.
///
/// An entry the recorder can't place under one of the sources is dropped with
/// a debug line: that's a backend reporting a path form the caller doesn't
/// use, and a tree with a hole beats a tree that lies.
///
/// ❗ The scan calls in from its own walk, so an entry that panics is caught
/// here ([`built`]): the recorder goes quiet, the scan walks on, and
/// [`finish`](Self::finish) builds no tree.
pub(crate) struct TreeRecorder {
    tree: Mutex<PathTree>,
    /// The order the finished tree ranks in, and where a tree bug is reported.
    order: TreeOrder,
    /// An entry panicked: record nothing more, build nothing.
    broken: AtomicBool,
}

impl TreeRecorder {
    pub(crate) fn new(sources: &[PathBuf], order: TreeOrder) -> Self {
        let tree = built(&order, "placing the selection", || PathTree::new(sources));
        Self {
            order,
            broken: AtomicBool::new(tree.is_none()),
            // An empty tree places nothing, so it can't panic.
            tree: Mutex::new(tree.unwrap_or_else(|| PathTree::new(&[]))),
        }
    }

    /// The tree of everything recorded so far, or `None` after a tree bug.
    /// Takes it: the recorder is empty afterwards, so call it once the scan
    /// has returned.
    pub(crate) fn finish(&self) -> Option<TreeModel> {
        if self.broken.load(Ordering::Relaxed) {
            return None;
        }
        let tree = std::mem::replace(&mut *self.tree.lock_ignore_poison(), PathTree::new(&[]));
        built(&self.order, "finishing a scan's tree", || tree.finish(&self.order).0)
    }

    /// Records one entry, unless an earlier one broke the recorder.
    fn record(&self, entry: impl FnOnce(&mut PathTree)) {
        if self.broken.load(Ordering::Relaxed) {
            return;
        }
        if built(&self.order, "recording a scanned entry", || {
            entry(&mut self.tree.lock_ignore_poison())
        })
        .is_none()
        {
            self.broken.store(true, Ordering::Relaxed);
        }
    }
}

impl ScanRecorder for TreeRecorder {
    fn file(&self, path: &Path, size: u64) {
        self.record(|tree| {
            if tree.file(path, size).is_none() {
                log_unplaced(path);
            }
        });
    }

    fn dir(&self, path: &Path) {
        self.record(|tree| {
            if tree.dir(path).is_none() {
                log_unplaced(path);
            }
        });
    }

    fn subtree(&self, path: &Path, scan: &CopyScanResult) {
        self.record(|tree| tree.subtree(path, scan));
    }
}

fn log_unplaced(path: &Path) {
    log::debug!(
        target: "cmdr_operations",
        "transfer tree: dropped a scanned entry under no selected source: {}",
        LogPrivate::path(path)
    );
}

/// A tree of the sources alone, each a file or an opaque folder with its
/// scanned totals: what a cached preview's per-source results say when the
/// preview built no tree of its own.
pub(crate) fn from_per_path(
    sources: &[PathBuf],
    per_path: &[(PathBuf, CopyScanResult)],
    order: &TreeOrder,
) -> Option<TreeModel> {
    let recorder = TreeRecorder::new(sources, order.clone());
    for (path, scan) in per_path {
        recorder.subtree(path, scan);
    }
    recorder.finish()
}

/// A rename move's tree: one row per selected item, and nothing below a
/// folder. A rename takes a whole item in one call and walks nothing, so the
/// item IS the unit of work, the same unit its progress counts; a folder row
/// is done the moment its rename lands (`TreeModel::complete_subtree`).
pub(crate) fn from_sources(
    sources: &[PathBuf],
    is_folder: impl Fn(&Path) -> bool,
    order: &TreeOrder,
) -> Option<TreeModel> {
    built(order, "a rename move's items", || {
        let mut tree = PathTree::new(sources);
        for source in sources.iter().filter(|source| is_folder(source)) {
            tree.dir(source);
        }
        tree.finish(order).0
    })
}

/// The local walk's tree: `dirs` (every folder it entered, the sources among
/// them) and `files` (every file, link, and special entry it counted). Returns
/// each file's node, in `files` order, for `FileInfo::tree_node`: `None` for
/// one under no source, which the walk never produces.
pub(crate) fn from_local_scan<'a>(
    sources: &[PathBuf],
    dirs: &[PathBuf],
    files: impl IntoIterator<Item = (&'a Path, u64)>,
    order: &TreeOrder,
) -> Option<(TreeModel, Vec<Option<NodeId>>)> {
    built(order, "a local walk", || {
        let mut tree = PathTree::new(sources);
        for dir in dirs {
            tree.dir(dir);
        }
        let file_ids: Vec<Option<BuilderId>> = files.into_iter().map(|(path, size)| tree.file(path, size)).collect();
        let (model, final_ids) = tree.finish(order);
        let nodes = file_ids
            .into_iter()
            .map(|id| id.map(|id| final_ids[id.0 as usize]))
            .collect();
        (model, nodes)
    })
}

/// A compress plan's tree, from its entries' root-relative names (`top/a.txt`,
/// and `top/sub/` for a folder). Returns each entry's node, in order.
pub(crate) fn from_archive_plan<'a>(
    entries: impl IntoIterator<Item = (&'a str, bool, u64)>,
    order: &TreeOrder,
) -> Option<(TreeModel, Vec<NodeId>)> {
    built(order, "a compress plan", || {
        let mut builder = TreeBuilder::new();
        let mut folders: HashMap<&'a str, BuilderId> = HashMap::new();
        let mut entry_ids = Vec::new();
        for (name, is_directory, size) in entries {
            let trimmed = name.trim_end_matches('/');
            let (parent, leaf) = match trimmed.rsplit_once('/') {
                Some((parent, leaf)) => (folder_for(&mut builder, &mut folders, parent), leaf),
                None => (BuilderId::ROOT, trimmed),
            };
            let id = if is_directory {
                *folders
                    .entry(trimmed)
                    .or_insert_with(|| builder.child(parent, leaf, NodeKind::Folder, 0))
            } else {
                builder.child(parent, leaf, NodeKind::File, size)
            };
            entry_ids.push(id);
        }
        let (model, final_ids) = builder.finish(order);
        let nodes = entry_ids.into_iter().map(|id| final_ids[id.0 as usize]).collect();
        (model, nodes)
    })
}

/// The folder at `path` in a compress plan, created with its ancestors when the
/// plan hasn't listed it yet.
fn folder_for<'a>(builder: &mut TreeBuilder, folders: &mut HashMap<&'a str, BuilderId>, path: &'a str) -> BuilderId {
    if let Some(&id) = folders.get(path) {
        return id;
    }
    let (parent, leaf) = match path.rsplit_once('/') {
        Some((parent, leaf)) => (folder_for(builder, folders, parent), leaf),
        None => (BuilderId::ROOT, path),
    };
    let id = builder.child(parent, leaf, NodeKind::Folder, 0);
    folders.insert(path, id);
    id
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
