//! Archive routing for the [`VolumeManager`]: the half of
//! [`resolve`](VolumeManager::resolve) that sends a path crossing a `.zip`
//! boundary to a read-only, on-demand [`ArchiveVolume`], plus the LRU that caps
//! how many stay registered.
//!
//! The plain registry (hold volumes by ID) lives in the parent `manager`
//! module, the git-portal route in `git_routing.rs`, and the dispatcher over
//! both in `routing.rs`. This is one more inherent `impl VolumeManager` block
//! (a type's impl can span files within a crate), so every method stays at
//! `VolumeManager::…` regardless of which file it's in.

use super::super::{Volume, WatchCoverage};
use super::VolumeManager;
use super::routing::ArchiveRoot;
use crate::file_system::volume::{ResolvedVolume, RoutedKind};
use crate::ignore_poison::IgnorePoison;
use cmdr_archive::{
    ArchiveFormat, ArchiveVolume, confirm_archive_boundary, confirm_archive_boundary_on_parent,
    confirm_remote_archive_boundary, format_for_path,
};
use cmdr_fs::app_path::AppPath;
use cmdr_fs::archive_format::{archive_boundary_candidate, archive_boundary_candidates};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Max number of `ArchiveVolume`s kept registered at once. Browsing many zips
/// must not leak volumes + parents + index caches, so the archive LRU evicts the
/// least-recently-resolved archive past this cap. Eviction is harmless: the next
/// navigation re-resolves and re-registers lazily (`ArchiveVolume::new` is cheap;
/// the index re-parses on demand).
const ARCHIVE_LRU_CAP: usize = 16;

/// What an FSEvents watch on `backing_path`'s directory can observe, which is the
/// ceiling on what an archive's content watch can honestly report.
///
/// An archive on an OS-mounted share is watched by FSEvents like any other local
/// path, and FSEvents there is blind to other clients, so the archive inherits
/// [`WatchCoverage::ThisMachineOnly`]. A remote-BACKED archive (direct SMB / MTP
/// parent) never gets here: its watch can't arm at all.
///
/// One `statfs`, called once per archive registration.
fn watch_coverage_for_backing_file(backing_path: &Path) -> WatchCoverage {
    let on_network = backing_path
        .parent()
        .is_some_and(cmdr_local::filesystem_kind::path_is_on_network_mount);
    if on_network {
        WatchCoverage::ThisMachineOnly
    } else {
        WatchCoverage::EveryWriter
    }
}

impl VolumeManager {
    /// The archive half of [`resolve`](Self::resolve): routes a path that
    /// crosses a `.zip` boundary to the read-only [`ArchiveVolume`] for that
    /// archive, registering it on demand.
    ///
    /// No archive-extension component → a plain [`get`](Self::get) with the path
    /// unchanged, and zero I/O (the pure candidate check gates everything below).
    /// A confirmed boundary (a path component that's a real archive FILE, magic-
    /// byte checked) → `register_if_absent` the archive under `archive-{hash(zip)}`,
    /// bump the LRU, and return `(archive_volume, path)`. The returned path is
    /// ALWAYS the input path — `ArchiveVolume` maps it to its inner namespace
    /// itself (see [`ResolvedVolume`]).
    ///
    /// Confirmation is parent-aware, which is why this is `async`: a local
    /// parent confirms with a `std::fs` stat and sniff, a remote one (direct SMB /
    /// MTP) through its own I/O. `cmdr-archive` owns both halves
    /// ([`confirm_archive_boundary_on_parent`]).
    pub(super) async fn resolve_archive(&self, volume_id: &str, path: &Path) -> ResolvedVolume {
        // Pure string pre-filter: no archive-extension component ⇒ zero I/O on
        // the hot listing path (neither disk nor network is touched here).
        let Some((zip_path, _inner)) = archive_boundary_candidate(path) else {
            return ResolvedVolume::passthrough(self.get(volume_id), path);
        };

        // The requested volume physically holds the `.zip`, so it's the archive's
        // parent (shared lane key, space info, and the remote byte source).
        let Some(parent) = self.get(volume_id) else {
            return ResolvedVolume::passthrough(None, path);
        };

        if confirm_archive_boundary_on_parent(parent.as_ref(), path)
            .await
            .is_none()
        {
            return ResolvedVolume::passthrough(Some(parent), path);
        }

        let archive_id = archive_volume_id(&zip_path);
        self.register_archive(archive_id, volume_id, parent, zip_path, path, RoutedKind::Archive)
            .unwrap_or_else(|| ResolvedVolume::passthrough(self.get(volume_id), path))
    }

    /// An archive INSIDE a restic snapshot (#631): the first archive-named
    /// component below `view`'s root, confirmed and read THROUGH the view, as
    /// [`RoutedKind::ArchiveInSnapshot`]. `None` when there's no such component,
    /// when `path` is the archive itself and `archive_root` says that's a file,
    /// or when the view can't vouch for it (the repo is locked, the name is a
    /// folder, the magic doesn't match); the view then answers for the path.
    ///
    /// The confirm is the remote one: a snapshot's files are blobs in pack files,
    /// never something `std::fs` can stat. ❌ No `canonicalize` for the id either:
    /// the path isn't real, and on an OS-mounted share a stat of it is a network
    /// round trip that a wedged mount turns into minutes.
    pub(super) async fn resolve_archive_in_snapshot(
        &self,
        view_id: &str,
        view: Arc<dyn Volume>,
        path: &Path,
        archive_root: ArchiveRoot,
    ) -> Option<ResolvedVolume> {
        let (zip_path, inner) = archive_boundary_candidates(path).find(|(zip, _)| {
            zip.starts_with(&*view.root().as_std_legacy()) && zip.as_path() != &*view.root().as_std_legacy()
        })?;
        if inner.as_os_str().is_empty() && archive_root == ArchiveRoot::File {
            return None;
        }
        if !confirm_remote_archive_boundary(view.as_ref(), &zip_path).await {
            return None;
        }
        let archive_id = archive_in_snapshot_volume_id(&view, &zip_path);
        self.register_archive(archive_id, view_id, view, zip_path, path, RoutedKind::ArchiveInSnapshot)
    }

    /// The archive half of [`resolve_local_only`](Self::resolve_local_only):
    /// confirms LOCAL boundaries only, because a remote confirm needs async I/O
    /// its caller can't do.
    pub(super) fn resolve_local_archive(&self, volume_id: &str, path: &Path) -> ResolvedVolume {
        let Some((zip_path, _inner)) = confirm_archive_boundary(path) else {
            return ResolvedVolume::passthrough(self.get(volume_id), path);
        };
        let Some(parent) = self.get(volume_id) else {
            return ResolvedVolume::passthrough(None, path);
        };
        let archive_id = archive_volume_id(&zip_path);
        self.register_archive(archive_id, volume_id, parent, zip_path, path, RoutedKind::Archive)
            .unwrap_or_else(|| ResolvedVolume::passthrough(self.get(volume_id), path))
    }

    /// Async, parent-aware sibling of the sync
    /// [`cmdr_archive::path_is_inside_archive`]: true when `path` points INSIDE a
    /// confirmed archive (a non-empty inner path), for BOTH local and remote
    /// (direct SMB / MTP) parents. The `.zip` file itself is a plain file → false.
    ///
    /// The write-routing seams (delete / rename / copy-out source) use this so a
    /// remote archive-inner write reaches the managed edit driver instead of
    /// falling through to a confusing parent-volume write. The sync free function
    /// is `std::fs`-only, so it silently returns false for `smb://` / `mtp://`
    /// paths — the bug this closes. Confirmation is [`resolve`](Self::resolve)'s,
    /// [`confirm_archive_boundary_on_parent`].
    pub async fn path_is_inside_archive(&self, volume_id: &str, path: &Path) -> bool {
        self.confirm_boundary_on_parent(volume_id, path)
            .await
            .is_some_and(|(_zip, inner)| !inner.as_os_str().is_empty())
    }

    /// Async, parent-aware sibling of the sync
    /// [`cmdr_archive::path_crosses_archive_boundary`]: true when `path` crosses
    /// into a confirmed archive — the `.zip` file itself (empty inner) OR a path
    /// inside it.
    ///
    /// `create` uses this (not [`path_is_inside_archive`](Self::path_is_inside_archive)):
    /// a new entry's parent can BE the `.zip` file (creating at the archive root),
    /// which must still route to the managed edit driver.
    pub async fn path_crosses_archive_boundary(&self, volume_id: &str, path: &Path) -> bool {
        self.confirm_boundary_on_parent(volume_id, path).await.is_some()
    }

    /// Shared confirm behind the two async boundary predicates above: the
    /// `(zip_path, inner_path)` split when `path` crosses into a CONFIRMED archive
    /// on `volume_id`'s volume, else `None` (not an archive path, or the volume
    /// isn't registered).
    async fn confirm_boundary_on_parent(&self, volume_id: &str, path: &Path) -> Option<(PathBuf, PathBuf)> {
        // The pure string check first, so a plain path never takes the registry lock.
        archive_boundary_candidate(path)?;
        let parent = self.get(volume_id)?;
        confirm_archive_boundary_on_parent(parent.as_ref(), path).await
    }

    /// Registers (or reuses) the [`ArchiveVolume`] for a confirmed `.zip` under
    /// `archive_id`, bumps the LRU, and returns it resolved as `kind`. Shared by
    /// [`resolve`](Self::resolve) and [`resolve_local_only`](Self::resolve_local_only);
    /// the confirm step (the only local-vs-remote difference) happens before this.
    /// `None` when it was evicted before it could be read back, which only a
    /// pathologically small cap reaches; the caller picks the fallback.
    fn register_archive(
        &self,
        archive_id: String,
        volume_id: &str,
        parent: Arc<dyn Volume>,
        zip_path: PathBuf,
        path: &Path,
        kind: RoutedKind,
    ) -> Option<ResolvedVolume> {
        // The format is decided from the confirmed archive's name. A confirmed
        // boundary always carries a supported extension, so `format_for_path`
        // yields `Some`; the `Zip` fallback only guards a future path shape.
        let format = format_for_path(&zip_path).unwrap_or(ArchiveFormat::Zip);
        let backing_path = zip_path.clone();
        let parent_is_local = parent.supports_local_fs_access();
        let archive = Arc::new(ArchiveVolume::new(parent, zip_path, format, crate::volume_host::host()));
        // Only the resolve that actually registered starts the content watch, so
        // repeated resolves of an already-registered archive don't churn notify
        // watchers. The watch lives on the registered `ArchiveVolume` and stops
        // when the LRU evicts it (the volume's `Arc` drops). `volume_id` is the
        // parent drive id the listing cache keys on. Gated on the app handle: the
        // watch only refreshes frontend listings, so there's nothing to watch
        // before the frontend exists — and this keeps headless unit tests from
        // starting real OS watches (production sets the handle at startup, before
        // any archive is browsed). A non-local parent (direct SMB / MTP, a restic
        // snapshot) gets no watch at all: notify can't watch an `smb://` or a
        // snapshot path, so its `listing_watch_coverage` stays `None`, which keeps
        // pre-op rescans honest, and its coverage isn't even asked.
        //
        // The coverage ceiling is resolved HERE because the mount probe is
        // per-platform and app-side: the watch is an FSEvents watch on the
        // archive's parent directory, so an archive on an OS-mounted share can
        // only ever report this machine's writes. One `statfs` per archive
        // registration, off any hot path (this runs once, and only when the
        // register actually happened).
        if self.register_if_absent(&archive_id, archive.clone())
            && parent_is_local
            && crate::file_system::watcher::app_handle_present()
        {
            archive.start_content_watch(volume_id, watch_coverage_for_backing_file(&backing_path));
        }
        self.touch_archive_lru(&archive_id);

        self.get(&archive_id).map(|volume| ResolvedVolume {
            volume: Some(volume),
            path: AppPath::legacy_unchecked(&path.to_path_buf()).into_owned(),
            routed: Some(kind),
            unconfirmed: None,
        })
    }

    /// Records `id` as the most-recently-resolved archive and unregisters the
    /// least-recently-resolved ones past [`ARCHIVE_LRU_CAP`] (each volume's
    /// `ArchiveIndexCache` drops with it). Eviction is lazy-safe: a later
    /// `resolve` re-registers. Unregisters OUTSIDE the LRU lock so the LRU and
    /// volumes locks are never held at once.
    fn touch_archive_lru(&self, id: &str) {
        let evicted = {
            let mut lru = self.archive_lru.lock_ignore_poison();
            super::routing::touch_routed_lru(&mut lru, id, ARCHIVE_LRU_CAP)
        };
        for old in evicted {
            self.unregister(&old);
        }
    }

    /// Drops the parsed index of the archive registered over `zip_path`, if any,
    /// so the next read sees an edit the app just committed. Nothing registered
    /// means nothing cached: the next resolve parses afresh anyway.
    pub fn forget_archive_index(&self, zip_path: &Path) {
        if let Some(volume) = self.get(&archive_volume_id(zip_path))
            && let Some(archive) = volume.as_any().downcast_ref::<ArchiveVolume>()
        {
            archive.forget_parsed_index();
        }
    }
}

/// The registry id for the `ArchiveVolume` over `zip_path`:
/// `archive-{hash(canonical zip path)}`. Canonicalized so two spellings of the
/// same file share one registration. This id is backend-internal only — it never
/// enters frontend state, history, or persistence (the FE holds the parent drive
/// id for display), so a fixed-seed hash that's stable within a process is all
/// it needs.
fn archive_volume_id(zip_path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let canonical = std::fs::canonicalize(zip_path).unwrap_or_else(|_| zip_path.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("archive-{:016x}", hasher.finish())
}

/// The registry id for an archive inside a restic snapshot: the zip's path
/// hashed with the VIEW it reads through, by identity. A view is re-minted when
/// its parent is replaced (an SMB share reconnecting), and an archive keyed by
/// path alone would keep reading through the retired one. The address can't be
/// reused while this archive lives, since the archive holds its view.
fn archive_in_snapshot_volume_id(view: &Arc<dyn Volume>, zip_path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::ptr::addr_of!(**view).cast::<()>().hash(&mut hasher);
    zip_path.hash(&mut hasher);
    format!("archive-in-snapshot-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    //! The registry half: routing, reuse, the LRU, and the two predicates' wiring.
    //! The confirm itself (folders named `foo.zip`, mislabeled files, the sniff's
    //! refuse-typed routing, the local-or-remote fork) is `cmdr-archive`'s,
    //! tested in its `boundary_on_parent_test.rs`.

    use super::*;
    use crate::file_system::volume::InMemoryVolume;
    use cmdr_fs::app_path::AppPath;

    /// Writes a file whose first bytes are a valid zip start-of-file signature.
    /// Enough for `confirm_archive_boundary`'s magic check (routing only; these
    /// tests never parse the archive).
    fn write_zip_magic(path: &Path) {
        std::fs::write(path, b"PK\x03\x04not-a-real-archive-body").expect("write zip magic");
    }

    /// Writes N zip-magic bytes into a non-local `InMemoryVolume` at `path`, so a
    /// resolve against it takes the REMOTE confirm path (parent metadata + a
    /// four-byte ranged read), the way a direct-SMB / MTP parent does.
    async fn remote_parent_with_zip(zip_path: &Path) -> InMemoryVolume {
        let parent = InMemoryVolume::new("Remote");
        parent
            .create_file(
                &AppPath::legacy_unchecked(zip_path),
                b"PK\x03\x04not-a-real-archive-body",
            )
            .await
            .expect("seed remote zip");
        parent
    }

    #[tokio::test]
    async fn resolve_passes_through_a_non_archive_path() {
        let manager = VolumeManager::new();
        manager.register("root", Arc::new(InMemoryVolume::new("Root")));

        let resolved = manager.resolve("root", Path::new("/some/plain/dir")).await;
        assert!(!resolved.is_routed());
        assert_eq!(*resolved.path.as_std_legacy(), PathBuf::from("/some/plain/dir"));
        assert_eq!(resolved.volume.expect("volume").name(), "Root");
    }

    #[tokio::test]
    async fn resolve_routes_an_archive_inner_path_to_an_archive_volume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("bundle.zip");
        write_zip_magic(&zip);

        let manager = VolumeManager::new();
        // A LOCAL parent (real temp file) takes the std::fs confirm path.
        manager.register("root", Arc::new(InMemoryVolume::new("Root").with_local_fs_access()));

        let inner = zip.join("docs/readme.txt");
        let resolved = manager.resolve("root", &inner).await;
        assert!(resolved.is_routed());
        // The path is handed back unchanged (the ArchiveVolume maps it itself).
        assert_eq!(*resolved.path.as_std_legacy(), inner);
        // The resolved volume is the archive: its root is the `.zip` path.
        assert_eq!(*resolved.volume.expect("archive volume").root().as_std_legacy(), zip);
    }

    #[tokio::test]
    async fn resolve_routes_a_remote_zip_via_the_parents_ranged_reads() {
        // No local file exists; the parent is non-local and serves the `.zip`
        // bytes from its in-memory store through `get_metadata` + `read_range`.
        let zip = PathBuf::from("/device/bundle.zip");
        let manager = VolumeManager::new();
        manager.register("root", Arc::new(remote_parent_with_zip(&zip).await));

        let inner = zip.join("docs/readme.txt");
        let resolved = manager.resolve("root", &inner).await;
        assert!(resolved.is_routed(), "a remote zip must route to an ArchiveVolume");
        assert_eq!(*resolved.path.as_std_legacy(), inner);
        assert_eq!(*resolved.volume.expect("archive volume").root().as_std_legacy(), zip);
    }

    #[tokio::test]
    async fn resolve_reuses_the_same_archive_volume_across_calls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("bundle.zip");
        write_zip_magic(&zip);

        let manager = VolumeManager::new();
        manager.register("root", Arc::new(InMemoryVolume::new("Root").with_local_fs_access()));

        let a = manager.resolve("root", &zip.join("a")).await.volume.expect("a");
        let b = manager.resolve("root", &zip.join("b")).await.volume.expect("b");
        // register_if_absent means the second resolve reuses the first volume.
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[tokio::test]
    async fn mount_id_for_path_skips_a_registered_archive_and_names_its_parent_mount() {
        // An `ArchiveVolume`'s root is the `.zip` itself, the longest prefix of every
        // path inside it. It is not a mount: a path inside the archive belongs to the
        // volume holding the `.zip` (what `inspect_file` and the index router route by).
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("bundle.zip");
        write_zip_magic(&zip);

        let manager = VolumeManager::new();
        manager.register(
            "ext",
            Arc::new(InMemoryVolume::new("Ext").with_root(dir.path()).with_local_fs_access()),
        );
        let inner = zip.join("docs/readme.txt");
        assert!(manager.resolve("ext", &inner).await.is_routed());

        let inner = inner.to_string_lossy();
        assert_eq!(manager.mount_id_for_path(&inner).as_deref(), Some("ext"));
        assert_eq!(
            manager.mount_id_for_path(&zip.to_string_lossy()).as_deref(),
            Some("ext")
        );
    }

    #[tokio::test]
    async fn resolve_evicts_the_least_recently_used_archive_past_the_cap() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = VolumeManager::new();
        manager.register("root", Arc::new(InMemoryVolume::new("Root").with_local_fs_access()));

        // Resolve one more archive than the cap allows.
        let mut zips = Vec::new();
        for i in 0..=ARCHIVE_LRU_CAP {
            let zip = dir.path().join(format!("z{i}.zip"));
            write_zip_magic(&zip);
            manager.resolve("root", &zip.join("inner")).await;
            zips.push(zip);
        }

        // The registry holds the parent + exactly `ARCHIVE_LRU_CAP` archives:
        // the oldest was evicted.
        assert_eq!(manager.count(), 1 + ARCHIVE_LRU_CAP);

        // The first (oldest) archive is gone from the registry...
        let oldest_id = archive_volume_id(&zips[0]);
        assert!(manager.get(&oldest_id).is_none());
        // ...but re-resolving it re-registers lazily (eviction is harmless).
        let re = manager.resolve("root", &zips[0].join("inner")).await;
        assert!(re.is_routed());
        assert!(manager.get(&oldest_id).is_some());
    }

    #[tokio::test]
    async fn resolve_lists_a_real_zip_end_to_end() {
        use std::io::Write;

        let dir = tempfile::tempdir().expect("tempdir");
        let zip_path = dir.path().join("bundle.zip");
        {
            let file = std::fs::File::create(&zip_path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("readme.txt", options).expect("start file");
            writer.write_all(b"hello").expect("write");
            writer.add_directory("docs/", options).expect("add dir");
            writer.finish().expect("finish zip");
        }

        let manager = VolumeManager::new();
        // The parent stands in for a local drive holding the `.zip`, so it must
        // report local FS access (else the archive takes its remote read path and
        // can't reach the real temp file).
        manager.register("root", Arc::new(InMemoryVolume::new("Root").with_local_fs_access()));

        // Resolving the `.zip` path lists the archive root through the ArchiveVolume.
        let resolved = manager.resolve("root", &zip_path).await;
        assert!(resolved.is_routed());
        let volume = resolved.volume.expect("archive volume");
        let entries = volume
            .list_directory(&resolved.path, None)
            .await
            .expect("list archive root");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"readme.txt"), "got: {names:?}");
        assert!(names.contains(&"docs"), "got: {names:?}");
    }

    #[tokio::test]
    async fn resolve_without_a_registered_parent_yields_no_volume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("orphan.zip");
        write_zip_magic(&zip);

        let manager = VolumeManager::new();
        // No parent registered under "root".
        let resolved = manager.resolve("root", &zip.join("inner")).await;
        assert!(!resolved.is_routed());
        assert!(resolved.volume.is_none());
    }

    #[tokio::test]
    async fn path_is_inside_archive_confirms_a_local_inner_path_but_not_the_zip_itself() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("bundle.zip");
        write_zip_magic(&zip);

        let manager = VolumeManager::new();
        manager.register("root", Arc::new(InMemoryVolume::new("Root").with_local_fs_access()));

        // A genuinely-inner path is inside the archive; the `.zip` file itself is a
        // plain file (write-routing must treat deleting/renaming it as a normal op).
        assert!(manager.path_is_inside_archive("root", &zip.join("inner.txt")).await);
        assert!(!manager.path_is_inside_archive("root", &zip).await);
        // `crosses` includes the `.zip` file itself (create-at-archive-root).
        assert!(manager.path_crosses_archive_boundary("root", &zip).await);
        assert!(
            manager
                .path_crosses_archive_boundary("root", &zip.join("inner.txt"))
                .await
        );
        // A plain path is neither.
        assert!(!manager.path_is_inside_archive("root", dir.path()).await);
        assert!(!manager.path_crosses_archive_boundary("root", dir.path()).await);
    }

    #[tokio::test]
    async fn path_is_inside_archive_confirms_a_remote_inner_path_through_the_parent() {
        // The bug this pins: the sync `archive::path_is_inside_archive` is
        // `std::fs`-only, so it silently returns false for an `smb://` / `mtp://`
        // zip and the remote archive-inner write falls through to the parent volume.
        // The async parent-aware method confirms through the parent's own I/O.
        let zip = PathBuf::from("/device/bundle.zip");
        let manager = VolumeManager::new();
        manager.register("root", Arc::new(remote_parent_with_zip(&zip).await));

        assert!(
            manager
                .path_is_inside_archive("root", &zip.join("docs/readme.txt"))
                .await,
            "a remote archive-inner path must be detected so the write reaches the edit driver"
        );
        // The remote `.zip` file itself is still a plain file.
        assert!(!manager.path_is_inside_archive("root", &zip).await);
        assert!(manager.path_crosses_archive_boundary("root", &zip).await);
    }

    #[tokio::test]
    async fn the_boundary_predicates_answer_false_for_an_unregistered_volume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("bundle.zip");
        write_zip_magic(&zip);
        let manager = VolumeManager::new();

        assert!(!manager.path_crosses_archive_boundary("nobody", &zip).await);
        assert!(!manager.path_is_inside_archive("nobody", &zip.join("inner")).await);
    }
}
