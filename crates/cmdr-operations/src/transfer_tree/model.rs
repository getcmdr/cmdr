//! The arena: one compact node per selected entry, folder aggregates, and the
//! per-file-boundary mutations the drivers report through.
//!
//! Layout (the 64 B/node budget in `DETAILS.md` rests on it):
//! - Nodes sit in BREADTH-FIRST order, assigned once by the builder, so every
//!   folder's scanned children are one contiguous id range in tree order. A
//!   child's position is `id - first_child`, a child is `first_child + i`, and no
//!   per-node child list exists at all.
//! - A child discovered after the scan (a file that appeared mid-copy, or the
//!   contents of an opaque folder) is appended to the arena and listed in
//!   `extra_children`, after its parent's scanned children.
//! - Names live in one shared `String`; a node holds an offset and a length.
//! - A file's size and a folder's aggregate live in side vectors, reached
//!   through the node's `slot`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{TransferLeaves, WriteOperationError};

/// A node's stable id: its index in the arena. Assigned once when the scan's
/// tree is built and never reused, so the frontend keys rows and hand toggles
/// by it for the whole operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NodeId(pub(super) u32);

impl NodeId {
    /// The common parent of the selection. Never a row of its own.
    pub(crate) const ROOT: NodeId = NodeId(0);

    /// The id as the wire carries it.
    pub(crate) fn get(self) -> u32 {
        self.0
    }

    pub(super) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Each node's place in tree order (`TreeModel::tree_order_ranks`).
pub(crate) struct TreeOrderRanks(Vec<u32>);

impl TreeOrderRanks {
    pub(crate) fn of(&self, id: NodeId) -> u32 {
        self.0[id.index()]
    }
}

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum NodeKind {
    File,
    /// A folder whose children the scan walked.
    Folder,
    /// A folder whose totals the scan knows but whose children it never walked
    /// (`ScanRecorder::subtree`). The driver's live listing discovers them.
    Opaque,
}

impl NodeKind {
    pub(crate) fn is_folder(self) -> bool {
        !matches!(self, NodeKind::File)
    }
}

/// Where one file stands. A folder's state is derived from its aggregate at
/// read time (`TreeModel::folder_state`), never stored, except for an EMPTY
/// folder, which is done once the driver has created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LeafState {
    Waiting,
    Active,
    Done,
    Skipped,
    Failed,
}

impl LeafState {
    fn is_settled(self) -> bool {
        matches!(self, LeafState::Done | LeafState::Skipped | LeafState::Failed)
    }
}

/// A row's state as the view shows it, for files and folders alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[allow(
    missing_docs,
    reason = "a wire type: every `///` here reaches `bindings.ts` as JSDoc, so its fields and variants stay as they ship"
)]
pub enum TreeRowState {
    Waiting,
    Active,
    Done,
    Skipped,
    Failed,
}

/// 16 bytes. ❗ Every field is load-bearing for the per-node budget: grow this
/// and the 1M-node budget test says by how much.
#[derive(Debug, Clone, Copy)]
struct Node {
    parent: u32,
    name_offset: u32,
    name_len: u16,
    kind: NodeKind,
    state: LeafState,
    /// Index into `file_sizes` for a file, into `folders` for a folder.
    slot: u32,
}

/// A folder's children range and its descendants' counts. Counts are of FILES
/// below it, at any depth.
// DEFAULT-OK: all zeroes is a folder with no children and nothing counted yet,
// which is what a freshly discovered folder truthfully is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FolderAgg {
    first_child: u32,
    child_count: u32,
    pub(crate) files_total: u32,
    /// Landed plus skipped.
    pub(crate) files_done: u32,
    pub(crate) skipped: u32,
    pub(crate) failed: u32,
    pub(crate) bytes_total: u64,
    /// Bytes of landed and skipped files. In-flight bytes are added at read
    /// time (`view.rs`), never stored here.
    pub(crate) bytes_done: u64,
}

/// One file being transferred right now, with the live byte count its driver's
/// handle advances per chunk. The tree reads it at view time, so a chunk never
/// takes the tree's lock.
#[derive(Debug, Clone)]
pub(crate) struct ActiveLeaf {
    pub(crate) node: NodeId,
    pub(crate) bytes: Arc<AtomicU64>,
}

/// Where a transfer's sources land, so a failure that names a DESTINATION path
/// (a full disk, a folder that won't take a write) finds its row: each source
/// sits in a landing folder under the name its [`TransferLeaves`] gives it.
// DEFAULT-OK: no landing folder is "nothing lands anywhere a path could name",
// which is what a compress (its destination is one archive) truthfully says.
#[derive(Debug, Default)]
pub(crate) struct TreeLanding {
    /// Where the sources land: the destination folder, plus a cross-FS move's
    /// staging folder, which its copy phase writes into first.
    folders: Vec<PathBuf>,
    /// The name each source lands as.
    leaves: TransferLeaves,
}

impl TreeLanding {
    /// Each source lands in `folder` under the name `leaves` gives it.
    pub(crate) fn under(folder: &Path, leaves: &TransferLeaves) -> Self {
        Self {
            folders: vec![folder.to_path_buf()],
            leaves: leaves.clone(),
        }
    }

    /// Nothing a path could name: a compress writes one archive.
    pub(crate) fn nowhere() -> Self {
        Self::default()
    }

    /// The sources pass through `folder` first, under the same names.
    #[must_use]
    pub(crate) fn and_first_under(mut self, folder: PathBuf) -> Self {
        self.folders.push(folder);
        self
    }
}

/// The selection as a tree, plus where every file stands.
// DEFAULT-OK: an empty arena with nothing active; `from_seeds` fills it before
// anyone reads it.
#[derive(Debug, Default)]
pub(crate) struct TreeModel {
    nodes: Vec<Node>,
    names: String,
    file_sizes: Vec<u64>,
    folders: Vec<FolderAgg>,
    extra_children: HashMap<u32, Vec<NodeId>>,
    active: Vec<ActiveLeaf>,
    /// The file that started last. The view's focus while nothing is active,
    /// so a folder doesn't flicker shut in the gap between two files.
    last_started: Option<NodeId>,
    failures: HashMap<u32, WriteOperationError>,
    /// Each selected source's top-level row, by the path the operation names it
    /// with: how a driver finds its source's row, and where a failing path
    /// starts its walk down (`node_at_path`). The selection only, ❌ never a
    /// path per node.
    sources: HashMap<PathBuf, NodeId>,
    /// Where the sources land, for a failure that names a destination path.
    landing: TreeLanding,
}

/// What the builder hands the arena for one node, in final (breadth-first) id
/// order. Children of a folder are the `child_count` ids from `first_child`.
pub(super) struct NodeSeed {
    pub(super) parent: u32,
    pub(super) name_offset: u32,
    pub(super) name_len: u16,
    pub(super) kind: NodeKind,
    pub(super) file_size: u64,
    pub(super) first_child: u32,
    pub(super) child_count: u32,
    /// Totals an opaque folder carries from the scan.
    pub(super) opaque_totals: (u32, u64),
}

impl TreeModel {
    /// Lays out the arena from seeds already in breadth-first order (the
    /// builder's job), then sums every folder's totals bottom-up.
    pub(super) fn from_seeds(seeds: Vec<NodeSeed>, names: String) -> Self {
        let mut model = TreeModel {
            nodes: Vec::with_capacity(seeds.len()),
            names,
            ..TreeModel::default()
        };
        for seed in &seeds {
            let slot = if seed.kind.is_folder() {
                model.folders.push(FolderAgg {
                    first_child: seed.first_child,
                    child_count: seed.child_count,
                    files_total: seed.opaque_totals.0,
                    bytes_total: seed.opaque_totals.1,
                    ..FolderAgg::default()
                });
                model.folders.len() - 1
            } else {
                model.file_sizes.push(seed.file_size);
                model.file_sizes.len() - 1
            };
            model.nodes.push(Node {
                parent: seed.parent,
                name_offset: seed.name_offset,
                name_len: seed.name_len,
                kind: seed.kind,
                state: LeafState::Waiting,
                slot: slot as u32,
            });
        }
        // Breadth-first: every child's id is larger than its parent's, so a
        // descending pass has finished a folder before adding it upward.
        for index in (1..model.nodes.len()).rev() {
            let node = model.nodes[index];
            let (files, bytes) = match node.kind {
                NodeKind::File => (1, model.file_sizes[node.slot as usize]),
                NodeKind::Folder | NodeKind::Opaque => {
                    let agg = model.folders[node.slot as usize];
                    (agg.files_total, agg.bytes_total)
                }
            };
            let parent = &mut model.folders[model.nodes[node.parent as usize].slot as usize];
            parent.files_total += files;
            parent.bytes_total += bytes;
        }
        model.nodes.shrink_to_fit();
        model.names.shrink_to_fit();
        model.file_sizes.shrink_to_fit();
        model.folders.shrink_to_fit();
        model
    }

    // ---- Reads ---------------------------------------------------------------

    /// How many nodes, the root included.
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn kind(&self, id: NodeId) -> NodeKind {
        self.nodes[id.index()].kind
    }

    pub(crate) fn name(&self, id: NodeId) -> &str {
        let node = &self.nodes[id.index()];
        let start = node.name_offset as usize;
        &self.names[start..start + node.name_len as usize]
    }

    /// The parent, or `None` for the root.
    pub(crate) fn parent(&self, id: NodeId) -> Option<NodeId> {
        (id != NodeId::ROOT).then(|| NodeId(self.nodes[id.index()].parent))
    }

    /// Scanned children plus discovered ones. Zero for a file.
    pub(crate) fn child_count(&self, id: NodeId) -> u32 {
        let Some(agg) = self.folder(id) else {
            return 0;
        };
        let extra = self.extra_children.get(&id.0).map_or(0, Vec::len) as u32;
        agg.child_count + extra
    }

    /// The `index`th child in tree order: the scanned ones first, then the
    /// discovered ones in the order they turned up.
    pub(crate) fn child_at(&self, id: NodeId, index: u32) -> NodeId {
        let agg = self.folder(id).expect("only a folder has children");
        if index < agg.child_count {
            NodeId(agg.first_child + index)
        } else {
            self.extra_children[&id.0][(index - agg.child_count) as usize]
        }
    }

    /// Where `id` sits among its parent's children (0-based).
    pub(crate) fn position_in_parent(&self, id: NodeId) -> u32 {
        let parent = self.parent(id).expect("the root has no siblings");
        let agg = self.folder(parent).expect("a parent is a folder");
        if id.0 >= agg.first_child && id.0 < agg.first_child + agg.child_count {
            return id.0 - agg.first_child;
        }
        // `discover` appends fresh ids, so a folder's extras ascend: a binary
        // search keeps a view's focus path O(log k) inside a big opaque folder.
        let at = self.extra_children[&parent.0]
            .binary_search(&id)
            .expect("a child is either scanned or discovered");
        agg.child_count + at as u32
    }

    /// Every node's place in a depth-first walk of the tree in tree order: the
    /// order its rows list in, fully expanded, and the order a driver starts
    /// files in. One pass, for a caller putting a flat file list in that order.
    pub(crate) fn tree_order_ranks(&self) -> TreeOrderRanks {
        let mut ranks = vec![0u32; self.nodes.len()];
        let mut stack = vec![NodeId::ROOT];
        let mut next = 0u32;
        while let Some(id) = stack.pop() {
            ranks[id.index()] = next;
            next += 1;
            for index in (0..self.child_count(id)).rev() {
                stack.push(self.child_at(id, index));
            }
        }
        TreeOrderRanks(ranks)
    }

    /// The top-level row of a selected source, by the path the operation names
    /// it with.
    pub(crate) fn source_node(&self, source: &Path) -> Option<NodeId> {
        self.sources.get(source).copied()
    }

    pub(super) fn set_sources(&mut self, sources: HashMap<PathBuf, NodeId>) {
        self.sources = sources;
    }

    pub(crate) fn set_landing(&mut self, landing: TreeLanding) {
        self.landing = landing;
    }

    /// The row at `path`, a source-side or a destination-side one: its
    /// source's row (by the source's own path, or by where it lands), then
    /// down one name per component. `None` for a path the tree never held
    /// (an operation-wide refusal, a renamed landing). O(sources + depth ×
    /// siblings), for the rare caller with nothing but a path (a failure), ❌
    /// never per file.
    pub(crate) fn node_at_path(&self, path: &Path) -> Option<NodeId> {
        let (node, below) = self.under_a_source(path).or_else(|| self.under_a_landing(path))?;
        below.components().try_fold(node, |at, component| {
            self.child_named(at, &component.as_os_str().to_string_lossy())
        })
    }

    /// The source `path` sits under, and the rest of the path below it.
    fn under_a_source<'p>(&self, path: &'p Path) -> Option<(NodeId, &'p Path)> {
        let (source, node) = self
            .sources
            .iter()
            .filter(|(source, _)| path.starts_with(source))
            .max_by_key(|(source, _)| source.as_os_str().len())?;
        Some((*node, path.strip_prefix(source).ok()?))
    }

    /// The source whose landing `path` sits under, and the rest below it. Two
    /// sources with one name can't both land under it, so that name answers
    /// neither rather than a guess.
    fn under_a_landing<'p>(&self, path: &'p Path) -> Option<(NodeId, &'p Path)> {
        // Every folder, not the first that holds the path: a cross-FS move's
        // staging folder sits inside its destination.
        self.landing
            .folders
            .iter()
            .filter_map(|folder| path.strip_prefix(folder).ok())
            .find_map(|below| self.landed_source(below))
    }

    /// The source that landed as `below`'s first component, and the rest.
    fn landed_source<'p>(&self, below: &'p Path) -> Option<(NodeId, &'p Path)> {
        let mut components = below.components();
        let landed_as = components.next()?.as_os_str();
        // The source that lands under that name, else the one whose own name it
        // is (a path that names where a source would have landed unrenamed).
        let node = self
            .only_source(|source| self.landing.leaves.leaf_of(source) == Some(landed_as))
            .or_else(|| self.only_source(|source| source.file_name() == Some(landed_as)))?;
        Some((node, components.as_path()))
    }

    /// The one source `matches` picks, or `None` when none or several do.
    fn only_source(&self, matches: impl Fn(&Path) -> bool) -> Option<NodeId> {
        let mut picked = self
            .sources
            .iter()
            .filter(|(source, _)| matches(source))
            .map(|(_, node)| *node);
        let node = picked.next()?;
        picked.next().is_none().then_some(node)
    }

    /// The child of `folder` called `name`, by a walk over its children.
    pub(crate) fn child_named(&self, folder: NodeId, name: &str) -> Option<NodeId> {
        (0..self.child_count(folder))
            .map(|index| self.child_at(folder, index))
            .find(|child| self.name(*child) == name)
    }

    /// Every child of `folder` by name, for a walker that lists the same level
    /// and looks each entry up (`TreeLevel`). Built once per level, dropped
    /// with it.
    pub(crate) fn children_by_name(&self, folder: NodeId) -> HashMap<String, NodeId> {
        (0..self.child_count(folder))
            .map(|index| {
                let child = self.child_at(folder, index);
                (self.name(child).to_owned(), child)
            })
            .collect()
    }

    /// A folder's aggregate, or `None` for a file.
    pub(crate) fn folder(&self, id: NodeId) -> Option<&FolderAgg> {
        let node = &self.nodes[id.index()];
        node.kind.is_folder().then(|| &self.folders[node.slot as usize])
    }

    /// A file's size, or `None` for a folder.
    pub(crate) fn file_size(&self, id: NodeId) -> Option<u64> {
        let node = &self.nodes[id.index()];
        (!node.kind.is_folder()).then(|| self.file_sizes[node.slot as usize])
    }

    pub(crate) fn leaf_state(&self, id: NodeId) -> LeafState {
        self.nodes[id.index()].state
    }

    pub(crate) fn failure(&self, id: NodeId) -> Option<&WriteOperationError> {
        self.failures.get(&id.0)
    }

    /// The files in flight right now.
    #[cfg(test)]
    pub(crate) fn active(&self) -> &[ActiveLeaf] {
        &self.active
    }

    /// The files the view follows: the active ones, or, between two files, the
    /// one that started last.
    pub(crate) fn focus(&self) -> Vec<NodeId> {
        if self.active.is_empty() {
            self.last_started.into_iter().collect()
        } else {
            self.active.iter().map(|leaf| leaf.node).collect()
        }
    }

    /// A folder's state from its counts and whether work is in flight below it.
    ///
    /// ❗ Never done while something below is in flight: an opaque folder's
    /// scan can count fewer files than its walk finds (one arrived since), and
    /// its counts then read complete while the extra one is still landing.
    pub(crate) fn folder_state(&self, id: NodeId, active_beneath: bool) -> TreeRowState {
        let agg = self.folder(id).expect("a folder's state is derived from its aggregate");
        if agg.failed > 0 {
            return TreeRowState::Failed;
        }
        if agg.files_total == 0 {
            return match self.nodes[id.index()].state {
                LeafState::Done => TreeRowState::Done,
                LeafState::Skipped => TreeRowState::Skipped,
                _ if active_beneath => TreeRowState::Active,
                _ => TreeRowState::Waiting,
            };
        }
        if agg.files_done >= agg.files_total && !active_beneath {
            return if agg.skipped >= agg.files_done {
                TreeRowState::Skipped
            } else {
                TreeRowState::Done
            };
        }
        if active_beneath || agg.files_done > 0 {
            TreeRowState::Active
        } else {
            TreeRowState::Waiting
        }
    }

    /// Bytes this arena holds on the heap, counted by capacity, for the budget
    /// test and the logs.
    pub(crate) fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let extras: usize = self
            .extra_children
            .values()
            .map(|children| children.capacity() * size_of::<NodeId>() + size_of::<(u32, Vec<NodeId>)>())
            .sum();
        self.nodes.capacity() * size_of::<Node>()
            + self.names.capacity()
            + self.file_sizes.capacity() * size_of::<u64>()
            + self.folders.capacity() * size_of::<FolderAgg>()
            + extras
            + self.active.capacity() * size_of::<ActiveLeaf>()
            + self.failures.capacity() * size_of::<(u32, WriteOperationError)>()
            + self
                .sources
                .keys()
                .map(|path| path.capacity() + size_of::<(PathBuf, NodeId)>())
                .sum::<usize>()
            + self.landing.folders.iter().map(PathBuf::capacity).sum::<usize>()
    }

    // ---- Mutations: one per file boundary ------------------------------------

    /// A file starts. `bytes` is the live count its driver advances per chunk.
    pub(crate) fn begin(&mut self, file: NodeId, bytes: Arc<AtomicU64>) {
        if self.leaf_state(file).is_settled() {
            return;
        }
        self.active.retain(|leaf| leaf.node != file);
        self.active.push(ActiveLeaf { node: file, bytes });
        self.nodes[file.index()].state = LeafState::Active;
        self.last_started = Some(file);
    }

    /// A file landed: every ancestor gains it.
    pub(crate) fn complete(&mut self, file: NodeId) {
        if !self.settle(file, LeafState::Done) {
            return;
        }
        let size = self.file_size(file).unwrap_or(0);
        self.for_each_ancestor(file, |agg| {
            agg.files_done += 1;
            agg.bytes_done += size;
        });
    }

    /// A file stopped without landing (a cancel, or an attempt that failed and
    /// may be retried): back to waiting, its in-flight bytes gone with it.
    pub(crate) fn withdraw(&mut self, file: NodeId) {
        if self.leaf_state(file) != LeafState::Active {
            return;
        }
        self.active.retain(|leaf| leaf.node != file);
        self.nodes[file.index()].state = LeafState::Waiting;
    }

    /// A file the transfer skipped: done for the bars, counted as skipped.
    pub(crate) fn skip(&mut self, file: NodeId) {
        if !self.settle(file, LeafState::Skipped) {
            return;
        }
        let size = self.file_size(file).unwrap_or(0);
        self.for_each_ancestor(file, |agg| {
            agg.files_done += 1;
            agg.bytes_done += size;
            agg.skipped += 1;
        });
    }

    /// A file or folder that failed, with the typed reason the row shows. A
    /// folder counts itself among its failures, so it reads failed whatever
    /// its files did.
    pub(crate) fn fail(&mut self, node: NodeId, error: WriteOperationError) {
        if self.kind(node).is_folder() {
            if self.failures.contains_key(&node.0) {
                return;
            }
            self.folder_mut(node).failed += 1;
        } else if !self.settle(node, LeafState::Failed) {
            return;
        }
        self.failures.insert(node.0, error);
        self.for_each_ancestor(node, |agg| agg.failed += 1);
    }

    /// The driver created this folder. Only an EMPTY folder reads it: one with
    /// files is done when they are.
    pub(crate) fn folder_created(&mut self, folder: NodeId) {
        if self.kind(folder).is_folder() {
            self.nodes[folder.index()].state = LeafState::Done;
        }
    }

    /// Every empty folder is in place: what a driver that creates them all in
    /// one pass at the end says (the local engine's scanned-dirs pass).
    pub(crate) fn empty_folders_created(&mut self) {
        for index in 1..self.nodes.len() {
            let node = self.nodes[index];
            if node.kind.is_folder() && self.folders[node.slot as usize].files_total == 0 {
                self.nodes[index].state = LeafState::Done;
            }
        }
    }

    /// `top` landed whole in one step (a rename): every file below it is done.
    pub(crate) fn complete_subtree(&mut self, top: NodeId) {
        self.settle_subtree(top, LeafState::Done);
    }

    /// `top` was skipped whole (a top-level Skip, a bulk pre-known conflict):
    /// every file below it not already settled is skipped.
    pub(crate) fn skip_subtree(&mut self, top: NodeId) {
        self.settle_subtree(top, LeafState::Skipped);
    }

    /// Settles every unsettled file below `top` as `state` (`Done` or
    /// `Skipped`), plus what an opaque folder's scan counted and its walk never
    /// named. One report per subtree, O(its size), so ❌ never per file.
    fn settle_subtree(&mut self, top: NodeId, state: LeafState) {
        let mut stack = vec![top];
        let mut opaque = Vec::new();
        while let Some(id) = stack.pop() {
            match self.kind(id) {
                NodeKind::File => match state {
                    LeafState::Skipped => self.skip(id),
                    _ => self.complete(id),
                },
                kind => {
                    if self.leaf_state(id) == LeafState::Waiting {
                        self.nodes[id.index()].state = state;
                    }
                    if kind == NodeKind::Opaque {
                        opaque.push(id);
                    }
                    for index in 0..self.child_count(id) {
                        stack.push(self.child_at(id, index));
                    }
                }
            }
        }
        // After its found children settled, whatever an opaque folder still
        // owes is the files its walk never reached. An opaque folder is never
        // found inside another, so one pass settles them all.
        for folder in opaque {
            let agg = self.folder(folder).copied().unwrap_or_default();
            let files = agg.files_total.saturating_sub(agg.files_done + agg.failed);
            let bytes = agg.bytes_total.saturating_sub(agg.bytes_done);
            if files == 0 && bytes == 0 {
                continue;
            }
            let skipped = if state == LeafState::Skipped { files } else { 0 };
            let mut at = Some(folder);
            while let Some(ancestor) = at {
                let agg = self.folder_mut(ancestor);
                agg.files_done += files;
                agg.bytes_done += bytes;
                agg.skipped += skipped;
                at = self.parent(ancestor);
            }
        }
    }

    /// A child the scan never saw, found by the driver's live listing: an
    /// opaque folder's contents, or a file that appeared after the scan.
    ///
    /// Its totals reach every ancestor up to the nearest OPAQUE one, which
    /// already counted it in the totals the scan reported.
    pub(crate) fn discover(&mut self, parent: NodeId, name: &str, kind: NodeKind, size: u64) -> NodeId {
        debug_assert!(self.kind(parent).is_folder(), "only a folder gains children");
        let name_offset = self.names.len() as u32;
        let name = clip_name(name);
        self.names.push_str(name);
        let slot = if kind.is_folder() {
            self.folders.push(FolderAgg::default());
            self.folders.len() - 1
        } else {
            self.file_sizes.push(size);
            self.file_sizes.len() - 1
        };
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            parent: parent.0,
            name_offset,
            name_len: name.len() as u16,
            kind,
            state: LeafState::Waiting,
            slot: slot as u32,
        });
        self.extra_children.entry(parent.0).or_default().push(id);
        if kind == NodeKind::File {
            let mut at = Some(parent);
            while let Some(ancestor) = at {
                if self.kind(ancestor) == NodeKind::Opaque {
                    break;
                }
                let agg = self.folder_mut(ancestor);
                agg.files_total += 1;
                agg.bytes_total += size;
                at = self.parent(ancestor);
            }
        }
        id
    }

    /// Moves `file` to a settled state once. `false` when it already was one,
    /// so a double report never counts twice.
    fn settle(&mut self, file: NodeId, state: LeafState) -> bool {
        if self.kind(file) != NodeKind::File || self.leaf_state(file).is_settled() {
            return false;
        }
        self.active.retain(|leaf| leaf.node != file);
        self.nodes[file.index()].state = state;
        true
    }

    fn for_each_ancestor(&mut self, file: NodeId, mut apply: impl FnMut(&mut FolderAgg)) {
        let mut at = self.parent(file);
        while let Some(ancestor) = at {
            apply(self.folder_mut(ancestor));
            at = self.parent(ancestor);
        }
    }

    fn folder_mut(&mut self, id: NodeId) -> &mut FolderAgg {
        let slot = self.nodes[id.index()].slot as usize;
        &mut self.folders[slot]
    }

    /// The bytes in flight below each folder, and which folders have work
    /// in flight below them, for one view.
    pub(crate) fn in_flight_by_folder(&self) -> HashMap<u32, u64> {
        let mut by_folder = HashMap::new();
        for leaf in &self.active {
            let bytes = leaf.bytes.load(Ordering::Relaxed);
            let mut at = self.parent(leaf.node);
            while let Some(ancestor) = at {
                *by_folder.entry(ancestor.0).or_insert(0) += bytes;
                at = self.parent(ancestor);
            }
        }
        by_folder
    }

    /// The live bytes of `file` while it's active.
    pub(crate) fn in_flight_bytes(&self, file: NodeId) -> Option<u64> {
        self.active
            .iter()
            .find(|leaf| leaf.node == file)
            .map(|leaf| leaf.bytes.load(Ordering::Relaxed))
    }
}

/// A name cut to what a node's `u16` length holds, on a char boundary. No real
/// filename comes near it (macOS caps a path at 1,024 bytes); it keeps a
/// pathological one from corrupting the arena.
pub(super) fn clip_name(name: &str) -> &str {
    if name.len() <= u16::MAX as usize {
        return name;
    }
    let mut end = u16::MAX as usize;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
