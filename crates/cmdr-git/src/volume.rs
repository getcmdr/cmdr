//! [`GitPortalVolume`]: a read-only [`Volume`] over one repo's virtual `.git`
//! trees.
//!
//! The same shape `ArchiveVolume` has. A path crossing `.git/<category>/` isn't
//! a hook inside the local backend: `VolumeManager::resolve` routes it here, and
//! this volume maps the whole input path to `(repo, category, ref, tree path)`
//! through [`path::classify_in`], so `ResolvedVolume.path` stays the caller's
//! path verbatim.
//!
//! ## What it serves, and what it doesn't
//!
//! Its namespace is the SIX virtual categories and everything under them.
//! `root()` is `<worktree>/.git`, and listing that root answers the six category
//! rows alone: the real `.git/*` entries (`HEAD`, `config`, `hooks/`) are the
//! parent volume's, stay writable there, and reach the pane through the listing
//! overlay rather than through here. Anything else — `.git/config`,
//! `.git/objects/…`, a `.git` that isn't a repository at all — is `NotFound`,
//! because this volume genuinely doesn't hold it.
//!
//! ## Read-only by construction
//!
//! `is_writable()` is false and every mutation keeps the trait's `NotSupported`
//! default, so nothing has to remember a guard: a git tree is a snapshot of
//! objects, and the way to change one is `git` in a terminal. It can't watch
//! either (`can_watch_listings()` false, `listing_watch_coverage` `None`): the
//! paths don't exist on disk, so `notify` has nothing to arm on. Invalidation
//! comes from the per-repo `.git/*` watcher (`watcher.rs`) instead.

use cmdr_fs::app_path::AppPath;
use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::app_path::BridgedPath;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::listing_progress::report_while;
use cmdr_fs::volume::scan_walk::{self, ScanSource, Walking};

use crate::fault_guard::GuardedRead;
use crate::path::{self, Cat, VirtualGitPath};
use crate::portal::GitPortal;
use crate::repo::RepoHandle;
use crate::{Lookup, found_or_not_found, list_ref_tree, log, stash, submodules, virtual_listing, worktrees};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{
    BatchScanResult, CopyScanResult, DirectoryCreation, LaneKey, ListingProgress, ScanBoundary, SpaceInfo, Volume,
    VolumeError, VolumeReadStream,
};

/// How often a running commit walk reports how far it got: fast enough that a
/// long walk visibly climbs, slow enough that the number stays readable. The
/// local reader's cadence.
const PROGRESS_SAMPLE_INTERVAL: Duration = Duration::from_millis(200);

/// Tells a commit walk to stop when the listing that started it is dropped.
struct AbandonOnDrop(Arc<log::WalkControl>);

impl Drop for AbandonOnDrop {
    fn drop(&mut self) {
        self.0.abandon();
    }
}

/// A read-only [`Volume`] over one repository's virtual `.git` trees.
pub struct GitPortalVolume {
    /// The repo handles and the host this volume shares with every other portal
    /// volume in the process.
    portal: Arc<GitPortal>,
    /// `<worktree>/.git`: this volume's root, and the prefix of every path it
    /// serves.
    dot_git: BridgedPath,
    /// The volume physically holding the repo. Source of the shared lane key
    /// (portal work must not run in parallel with other work on the same disk)
    /// and of the space answer.
    parent: Arc<dyn Volume>,
}

impl GitPortalVolume {
    /// Builds the portal volume for the worktree at `repo_root`, over `portal`
    /// and backed by `parent`.
    ///
    /// `parent` is the volume physically holding the repo: this one borrows its
    /// lane key and its free space, the way an `ArchiveVolume` borrows the volume
    /// holding the `.zip`. Cheap and infallible; the repository is opened on
    /// first use through the portal's shared cache, and a `.git` that isn't one
    /// answers `NotFound` rather than failing to construct.
    pub fn new(portal: Arc<GitPortal>, repo_root: PathBuf, parent: Arc<dyn Volume>) -> Self {
        Self {
            portal,
            dot_git: BridgedPath::new(repo_root.join(".git")),
            parent,
        }
    }

    /// Runs `work` on the app's runtime rather than the caller's thread: every
    /// `gix` call below is blocking, and a listing of a big repo would otherwise
    /// stall an async worker.
    ///
    /// Whatever `work` answers is refused as `VolumeGone` when a mapped page went
    /// away while it ran, or the repo's volume stopped it because it's about to
    /// unmount (`fault_guard.rs`). `work` gets the read, for a walk to poll.
    fn blocking<T, F>(&self, work: F) -> Pin<Box<dyn Future<Output = Result<T, VolumeError>> + Send>>
    where
        T: Send + 'static,
        F: FnOnce(&GuardedRead) -> Result<T, VolumeError> + Send + 'static,
    {
        let worktree_root = self.dot_git.parent().unwrap_or(&self.dot_git).to_path_buf();
        let guarded = move || {
            let read = GuardedRead::begin(&worktree_root);
            let answer = work(&read);
            match read.refusal() {
                Some(refused) => Err(VolumeError::FriendlyGit(refused)),
                None => answer,
            }
        };
        let task = self.portal.host().runtime().spawn_blocking(guarded);
        Box::pin(async move {
            task.await
                .expect("the git portal's blocking closure doesn't panic and the task is uncancelable")
        })
    }
}

impl Volume for GitPortalVolume {
    /// The name of the thing at [`root`](Volume::root), as every backend
    /// answers it. The registry id carries which repo this is.
    fn name(&self) -> &str {
        ".git"
    }

    fn backend_kind(&self) -> cmdr_fs::volume::BackendKind {
        cmdr_fs::volume::BackendKind::GitPortal
    }

    fn root(&self) -> &AppPath {
        self.dot_git.app()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// The **parent's** lane key: reading a snapshot still reads the repo's
    /// object database off the parent's disk, so the two must not be scheduled
    /// as independent resources.
    fn lane_key(&self) -> LaneKey {
        self.parent.lane_key()
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        let owned = path.to_path_buf();
        let portal = Arc::clone(&self.portal);
        let control = Arc::new(log::WalkControl::default());
        let walk_control = Arc::clone(&control);
        let listing = self.blocking(move |read| {
            // A volume about to unmount stops the commit walk the way a pane leaving does.
            walk_control.stop_when(read.stop_flag());
            let Some((virt, handle, root)) = path::classify_in(portal.repos(), &owned) else {
                return Err(VolumeError::NotFound(owned.display().to_string()));
            };
            found_or_not_found(listing_for(&virt, &handle, &root, &walk_control), &owned)
        });
        Box::pin(async move {
            // The blocking walk can't be aborted, so a pane that leaves (dropping
            // this future) tells it to stop instead.
            let _abandon_on_drop = AbandonOnDrop(Arc::clone(&control));
            // Only the commit walk counts as it goes (every row is a folder); every
            // other listing is quick and reports once, below.
            let sample = || ListingProgress {
                files: 0,
                dirs: control.listed(),
                bytes: 0,
                ..ListingProgress::default()
            };
            let entries = report_while(listing, PROGRESS_SAMPLE_INTERVAL, sample, on_progress).await?;
            if let Some(callback) = on_progress {
                callback(ListingProgress::of(&entries));
            }
            Ok(entries)
        })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        let owned = path.to_path_buf();
        let portal = Arc::clone(&self.portal);
        self.blocking(move |_| {
            let Some((virt, handle, root)) = path::classify_in(portal.repos(), &owned) else {
                return Err(VolumeError::NotFound(owned.display().to_string()));
            };
            found_or_not_found(virtual_listing::get_metadata_for(&root, &virt, &handle), &owned)
        })
    }

    // ---- Read-only: a snapshot is changed with `git`, not with a file manager

    /// ❌ Never flip this to track "the user can write to this repo". They can,
    /// through the parent volume, which is what serves every real file under
    /// `.git`. This volume holds objects that already exist, addressed by
    /// content; there is nothing here to write to.
    fn is_writable(&self) -> bool {
        false
    }

    /// `create_file`, `create_directory`, `delete`, `rename`, and
    /// `write_from_stream` all inherit the trait's `NotSupported` default. Only
    /// this one is overridden: the default walks `exists()` and answers
    /// `Ok(AlreadyExisted)` for a directory that's already there, which would
    /// have a read-only volume claim it created something.
    fn create_directory_all<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<DirectoryCreation, VolumeError>> + Send + 'a>> {
        let _ = path;
        Box::pin(async { Err(VolumeError::NotSupported) })
    }

    // ---- Copy out: streaming reads and the scan that plans them ------------

    fn supports_export(&self) -> bool {
        true
    }

    /// Reports the mode: a tree entry's kind IS its mode (`BlobExecutable` is
    /// `0o755`, `Blob` is `0o644`), which lets a script copied out of a snapshot
    /// still run. Read-only, so it keeps nothing.
    fn metadata_caps(&self) -> cmdr_fs::metadata::MetadataCaps {
        cmdr_fs::metadata::MetadataCaps::MODE_REPORTER
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        let owned = path.to_path_buf();
        let portal = Arc::clone(&self.portal);
        self.blocking(move |_| {
            let Some((virt, handle, _root)) = path::classify_in(portal.repos(), &owned) else {
                return Err(VolumeError::NotFound(owned.display().to_string()));
            };
            let opened = match &virt {
                VirtualGitPath::RefTree(cat, name, sub) if cat.browses_commit_tree() => {
                    crate::open_blob(&handle, *cat, name, sub)
                }
                // A category, a ref, or a leaf row: a directory has no bytes.
                _ => return Err(VolumeError::NotSupported),
            };
            found_or_not_found(opened, &owned)
        })
    }

    fn scan_for_copy<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<CopyScanResult, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let path_std = path.as_std_legacy();
            let path: &Path = &path_std;
            scan_walk::scan_one(self, path).await
        })
    }

    fn scan_for_copy_batch_with_boundary<'a>(
        &'a self,
        paths: &'a [AppPathBuf],
        boundary: &'a ScanBoundary,
    ) -> Pin<Box<dyn Future<Output = Result<BatchScanResult, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let paths_std: Vec<PathBuf> = paths.iter().map(|p| p.as_std_legacy().into_owned()).collect();
            let paths: &[PathBuf] = &paths_std;
            scan_walk::scan_trees(self, paths, boundary).await
        })
    }

    // ---- Capability flags: answered explicitly, never inherited ------------

    /// `true`: this volume's root is `<worktree>/.git`, a path inside the volume
    /// that holds the repository. See [`Volume::routes_over_a_parent`].
    fn routes_over_a_parent(&self) -> bool {
        true
    }

    /// `None`: a virtual path has no counterpart on disk, so there's no
    /// `copyfile(2)` fast path to advertise.
    fn local_path(&self) -> Option<AppPathBuf> {
        None
    }

    /// `false`: `.git/branches/main/README.md` can't be `stat`ed or read with
    /// `std::fs`. It's an object in a pack file.
    fn supports_local_fs_access(&self) -> bool {
        false
    }

    /// `None`: a snapshot's size doesn't move, so polling it would be pure
    /// churn (the trait default is every two seconds).
    fn space_poll_interval(&self) -> Option<Duration> {
        None
    }

    /// The parent's. A repo's objects live on the parent's disk, and a copy OUT
    /// of a snapshot lands wherever the destination is, so the parent's free
    /// space is the honest answer; reporting zero would read as "disk full" to
    /// the pre-copy space check.
    fn get_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        Box::pin(async move { self.parent.get_space_info().await })
    }

    /// The parent's answer for the filesystem holding the repo, whatever `path` is
    /// asked about: a repo on a phone's SD card sits on the card, which the
    /// parent's volume-wide figure doesn't describe.
    fn get_space_info_at<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            self.parent
                .get_space_info_at(&AppPath::legacy_unchecked(&self.dot_git))
                .await
        })
    }

    /// `false`, and `listing_watch_coverage` stays at the trait's `None`: these
    /// paths don't exist on disk, so `notify` has nothing to arm on (it answers "No
    /// path was found" and spams the warn log). Invalidation arrives from the
    /// per-repo `.git/*` watcher instead (`watcher.rs`).
    fn can_watch_listings(&self) -> bool {
        false
    }
}

/// What a classified path lists. A free function so the blocking closure needs
/// only the classified triple, never the volume.
fn listing_for(
    virt: &VirtualGitPath,
    handle: &RepoHandle,
    root: &Path,
    control: &log::WalkControl,
) -> Lookup<Vec<FileEntry>> {
    match virt {
        VirtualGitPath::Root => Ok(Some(virtual_listing::list_categories(handle, root))),
        VirtualGitPath::Category(Cat::Branches) => virtual_listing::list_branches(handle, root).map(Some),
        VirtualGitPath::Category(Cat::Tags) => virtual_listing::list_tags(handle, root).map(Some),
        VirtualGitPath::Category(Cat::Commits) => log::list_commits(handle, root, control).map(Some),
        VirtualGitPath::Category(Cat::Stash) => stash::list_stashes(root).map(Some),
        VirtualGitPath::Category(Cat::Worktrees) => worktrees::list_worktrees(handle, root).map(Some),
        VirtualGitPath::Category(Cat::Submodules) => submodules::list_submodules(handle, root).map(Some),
        VirtualGitPath::Ref(cat, name) if cat.browses_commit_tree() => list_ref_tree(handle, root, *cat, name, ""),
        VirtualGitPath::RefTree(cat, name, sub) if cat.browses_commit_tree() => {
            list_ref_tree(handle, root, *cat, name, sub)
        }
        // A worktree or submodule row is a leaf carrying `redirectToPath`; the
        // frontend redirects on Enter, so listing one is empty, not missing.
        VirtualGitPath::Ref(_, _) | VirtualGitPath::RefTree(_, _, _) => Ok(Some(Vec::new())),
    }
}

/// The two reads `scan_walk` needs, so `scan_for_copy` and the batch scan behind
/// every transfer come for free — that's what makes copying a whole branch tree
/// out to another volume work at all.
impl ScanSource for GitPortalVolume {
    fn scan_stat<'a>(&'a self, path: &'a Path) -> Walking<'a, FileEntry> {
        self.get_metadata(&AppPath::legacy_unchecked(path))
    }

    fn scan_list<'a>(&'a self, path: &'a Path) -> Walking<'a, Vec<FileEntry>> {
        self.list_directory(&AppPath::legacy_unchecked(path), None)
    }
}
