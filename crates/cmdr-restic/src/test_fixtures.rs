//! Tiny restic repos built with `rustic_core` itself, for this crate's tests and
//! the host's.
//!
//! Published under the `testing` feature (a dev-dependency turns it on), so the
//! app's routing and copy tests build repos the same way these do rather than
//! growing a second, drifting set. A [`FixtureRepo`] lives in memory; write it
//! onto any `Volume` with [`FixtureRepo::write_into`], or take its raw layout
//! from [`FixtureRepo::files`].
//!
//! Never in a shipped build.

use crate::ResticVolume;
use crate::hub::bridged;
use crate::path::child;
use bytes::Bytes;
use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::Volume;
use rustic_core::jiff::Timestamp;
use rustic_core::jiff::tz::TimeZone;
use rustic_core::repofile::ConfigFile;
use rustic_core::{
    BackupOptions, BytesList, ConfigOptions, Credentials, FileType, Id, IndexedIdsStatus, KeyOptions, LimitOption,
    OpenStatus, PathList, PruneOptions, ReadBackend, Repository, RepositoryBackends, RepositoryOptions, RusticResult,
    SnapshotOptions, StringList, WriteBackend,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// A restic repo in memory, opened with its password and ready to take
/// snapshots.
pub struct FixtureRepo {
    backend: Arc<MemoryBackend>,
    repo: Repository<IndexedIdsStatus>,
}

impl FixtureRepo {
    /// A fresh repo in restic's current format (v2, zstd-compressed).
    pub fn init(password: &str) -> Self {
        let backend = Arc::new(MemoryBackend::default());
        let repo = new_repository(&backend)
            .init(
                &Credentials::password(password),
                &KeyOptions::default(),
                &ConfigOptions::default(),
            )
            .expect("a fresh memory repo initializes");
        Self::indexed(backend, repo)
    }

    /// A fresh repo in restic's original format (v1, uncompressed), which repos
    /// made before restic 0.14 still use.
    ///
    /// rustic's `init` only ever writes v2 (it refuses `set_version = 1` as a
    /// downgrade), so this hands it a v1 config directly. Its repository id is
    /// all zeros, as the id type isn't public; nothing a reader does depends on it.
    pub fn init_v1(password: &str) -> Self {
        let backend = Arc::new(MemoryBackend::default());
        let config = ConfigFile {
            version: 1,
            // A known-irreducible chunker polynomial (restic's own test value),
            // since rustic's generator is private. Only a backup's chunking reads it.
            chunker_polynomial: "3da3358b4dc173".to_string(),
            ..ConfigFile::default()
        };
        let repo = new_repository(&backend)
            .init_with_config(&Credentials::password(password), &KeyOptions::default(), config)
            .expect("a fresh v1 memory repo initializes");
        Self::indexed(backend, repo)
    }

    fn indexed(backend: Arc<MemoryBackend>, repo: Repository<OpenStatus>) -> Self {
        let repo = repo.to_indexed_ids().expect("an empty repo indexes");
        Self { backend, repo }
    }

    /// Saves a snapshot that backed up nothing (an empty tree, no summary), taken
    /// at `time` on `hostname`. Enough for anything that only reads the snapshot
    /// list.
    pub fn add_snapshot(&self, time: SystemTime, hostname: &str) {
        self.add_tagged_snapshot(time, hostname, &[], &[]);
    }

    /// [`add_snapshot`](Self::add_snapshot), recording `tags` and the backed-up
    /// `paths` too.
    pub fn add_tagged_snapshot(&self, time: SystemTime, hostname: &str, tags: &[&str], paths: &[&str]) {
        let mut snapshot = SnapshotOptions::default()
            .to_snapshot()
            .expect("default snapshot options are valid");
        snapshot.time = zoned(time);
        snapshot.hostname = hostname.to_string();
        snapshot.tags = string_list(tags);
        snapshot.paths = string_list(paths);
        // No summary, as restic before 0.17 wrote them (rustic would otherwise
        // add an all-zero one).
        snapshot.summary = None;
        self.repo
            .save_snapshots(vec![snapshot])
            .expect("saving a snapshot to memory");
    }

    /// Backs up the real folder `source` into a new snapshot taken at `time`, and
    /// returns its full id. Use it when a test needs trees and file contents.
    pub fn backup(&self, source: &Path, time: SystemTime) -> String {
        let mut snapshot = SnapshotOptions::default()
            .to_snapshot()
            .expect("default snapshot options are valid");
        snapshot.time = zoned(time);
        let source = PathList::from_string(&source.to_string_lossy())
            .expect("a path is a path list")
            .sanitize()
            .expect("a real folder sanitizes");
        let snapshot = self
            .repo
            .backup(&BackupOptions::default(), &source, snapshot)
            .expect("backing up a folder into memory");
        (*snapshot.id.to_hex()).to_string()
    }

    /// Every file of the repo, as (path relative to the repo folder, bytes), in
    /// restic's on-disk layout.
    pub fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.layout()
            .into_iter()
            .map(|(names, bytes)| (names.iter().collect(), bytes))
            .collect()
    }

    /// Writes the repo onto `volume` under `root`, folders included (an empty
    /// `locks/` too, as `restic init` leaves one).
    ///
    /// Files already there are left alone, so writing again after another
    /// [`backup`](Self::backup) adds just the new ones: restic's files are named
    /// by their content, and the config never changes.
    pub async fn write_into(&self, volume: &dyn Volume, root: &Path) {
        self.write_into_at(volume, &bridged(root)).await;
    }

    /// [`write_into`](Self::write_into) for a root the caller holds as an app
    /// path.
    pub async fn write_into_at(&self, volume: &dyn Volume, root: &AppPath) {
        let mut dirs = vec![root.to_owned()];
        for folder in ["data", "index", "keys", "locks", "snapshots"] {
            dirs.push(child(root, folder));
        }
        let files = self.layout();
        for (names, _) in &files {
            if let [first, fan_out, _] = names.as_slice()
                && first == "data"
            {
                let dir = child(&child(root, first), fan_out);
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        for dir in dirs {
            if !volume.exists(&dir).await {
                volume.create_directory(&dir).await.expect("fixture folder");
            }
        }
        for (names, bytes) in files {
            let path = under(root, &names);
            if !volume.exists(&path).await {
                volume.create_file(&path, &bytes).await.expect("fixture file");
            }
        }
    }

    /// Prunes the repo the way `restic prune --repack-all` on another machine
    /// does: every blob moves to a new pack under a new index file, and the old
    /// packs and index files are deleted at once. Snapshot ids don't change.
    pub fn repack_all(&self) {
        let options = PruneOptions::default()
            .repack_all(true)
            .instant_delete(true)
            .max_repack(LimitOption::Unlimited);
        let plan = self.repo.prune_plan(&options).expect("planning a prune in memory");
        self.repo.prune(&options, plan).expect("pruning in memory");
    }

    /// [`write_into`](Self::write_into), then deletes the pack and index files
    /// on `volume` the repo no longer has: what a prune leaves behind.
    pub async fn sync_into(&self, volume: &dyn Volume, root: &Path) {
        self.sync_into_at(volume, &bridged(root)).await;
    }

    /// [`sync_into`](Self::sync_into) for a root the caller holds as an app
    /// path.
    pub async fn sync_into_at(&self, volume: &dyn Volume, root: &AppPath) {
        self.write_into_at(volume, root).await;
        let kept: Vec<AppPathBuf> = self.layout().iter().map(|(names, _)| under(root, names)).collect();
        let mut dirs = vec![child(root, "index")];
        let data = child(root, "data");
        for entry in volume.list_directory(&data, None).await.expect("data/ lists") {
            dirs.push(child(&data, &entry.name));
        }
        for dir in dirs {
            for entry in volume.list_directory(&dir, None).await.expect("a repo folder lists") {
                let path = child(&dir, &entry.name);
                if !entry.is_directory && !kept.contains(&path) {
                    volume.delete(&path).await.expect("a pruned file deletes");
                }
            }
        }
    }

    /// Every file of the repo, as (its names below the repo folder, bytes), in
    /// restic's on-disk layout.
    fn layout(&self) -> Vec<(Vec<String>, Vec<u8>)> {
        self.backend
            .files
            .read()
            .expect("fixture lock")
            .iter()
            .map(|((tpe, id), bytes)| (layout_names(tpe, id), bytes.to_vec()))
            .collect()
    }
}

/// `names` joined under `root`, one plain name at a time.
fn under(root: &AppPath, names: &[String]) -> AppPathBuf {
    names.iter().fold(root.to_owned(), |at, name| child(&at, name))
}

#[cfg(test)]
impl FixtureRepo {
    /// [`files`](Self::files), each under `root` as an app path.
    pub(crate) fn files_at(&self, root: &AppPath) -> Vec<(AppPathBuf, Vec<u8>)> {
        self.layout()
            .into_iter()
            .map(|(names, bytes)| (under(root, &names), bytes))
            .collect()
    }
}

/// `rel` (`a/b.txt`: plain names joined by `/`) under `base`, by the app path's
/// own join, so a test's path never takes the host's separator.
#[cfg(test)]
pub(crate) fn path_in(base: &AppPath, rel: &str) -> AppPathBuf {
    rel.split('/').fold(base.to_owned(), |at, name| child(&at, name))
}

/// [`path_in`] as a method, so a cell reads `view().at("latest")`.
#[cfg(test)]
pub(crate) trait At {
    /// `rel` (plain names joined by `/`) under this path.
    fn at(&self, rel: impl AsRef<str>) -> AppPathBuf;
}

#[cfg(test)]
impl At for AppPath {
    fn at(&self, rel: impl AsRef<str>) -> AppPathBuf {
        path_in(self, rel.as_ref())
    }
}

/// A canonical app path written as a test constant (`/Volumes/testshare/…`).
///
/// # Panics
///
/// On text that isn't one: a cell names a real place.
#[cfg(test)]
pub(crate) fn app(text: &str) -> AppPathBuf {
    AppPath::from_normalized(text)
        .unwrap_or_else(|why| panic!("{text:?} is no canonical app path: {why}"))
        .to_owned()
}

/// `path` as the `std::path` value `cmdr_fs::volume::conformance`'s assertions
/// still take: the one bridge this crate's tests keep, until that suite speaks
/// app paths.
#[cfg(test)]
pub(crate) fn std_path_of(path: &AppPath) -> PathBuf {
    path.as_std_legacy().into_owned()
}

/// Tries `password` on the repo `volume` views, the way the host's unlock does:
/// `true` when it unlocks (the hub keeps it), `false` when it's wrong.
///
/// The door for tests that need an unlocked view without going through the
/// credential store (`PasswordLocked::unlock` files the password there).
pub async fn unlock(volume: &ResticVolume, password: &str) -> bool {
    volume.unlock(password).await.expect("the repo answers")
}

fn new_repository(backend: &Arc<MemoryBackend>) -> Repository<()> {
    let mut options = RepositoryOptions::default();
    options.no_cache = true;
    let backend: Arc<dyn WriteBackend> = Arc::clone(backend) as Arc<dyn WriteBackend>;
    Repository::new(&options, &RepositoryBackends::new(backend, None)).expect("a memory backend makes a repository")
}

fn string_list(items: &[&str]) -> StringList {
    let mut list = StringList::default();
    for item in items {
        list.add((*item).to_string());
    }
    list
}

fn zoned(time: SystemTime) -> rustic_core::jiff::Zoned {
    Timestamp::try_from(time)
        .expect("a fixture time is in jiff's range")
        .to_zoned(TimeZone::UTC)
}

fn layout_names(dir: &str, id: &Id) -> Vec<String> {
    let hex = id.to_hex();
    match dir {
        "config" => vec!["config".to_owned()],
        "data" => vec!["data".to_owned(), hex[..2].to_owned(), hex.to_string()],
        _ => vec![dir.to_owned(), hex.to_string()],
    }
}

/// rustic's storage trait over a map: what a fixture repo is written into.
#[derive(Default)]
struct MemoryBackend {
    /// Keyed by the file type's folder name (`FileType` isn't `Ord`) and id.
    files: RwLock<BTreeMap<(&'static str, Id), Bytes>>,
}

impl ReadBackend for MemoryBackend {
    fn location(&self) -> String {
        "memory".to_string()
    }

    fn list_with_size(&self, tpe: FileType) -> RusticResult<Vec<(Id, u32)>> {
        let files = self.files.read().expect("fixture lock");
        Ok(files
            .iter()
            .filter(|((dir, _), _)| *dir == tpe.dirname())
            .map(|((_, id), bytes)| (*id, u32::try_from(bytes.len()).expect("fixture files are small")))
            .collect())
    }

    fn read_full(&self, tpe: FileType, id: &Id) -> RusticResult<Bytes> {
        let files = self.files.read().expect("fixture lock");
        Ok(files.get(&(tpe.dirname(), *id)).cloned().unwrap_or_default())
    }

    fn read_partial(&self, tpe: FileType, id: &Id, _cacheable: bool, offset: u32, length: u32) -> RusticResult<Bytes> {
        let whole = self.read_full(tpe, id)?;
        let start = offset as usize;
        Ok(whole.slice(start..start + length as usize))
    }

    fn warmup_path(&self, _tpe: FileType, _id: &Id) -> String {
        String::new()
    }
}

impl WriteBackend for MemoryBackend {
    fn write_bytes(&self, tpe: FileType, id: &Id, _cacheable: bool, content: BytesList) -> RusticResult<()> {
        let bytes: Vec<u8> = content.into_vec().iter().flat_map(|b| b.iter().copied()).collect();
        self.files
            .write()
            .expect("fixture lock")
            .insert((tpe.dirname(), *id), Bytes::from(bytes));
        Ok(())
    }

    fn remove(&self, tpe: FileType, id: &Id, _cacheable: bool) -> RusticResult<()> {
        self.files.write().expect("fixture lock").remove(&(tpe.dirname(), *id));
        Ok(())
    }
}
