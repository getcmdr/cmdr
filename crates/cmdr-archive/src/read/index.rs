//! The parsed archive index: a synthetic directory tree plus the query surface
//! the volume layer reads. Format-agnostic — zip, tar, and 7z all feed the same
//! tree.
//!
//! Each format's `parse` (in [`super::zip`] / [`super::tar`] / [`super::sevenz`])
//! produces a `Vec<(RawEntry, H)>` — a format-neutral [`RawEntry`] (the parser
//! contract in [`super::raw`]) plus a per-format read handle `H`. [`build_index`]
//! then does the format-independent
//! work over any `H`:
//!
//! 1. sanitize each name (Zip Slip defense, see [`super::name`]), and
//! 2. [`build_tree`] synthesizes the directory hierarchy from the entry path
//!    prefixes — most archives carry no explicit directory entries, so the tree
//!    is inferred from `a/b/c.txt` implying `a/` and `a/b/`.
//!
//! The pruned handle map is wrapped into an [`EntryStore`] variant, the one place
//! that dispatches per format: [`ArchiveIndex::open_read`] opens a streaming
//! reader through it, and [`ArchiveIndex::open_subtree_extract`] picks the
//! format's one-pass producer for [`super::extract`]'s channel.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::Xattr;
use cmdr_fs::rt::Runtime;

use super::error::ArchiveError;
use super::extract::{SubtreeExtractReader, SubtreeMember};
use super::format::ArchiveFormat;
use super::name::{QuarantineReason, SanitizedName, sanitize_entry_name};
use super::raw::{MAX_TREE_NODES, RawEntry};
use super::reader::ArchiveEntryReader;
use super::sidecars::{self, EntryRole, HeldXattrs, LazySidecar, MAX_HELD_XATTR_BYTES, SidecarLayout};
use super::source::ArchiveByteSource;
use crate::apple_double;

/// One node in the synthetic tree: a file, a directory (explicit or implied),
/// or a symlink. Archive-native — the volume layer maps this onto `FileEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveNode {
    /// Final path component (the display name).
    pub name: String,
    /// Full inner path relative to the archive root, `/`-separated, no leading
    /// or trailing slash. The root directory is the empty string.
    pub path: String,
    /// Whether this node is a directory, explicit in the archive or implied by
    /// a deeper entry's path.
    pub is_dir: bool,
    /// Whether the archive recorded this entry as a symlink.
    pub is_symlink: bool,
    /// Uncompressed size in bytes. `None` for directories (synthetic dirs have
    /// no size; explicit dir entries carry a zero size we still report as None).
    pub size: Option<u64>,
    /// Compressed size in bytes. `None` for directories.
    pub compressed_size: Option<u64>,
    /// Last-modified time as a Unix timestamp in seconds. `None` for synthetic
    /// directories (inferred, so no real timestamp).
    pub modified: Option<i64>,
    /// Whether extracting this entry is blocked because it's encrypted.
    pub encrypted: bool,
    /// The POSIX permission bits (`0o777` and below) the archive recorded, when
    /// it recorded any. `None` for a synthetic directory and for a format or a
    /// writer that stores no unix mode, which is the honest answer: a
    /// Windows-made zip knows about a read-only flag and nothing else.
    ///
    /// The volume layer maps this onto `FileEntry::permissions`, and the copy
    /// engine puts it on what an extract writes, so a `None` guessed at as
    /// `0o644` would be a claim about the source that nobody made.
    pub mode: Option<u32>,
}

impl ArchiveNode {
    fn root() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            is_dir: true,
            is_symlink: false,
            size: None,
            compressed_size: None,
            modified: None,
            encrypted: false,
            mode: None,
        }
    }
}

/// The lightweight, pure input to [`build_tree`]: one accepted entry, already
/// sanitized. Decoupled from `rc_zip::Entry` so the tree builder can be tested
/// with hand-built seeds (no zip bytes, no field-heavy `Entry` construction).
#[derive(Debug, Clone)]
struct NodeSeed {
    /// Sanitized inner path: non-empty, no leading/trailing slash, no `..`.
    path: String,
    is_dir: bool,
    is_symlink: bool,
    size: Option<u64>,
    compressed_size: Option<u64>,
    modified: Option<i64>,
    encrypted: bool,
    mode: Option<u32>,
}

/// The per-format read handles, keyed by sanitized inner path. Each variant
/// knows how to open a streaming reader for one entry; [`Self::open_read`]
/// dispatches. The zip store IS the rc-zip entry map; tar and 7z carry their own
/// location/metadata handles plus the format state their reader needs.
pub(super) enum EntryStore {
    Zip(HashMap<String, super::zip::ZipHandle>),
    Tar(super::tar::TarStore),
    SevenZ(super::sevenz::SevenZStore),
}

impl EntryStore {
    /// Opens a streaming reader for the file at the already-normalized `inner`
    /// path. The caller ([`ArchiveIndex::open_read`]) has confirmed it's a file,
    /// not a directory, and passes the tree's `size` so every format reports the
    /// same uncompressed total for progress. `password` decrypts an encrypted zip
    /// entry (ZipCrypto or WinZip AES) or a content-encrypted 7z entry; it's
    /// ignored for a plaintext entry and for tar (never encrypted). Maps a missing
    /// handle to `NotFound`. The decode runs on `runtime`'s blocking pool.
    pub(super) fn open_read(
        &self,
        inner: &str,
        size: u64,
        source: Arc<dyn ArchiveByteSource>,
        password: Option<&str>,
        runtime: &Runtime,
    ) -> Result<ArchiveEntryReader, ArchiveError> {
        match self {
            EntryStore::Zip(files) => {
                let handle = files
                    .get(inner)
                    .ok_or_else(|| ArchiveError::NotFound(inner.to_string()))?;
                super::zip::open_read(handle, source, password.map(str::as_bytes), runtime)
            }
            EntryStore::Tar(store) => store.open_read(inner, size, source, runtime),
            EntryStore::SevenZ(store) => store.open_read(inner, size, source, password, runtime),
        }
    }

    /// Spawns the one-pass subtree producer for this store's format over
    /// `source`, emitting every file in `wanted` (inner path → the member the
    /// tree describes) in archive order (see [`super::extract`]).
    ///
    /// Only sequential formats have a producer; a random-access store (zip, plain
    /// tar) yields nothing — the copy planner never routes those here (it gates on
    /// [`Volume::extraction_is_sequential`](cmdr_fs::volume::Volume::extraction_is_sequential)),
    /// so an empty stream is the correct defensive fallback. The producer runs on
    /// `runtime`'s blocking pool, detached: it ends when the reader drops.
    fn spawn_subtree_extract(
        &self,
        source: Arc<dyn ArchiveByteSource>,
        wanted: HashMap<String, SubtreeMember>,
        password: Option<&str>,
        runtime: &Runtime,
    ) -> SubtreeExtractReader {
        let (reader, sink) = SubtreeExtractReader::channel();
        match self {
            EntryStore::Tar(tar_store) => {
                let codec = tar_store.codec();
                drop(runtime.spawn_blocking(move || super::tar::stream_subtree(source, codec, wanted, &sink)));
            }
            EntryStore::SevenZ(_) => {
                let password = password.map(str::to_owned);
                drop(runtime.spawn_blocking(move || {
                    super::sevenz::stream_subtree(source, wanted, password.as_deref(), &sink);
                }));
            }
            // Random-access: no one-pass producer. Dropping `sink` closes the
            // channel, so the reader yields no members.
            EntryStore::Zip(_) => drop(sink),
        }
        reader
    }
}

/// The output of the shared tree builder plus the pruned per-format handle map,
/// before it's wrapped into an [`ArchiveIndex`] with a concrete [`EntryStore`].
pub(super) struct BuiltIndex<H> {
    nodes: HashMap<String, ArchiveNode>,
    children: HashMap<String, Vec<String>>,
    quarantined: Vec<(String, QuarantineReason)>,
    has_encrypted: bool,
    /// Read handles for the entries that ended up as real file nodes, plus each
    /// lazy sidecar's (keyed by the sidecar's own path, which no node has).
    files: HashMap<String, H>,
    held_xattrs: HashMap<String, Vec<Xattr>>,
    lazy_sidecars: HashMap<String, LazySidecar>,
}

impl<H> BuiltIndex<H> {
    /// Wraps the built tree into an [`ArchiveIndex`], turning the handle map into
    /// a concrete [`EntryStore`] via `wrap`.
    pub(super) fn into_index(self, wrap: impl FnOnce(HashMap<String, H>) -> EntryStore) -> ArchiveIndex {
        ArchiveIndex {
            nodes: self.nodes,
            children: self.children,
            quarantined: self.quarantined,
            has_encrypted: self.has_encrypted,
            held_xattrs: self.held_xattrs,
            lazy_sidecars: self.lazy_sidecars,
            store: wrap(self.files),
        }
    }
}

/// A parsed archive index: the synthetic tree plus the per-file read handles.
/// Format-agnostic — the tree query surface is shared, and the format-specific
/// read handles live behind [`EntryStore`]. Cheap to share (`Arc`) and immutable
/// once built.
pub struct ArchiveIndex {
    /// Every node (files and directories, including the root) by inner path.
    nodes: HashMap<String, ArchiveNode>,
    /// Directory path -> its children's inner paths, pre-sorted (directories
    /// first, then case-insensitive by name).
    children: HashMap<String, Vec<String>>,
    /// Raw names dropped by the sanitizer, with the reason. Kept for diagnostics
    /// and tests (a hostile `../evil` never reaches the tree, but we record it).
    quarantined: Vec<(String, QuarantineReason)>,
    has_encrypted: bool,
    /// The xattrs the parse already read for an item, by its inner path: a tar's
    /// pax records and paired `._name` sidecars.
    held_xattrs: HashMap<String, Vec<Xattr>>,
    /// A zip item's `__MACOSX` sidecar, by the item's inner path, read when the
    /// item is copied ([`Self::item_xattrs`]).
    lazy_sidecars: HashMap<String, LazySidecar>,
    /// The per-format read handles for readable (non-directory) entries, and
    /// for each lazy sidecar.
    store: EntryStore,
}

impl std::fmt::Debug for ArchiveIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveIndex")
            .field("nodes", &self.nodes.len())
            .field("quarantined", &self.quarantined.len())
            .field("has_encrypted", &self.has_encrypted)
            .field("held_xattrs", &self.held_xattrs.len())
            .field("lazy_sidecars", &self.lazy_sidecars.len())
            .finish()
    }
}

impl ArchiveIndex {
    /// Parses `source` into an index for the given `format`: reads the format's
    /// directory (zip central directory, one tar scan, or the 7z header),
    /// sanitizes names identically for every format, and synthesizes the tree.
    ///
    /// `password` is needed only to build the index of a HEADER-encrypted 7z
    /// (`-mhe=on`), whose metadata is itself encrypted. Browsing every other
    /// archive needs none: a zip's names live in the plaintext central directory,
    /// a content-encrypted 7z has a plaintext header, and decryption happens later,
    /// per entry, at [`open_read`](Self::open_read).
    pub fn parse(
        source: Arc<dyn ArchiveByteSource>,
        format: ArchiveFormat,
        password: Option<&str>,
    ) -> Result<Self, ArchiveError> {
        match format {
            // A document container IS a zip, so it reads through the identical
            // path. The two part company only at the write guard.
            ArchiveFormat::Zip | ArchiveFormat::Ooxml => {
                let entries = super::zip::parse(source.as_ref())?;
                Ok(build_index(entries, SidecarLayout::MacosxFolder)?.into_index(EntryStore::Zip))
            }
            ArchiveFormat::Tar(codec) => {
                let entries = super::tar::parse(source, codec)?;
                Ok(build_index(entries, SidecarLayout::Siblings)?
                    .into_index(|members| EntryStore::Tar(super::tar::TarStore::new(codec, members))))
            }
            ArchiveFormat::SevenZ => {
                let (entries, meta) = super::sevenz::parse(source, password)?;
                Ok(build_index(entries, SidecarLayout::None)?
                    .into_index(|members| EntryStore::SevenZ(super::sevenz::SevenZStore::new(meta, members))))
            }
        }
    }

    /// Lists the children of the directory at `inner_path`. Returns `None` if
    /// the path doesn't exist or isn't a directory. An empty directory returns
    /// `Some(vec![])`. Children are directories-first, then name-sorted.
    pub fn list(&self, inner_path: &str) -> Option<Vec<ArchiveNode>> {
        let key = normalize_lookup(inner_path);
        // A path is a listable directory iff it has a directory node.
        if !self.nodes.get(key).is_some_and(|n| n.is_dir) {
            return None;
        }
        let child_paths = match self.children.get(key) {
            Some(paths) => paths,
            None => return Some(Vec::new()),
        };
        Some(child_paths.iter().filter_map(|p| self.nodes.get(p).cloned()).collect())
    }

    /// Metadata for the node at `inner_path` (a file or a directory), or `None`
    /// if nothing exists there. The archive root (`""`) is a directory node.
    pub fn get(&self, inner_path: &str) -> Option<ArchiveNode> {
        let key = normalize_lookup(inner_path);
        self.nodes.get(key).cloned()
    }

    /// Whether `inner_path` exists in the archive.
    pub fn exists(&self, inner_path: &str) -> bool {
        self.nodes.contains_key(normalize_lookup(inner_path))
    }

    /// `Some(true)` if `inner_path` is a directory, `Some(false)` if a file,
    /// `None` if it doesn't exist.
    pub fn is_directory(&self, inner_path: &str) -> Option<bool> {
        self.nodes.get(normalize_lookup(inner_path)).map(|n| n.is_dir)
    }

    /// Where the local header of the zip entry behind the FILE node at
    /// `inner_path` starts, or `None` when there's no such zip file node.
    pub(super) fn zip_header_offset(&self, inner_path: &str) -> Option<u64> {
        match &self.store {
            EntryStore::Zip(files) => files
                .get(normalize_lookup(inner_path))
                .map(super::zip::ZipHandle::header_offset),
            EntryStore::Tar(_) | EntryStore::SevenZ(_) => None,
        }
    }

    /// Whether the archive contains any encrypted entry. Browsing still works
    /// (names live in the central directory); this lets the volume layer warn
    /// or gate extraction up front.
    pub fn has_encrypted_entries(&self) -> bool {
        self.has_encrypted
    }

    /// Names the sanitizer dropped, with the reason. Empty for a clean archive.
    pub fn quarantined(&self) -> &[(String, QuarantineReason)] {
        &self.quarantined
    }

    /// Opens a streaming, chunk-by-chunk reader over the decompressed bytes of
    /// the file at `inner_path`, pulling compressed bytes from `source`.
    ///
    /// Errors: `NotFound` (no such path), `IsADirectory` (path is a directory),
    /// `Encrypted` (encrypted, no `password` supplied), `WrongPassword` (the
    /// supplied one didn't work). Decompression runs off the async executor, on
    /// `runtime`'s blocking pool (the host's); see [`ArchiveEntryReader`].
    pub fn open_read(
        &self,
        inner_path: &str,
        source: Arc<dyn ArchiveByteSource>,
        password: Option<&str>,
        runtime: &Runtime,
    ) -> Result<ArchiveEntryReader, ArchiveError> {
        let key = normalize_lookup(inner_path);
        let node = self
            .nodes
            .get(key)
            .ok_or_else(|| ArchiveError::NotFound(key.to_string()))?;
        if node.is_dir {
            return Err(ArchiveError::IsADirectory(key.to_string()));
        }
        // ❗ Only a node opens: a sidecar's handle sits in the store too, keyed
        // by its hidden path, and must stay as unreachable as it is unlisted.
        self.store
            .open_read(key, node.size.unwrap_or(0), source, password, runtime)
    }

    /// Whether the item at `inner_path` carries xattrs from a sidecar or a pax
    /// record, so a caller can skip opening a byte source for one that doesn't.
    pub fn has_carried_xattrs(&self, inner_path: &str) -> bool {
        let key = normalize_lookup(inner_path);
        self.held_xattrs.contains_key(key) || self.lazy_sidecars.contains_key(key)
    }

    /// The xattrs the archive carries for the item at `inner_path` (a file or a
    /// folder): what the parse held for it, plus its zip sidecar, read now
    /// through `source`. Total: a sidecar that's missing, too big, encrypted
    /// without `password`, or malformed adds nothing and is logged, ❌ never an
    /// error, because the item's bytes are the copy.
    ///
    /// One read of the sidecar entry, buffered whole: it's metadata, and capped
    /// at [`apple_double::MAX_SIDECAR_BYTES`] whatever size the archive claims.
    pub async fn item_xattrs(
        &self,
        inner_path: &str,
        source: Arc<dyn ArchiveByteSource>,
        password: Option<&str>,
        runtime: &Runtime,
    ) -> Vec<Xattr> {
        let key = normalize_lookup(inner_path);
        let mut xattrs = self.held_xattrs.get(key).cloned().unwrap_or_default();
        if let Some(sidecar) = self.lazy_sidecars.get(key)
            && let Some(bytes) = self.read_sidecar(sidecar, source, password, runtime).await
        {
            for xattr in apple_double::parse(&bytes) {
                if !xattrs.iter().any(|held| held.name == xattr.name) {
                    xattrs.push(xattr);
                }
            }
        }
        xattrs
    }

    /// `sidecar`'s bytes, or `None` (logged) when it can't be read whole within
    /// the cap. The claimed size is checked BEFORE the entry opens, so an
    /// oversized sidecar never starts a decode.
    async fn read_sidecar(
        &self,
        sidecar: &LazySidecar,
        source: Arc<dyn ArchiveByteSource>,
        password: Option<&str>,
        runtime: &Runtime,
    ) -> Option<Vec<u8>> {
        if !sidecar.claimed_size_fits() {
            return None;
        }
        let reader = match self
            .store
            .open_read(&sidecar.key, sidecar.size, source, password, runtime)
        {
            Ok(reader) => reader,
            Err(e) => {
                log::debug!(target: "archive", "Couldn't open the sidecar {}: {e}", LogPrivate::file_path(&sidecar.key), e = LogError(&e));
                return None;
            }
        };
        sidecar.read_capped(reader).await
    }

    /// The uncompressed size of the file at `inner_path`, or `None` for a missing
    /// path or a directory. Used by the sequential one-pass extractor to size a
    /// member from the tree without touching the store.
    pub(super) fn file_size(&self, inner_path: &str) -> Option<u64> {
        let node = self.nodes.get(normalize_lookup(inner_path))?;
        (!node.is_dir).then_some(node.size.unwrap_or(0))
    }

    /// Opens a ONE-PASS extractor over the file subtree rooted at `inner_path`,
    /// decoding the archive stream a single time (see [`super::extract`]). Yields
    /// each FILE member (files + symlinks) under the root in ARCHIVE order; the
    /// directory structure is NOT yielded — the caller creates it from the tree.
    ///
    /// The set of wanted files (with their tree sizes, so they match the copy
    /// scan totals, and their tree dates) is computed here from the parsed tree, then handed to the
    /// format's producer. Meant for SEQUENTIAL formats (compressed tar, 7z); a
    /// random-access store yields an empty stream (the planner never routes those
    /// here).
    pub fn open_subtree_extract(
        &self,
        inner_path: &str,
        source: Arc<dyn ArchiveByteSource>,
        password: Option<&str>,
        runtime: &Runtime,
    ) -> SubtreeExtractReader {
        let root = normalize_lookup(inner_path);
        let mut wanted: HashMap<String, SubtreeMember> = HashMap::new();
        let member = |path: &str, node: &ArchiveNode| SubtreeMember {
            inner_path: path.to_string(),
            size: self.file_size(path).unwrap_or(0),
            modified: node.modified,
            mode: node.mode,
            // A sequential format reads its sidecars during the parse, so a
            // member's xattrs are already in hand; nothing to read mid-decode.
            xattrs: self.held_xattrs.get(path).cloned().unwrap_or_default(),
        };

        // A file root extracts just itself; a directory root walks its subtree.
        // Either way, only FILE nodes carry bytes to stream.
        match self.nodes.get(root) {
            Some(node) if !node.is_dir => {
                wanted.insert(root.to_string(), member(root, node));
            }
            _ => {
                let mut pending = vec![root.to_string()];
                while let Some(dir) = pending.pop() {
                    let Some(child_paths) = self.children.get(&dir) else {
                        continue;
                    };
                    for child in child_paths {
                        match self.nodes.get(child) {
                            Some(node) if node.is_dir => pending.push(child.clone()),
                            Some(node) => {
                                wanted.insert(child.clone(), member(child, node));
                            }
                            None => {}
                        }
                    }
                }
            }
        }

        self.store.spawn_subtree_extract(source, wanted, password, runtime)
    }
}

/// Trims leading/trailing slashes so a caller's inner path (which may arrive as
/// `/foo/` or `foo`) matches the stored keys (no surrounding slashes; root is
/// `""`).
fn normalize_lookup(inner_path: &str) -> &str {
    inner_path.trim_matches('/')
}

/// Sanitizes and classifies each parsed entry (format-neutrally), then hands the
/// accepted seeds to [`build_tree`]. Stashes each readable file's per-format read
/// handle (pruned to the entries that end up as real file nodes) and records
/// quarantined names. Generic over the handle type `H`, so every format shares
/// the Zip Slip defense, the synthetic-tree logic, and the DoS caps.
///
/// Sidecars (`layout`) pair AFTER the sanitizer, so a pairing only ever names an
/// accepted path, and never become nodes: listing, the copy scan, and the walk
/// never see one (`crate::apple_double`).
pub(super) fn build_index<H>(
    entries: Vec<(RawEntry, H)>,
    layout: SidecarLayout,
) -> Result<BuiltIndex<H>, ArchiveError> {
    let mut quarantined: Vec<(String, QuarantineReason)> = Vec::new();
    let mut has_encrypted = false;

    // Pass 1: sanitize every name once and say what it is.
    let mut accepted: Vec<(String, EntryRole, RawEntry, H)> = Vec::with_capacity(entries.len());
    for (raw, handle) in entries {
        has_encrypted |= raw.encrypted;
        match sanitize_entry_name(&raw.name) {
            SanitizedName::Quarantined(reason) => quarantined.push((raw.name, reason)),
            SanitizedName::Accepted(path) => {
                let role = sidecars::entry_role(&path, &raw, layout);
                accepted.push((path, role, raw, handle));
            }
        }
    }

    // A `._name` pairs only when `name` is there; otherwise it's the only copy
    // of what it holds.
    let present = sidecars::present_siblings(accepted.iter().map(|(path, role, _, _)| (path.as_str(), role)));

    // Pass 2, in archive order: the tree's collisions are first-writer-wins.
    let mut seeds: Vec<NodeSeed> = Vec::with_capacity(accepted.len());
    let mut files: HashMap<String, H> = HashMap::new();
    let mut sidecar_handles: HashMap<String, H> = HashMap::new();
    let mut lazy_sidecars: HashMap<String, LazySidecar> = HashMap::new();
    let mut held = HeldXattrs::default();
    for (path, role, raw, handle) in accepted {
        match role {
            EntryRole::MacosxSidecar(target) => {
                // Later duplicate wins, like a file's.
                lazy_sidecars.insert(
                    target,
                    LazySidecar {
                        key: path.clone(),
                        size: raw.size,
                    },
                );
                sidecar_handles.insert(path, handle);
            }
            EntryRole::MacosxOther => {}
            EntryRole::SiblingCandidate(target) if present.contains(&target) => match raw.sidecar_bytes {
                Some(bytes) => held.add(&target, apple_double::parse(&bytes)),
                None => log::warn!(
                    target: "archive",
                    "Skipped the {}-byte sidecar {}: past the cap, so its item lands without its xattrs",
                    raw.size,
                    LogPrivate::file_path(&path)
                ),
            },
            EntryRole::Item | EntryRole::SiblingCandidate(_) => {
                held.add(&path, raw.xattrs);
                seeds.push(NodeSeed {
                    path: path.clone(),
                    is_dir: raw.is_dir,
                    is_symlink: raw.is_symlink,
                    size: if raw.is_dir { None } else { Some(raw.size) },
                    compressed_size: if raw.is_dir { None } else { Some(raw.compressed_size) },
                    modified: raw.modified,
                    encrypted: raw.encrypted,
                    mode: raw.mode,
                });
                if !raw.is_dir {
                    // Later duplicate wins (some archives carry repeat names).
                    files.insert(path, handle);
                }
            }
        }
    }
    if held.dropped > 0 {
        log::warn!(
            target: "archive",
            "Dropped {} xattrs past the {MAX_HELD_XATTR_BYTES}-byte cap; their items land without them",
            held.dropped
        );
    }

    let (nodes, children) = build_tree(seeds, MAX_TREE_NODES)?;
    // A file the tree dropped (shadowed by a directory, or blocked by a file
    // ancestor) must not stay readable via `open_read`: keep a handle only for
    // paths that ended up as real file nodes.
    files.retain(|path, _| nodes.get(path).is_some_and(|node| !node.is_dir));
    // A sidecar matters only beside an item the tree holds.
    lazy_sidecars.retain(|target, _| nodes.contains_key(target));
    for sidecar in lazy_sidecars.values() {
        if let Some(handle) = sidecar_handles.remove(&sidecar.key) {
            files.insert(sidecar.key.clone(), handle);
        }
    }
    let mut held_xattrs = held.by_path;
    held_xattrs.retain(|path, xattrs| !xattrs.is_empty() && nodes.contains_key(path));
    Ok(BuiltIndex {
        nodes,
        children,
        quarantined,
        has_encrypted,
        files,
        held_xattrs,
        lazy_sidecars,
    })
}

/// Builds the directory tree from accepted entry seeds. Pure — no I/O — so the
/// synthetic-dir logic (implied ancestors, explicit-vs-implied, collisions) is
/// unit-tested directly.
///
/// Returns the node map (every path, files and dirs, including the root `""`)
/// and the per-directory child lists, pre-sorted directories-first then by
/// case-insensitive name.
/// The built tree: every node by inner path, plus each directory's pre-sorted
/// child-path list.
type BuiltTree = (HashMap<String, ArchiveNode>, HashMap<String, Vec<String>>);

fn build_tree(seeds: Vec<NodeSeed>, max_nodes: usize) -> Result<BuiltTree, ArchiveError> {
    let mut nodes: HashMap<String, ArchiveNode> = HashMap::new();
    let mut child_paths: HashMap<String, Vec<String>> = HashMap::new();
    // Every path whose parent link is already recorded. A path has exactly one
    // parent, so this global set prevents double-linking.
    let mut linked: HashSet<String> = HashSet::new();

    nodes.insert(String::new(), ArchiveNode::root());

    for seed in seeds {
        // Path collisions are resolved first-writer-wins. If an ancestor path is
        // already a FILE, this entry can't be placed under it — drop it rather
        // than leaving it as an unreachable orphan.
        if !ensure_ancestors(&seed.path, &mut nodes, &mut child_paths, &mut linked) {
            continue;
        }

        if seed.is_dir {
            // Create the dir, or upgrade an implied one with its real mtime. If a
            // file already holds this exact path, `upsert_dir` returns false and
            // the file wins — drop the dir entry.
            upsert_dir(&seed.path, seed.modified, &mut nodes, &mut child_paths, &mut linked);
        } else {
            // Yield to an existing directory at this path (the directory, with
            // children, wins). Otherwise create the file node, or overwrite an
            // earlier file of the same name (a later duplicate wins).
            let occupied_by_dir = nodes.get(&seed.path).is_some_and(|n| n.is_dir);
            if !occupied_by_dir {
                let node = ArchiveNode {
                    name: leaf_name(&seed.path).to_string(),
                    path: seed.path.clone(),
                    is_dir: false,
                    is_symlink: seed.is_symlink,
                    size: seed.size,
                    compressed_size: seed.compressed_size,
                    modified: seed.modified,
                    encrypted: seed.encrypted,
                    mode: seed.mode,
                };
                link_child(&seed.path, &mut child_paths, &mut linked);
                nodes.insert(seed.path.clone(), node);
            }
        }

        // Backstop against the many-entries amplification axis: refuse an
        // oversized tree rather than risk an OOM. One seed adds at most
        // MAX_COMPONENT_DEPTH nodes, so we overshoot the cap by a bounded margin
        // before catching it here.
        if nodes.len() > max_nodes {
            return Err(ArchiveError::TooLarge(format!(
                "directory tree exceeds the {max_nodes}-node limit"
            )));
        }
    }

    sort_children(&mut child_paths, &nodes);
    Ok((nodes, child_paths))
}

/// Ensures every ancestor directory of `path` exists as a synthetic dir node,
/// shallowest-first, so a parent is always created before its child. Returns
/// `false` if an ancestor path is already occupied by a FILE (so `path` can't be
/// placed under it); the caller drops the entry.
fn ensure_ancestors(
    path: &str,
    nodes: &mut HashMap<String, ArchiveNode>,
    child_paths: &mut HashMap<String, Vec<String>>,
    linked: &mut HashSet<String>,
) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    for depth in 1..parts.len() {
        let dir_path = parts[..depth].join("/");
        if !upsert_dir(&dir_path, None, nodes, child_paths, linked) {
            return false;
        }
    }
    true
}

/// Inserts a directory node at `dir_path` (creating it if absent) and links it
/// into its parent. If it already exists as a synthetic dir and `modified` is
/// `Some` (an explicit entry), upgrades its timestamp. Returns `false` if a FILE
/// already holds this exact path (first-writer-wins: the file keeps it).
fn upsert_dir(
    dir_path: &str,
    modified: Option<i64>,
    nodes: &mut HashMap<String, ArchiveNode>,
    child_paths: &mut HashMap<String, Vec<String>>,
    linked: &mut HashSet<String>,
) -> bool {
    match nodes.get_mut(dir_path) {
        Some(node) if node.is_dir => {
            // Existing dir: let an explicit entry fill in the real mtime.
            if node.modified.is_none() {
                node.modified = modified;
            }
            true
        }
        Some(_) => false, // a file already claimed this path
        None => {
            let node = ArchiveNode {
                name: leaf_name(dir_path).to_string(),
                path: dir_path.to_string(),
                is_dir: true,
                is_symlink: false,
                size: None,
                compressed_size: None,
                modified,
                encrypted: false,
                // A synthesized directory node: nothing recorded a mode for it.
                mode: None,
            };
            link_child(dir_path, child_paths, linked);
            nodes.insert(dir_path.to_string(), node);
            true
        }
    }
}

/// Records `path` as a child of its parent directory, once.
fn link_child(path: &str, child_paths: &mut HashMap<String, Vec<String>>, linked: &mut HashSet<String>) {
    if !linked.insert(path.to_string()) {
        return;
    }
    let parent = parent_path(path);
    child_paths
        .entry(parent.to_string())
        .or_default()
        .push(path.to_string());
}

/// Sorts every directory's children: directories first, then case-insensitive
/// by name. Deterministic output for a stable listing.
fn sort_children(child_paths: &mut HashMap<String, Vec<String>>, nodes: &HashMap<String, ArchiveNode>) {
    for paths in child_paths.values_mut() {
        paths.sort_by(|a, b| {
            let na = &nodes[a];
            let nb = &nodes[b];
            nb.is_dir
                .cmp(&na.is_dir)
                .then_with(|| na.name.to_lowercase().cmp(&nb.name.to_lowercase()))
                .then_with(|| na.name.cmp(&nb.name))
        });
    }
}

/// The final path component of an inner path.
fn leaf_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The parent directory of an inner path (`""` for a top-level entry).
fn parent_path(path: &str) -> &str {
    match path.rfind('/') {
        Some(idx) => &path[..idx],
        None => "",
    }
}

#[cfg(test)]
#[path = "index_test.rs"]
mod tests;
