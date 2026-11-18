//! The local disk on Windows: a drive (`C:/`) or a UNC share (`//server/share/`), one volume
//! each (plan D1), over `std::fs` through each path's `\\?\` form (`paths.rs`).
//!
//! The sibling of `LocalPosixVolume`, not a Windows arm inside it. What's real and
//! what's waiting is listed by name in the windows-port plan, § "Windows build state", and each
//! capability this volume doesn't answer yet keeps the trait's conservative default: a copy, a
//! symlink, xattrs, a write-access probe, and a stream are "not supported" rather than a guess.

pub(crate) mod paths;
pub(crate) mod rename;

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::listing_progress::report_while;
use cmdr_fs::volume::{
    EntryKind, ListingProgress, MountClass, MutationEvent, Presence, ScanConflict, SourceItemInfo, SpaceInfo,
    SupportedDates, Volume, VolumeError,
};
use tokio::task::spawn_blocking;

use crate::reading::{ListingTally, get_single_entry, list_directory_core_with_tally, supported_dates_of};
use crate::rename_volume_error;
use crate::space::{get_space_info_for_path, live_space_info_for_path};
use paths::native;
pub use rename::rename_local_exclusive;

/// How often a listing samples its running tally: the same cadence as `LocalPosixVolume`'s.
const PROGRESS_SAMPLE_INTERVAL: Duration = if crate::TEST_FAST_PATHS {
    Duration::from_millis(1)
} else {
    Duration::from_millis(200)
};

/// A volume backed by a local Windows filesystem (NTFS, ReFS, FAT, exFAT), rooted at a drive
/// or a UNC share. It takes and hands back app paths (`C:/Users/test_user_1`), never `\`.
pub struct LocalWindowsVolume {
    name: String,
    /// The app path of the root: `C:/`, `//server/share/`, or a folder under one.
    root: PathBuf,
    /// The mount this volume sits on, which decides whether a drive index can serve it.
    mount: MountClass,
    /// The app around this volume: the pane listings a mutation patches and the name order
    /// its listings come back in. ❗ A required argument, never a default (as for
    /// `LocalPosixVolume`).
    host: VolumeHost,
    /// Handed to every `list_directory` tally: see [`Self::with_listing_hook`].
    #[cfg(any(test, feature = "testing"))]
    listing_hook: Option<crate::reading::ListingHook>,
}

impl LocalWindowsVolume {
    /// A volume for a folder on a local disk. `root` is an app path (`C:/`, `C:/Users/x`).
    pub fn local_folder(name: impl Into<String>, root: impl Into<PathBuf>, host: VolumeHost) -> Self {
        Self::on_mount(name, root, MountClass::LocalDisk, host)
    }

    /// A volume rooted at a mount, with the [`MountClass`] its registration found.
    pub fn on_mount(name: impl Into<String>, root: impl Into<PathBuf>, mount: MountClass, host: VolumeHost) -> Self {
        Self {
            name: name.into(),
            root: root.into(),
            mount,
            host,
            #[cfg(any(test, feature = "testing"))]
            listing_hook: None,
        }
    }

    /// This volume with `hook` run on the lister's thread after each entry a
    /// `list_directory` counts, as `LocalPosixVolume::with_listing_hook`.
    #[cfg(any(test, feature = "testing"))]
    pub fn with_listing_hook(mut self, hook: crate::reading::ListingHook) -> Self {
        self.listing_hook = Some(hook);
        self
    }

    /// Resolves a caller's path to an absolute app path under this volume's root
    /// (`paths::resolve_under`, the slash-only twin of `cmdr_fs::volume::root_anchored`).
    pub(crate) fn resolve(&self, path: &Path) -> PathBuf {
        paths::resolve_under(&self.root, path)
    }
}

/// Runs `body` on the blocking pool; a `Volume` method runs on its caller's runtime.
async fn blocking<T: Send + 'static>(what: &'static str, body: impl FnOnce() -> T + Send + 'static) -> T {
    spawn_blocking(body)
        .await
        .unwrap_or_else(|_| panic!("spawn_blocking {what} closure doesn't panic and the task is uncancelable"))
}

impl Volume for LocalWindowsVolume {
    fn name(&self) -> &str {
        &self.name
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn rerooted(&self, new_root: &Path) -> Option<Arc<dyn Volume>> {
        Some(Arc::new(LocalWindowsVolume::on_mount(
            self.name.clone(),
            new_root,
            self.mount,
            self.host.clone(),
        )))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a Path,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        let is_volume_root = abs_path == self.root;
        Box::pin(async move {
            // The same tally bridge as `LocalPosixVolume::list_directory`: the callback can't
            // ride into `spawn_blocking`, so the lister publishes and this side samples.
            #[cfg(any(test, feature = "testing"))]
            let tally = Arc::new(ListingTally::with_hook(self.listing_hook.clone()));
            #[cfg(not(any(test, feature = "testing")))]
            let tally = Arc::new(ListingTally::default());
            let tally_for_listing = Arc::clone(&tally);
            let host = self.host.clone();
            let listing = spawn_blocking(move || {
                list_directory_core_with_tally(&abs_path, &tally_for_listing, is_volume_root, host.listings())
                    .map_err(|e| VolumeError::from_io_at(&e, &abs_path))
            });
            report_while(listing, PROGRESS_SAMPLE_INTERVAL, || tally.snapshot(), on_progress)
                .await
                .expect("spawn_blocking listing closure doesn't panic and the task is uncancelable")
        })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        Box::pin(blocking("metadata", move || {
            get_single_entry(&abs_path).map_err(|e| VolumeError::from_io_at(&e, &abs_path))
        }))
    }

    fn presence<'a>(&'a self, path: &'a Path) -> Pin<Box<dyn Future<Output = Presence> + Send + 'a>> {
        // Only "not found" is a definite miss: a sharing violation, an access refusal, or a
        // dead share say nothing about the path.
        let abs_path = self.resolve(path);
        Box::pin(blocking("presence", move || {
            match native(&abs_path).and_then(std::fs::symlink_metadata) {
                Ok(_) => Presence::Present,
                Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => {
                    Presence::Absent
                }
                Err(_) => Presence::Unknown,
            }
        }))
    }

    fn is_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        Box::pin(blocking("is_directory", move || {
            native(&abs_path)
                .and_then(std::fs::symlink_metadata)
                .map(|metadata| metadata.is_dir())
                .map_err(|e| VolumeError::from_io_at(&e, &abs_path))
        }))
    }

    fn entry_kind<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<EntryKind, VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        Box::pin(blocking("entry_kind", move || {
            let file_type = native(&abs_path)
                .and_then(std::fs::symlink_metadata)
                .map_err(|e| VolumeError::from_io_at(&e, &abs_path))?
                .file_type();
            Ok(if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            })
        }))
    }

    fn notify_mutation<'a>(
        &'a self,
        volume_id: &'a str,
        parent_path: &'a Path,
        mutation: MutationEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            crate::mutation::patch_listing_after_local_mutation(self.host.listings(), volume_id, parent_path, mutation);
        })
    }

    fn local_path(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }

    fn create_file<'a>(
        &'a self,
        path: &'a Path,
        content: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        let content = content.to_vec();
        Box::pin(blocking("create_file", move || -> Result<(), VolumeError> {
            use std::io::Write;
            // `create_new` is the no-clobber contract (`CREATE_NEW`): an existing name is
            // `AlreadyExists`, never a truncated file.
            let mut file = native(&abs_path)
                .and_then(|native| std::fs::OpenOptions::new().write(true).create_new(true).open(native))
                .map_err(|e| VolumeError::from_io_at(&e, &abs_path))?;
            file.write_all(&content)
                .map_err(|e| VolumeError::from_io_at(&e, &abs_path))
        }))
    }

    fn create_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        Box::pin(blocking("create_dir", move || {
            native(&abs_path)
                .and_then(std::fs::create_dir)
                .map_err(|e| VolumeError::from_io_at(&e, &abs_path))
        }))
    }

    fn delete<'a>(&'a self, path: &'a Path) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let abs_path = self.resolve(path);
        Box::pin(blocking("delete", move || {
            let native = native(&abs_path).map_err(|e| VolumeError::from_io_at(&e, &abs_path))?;
            let metadata = std::fs::symlink_metadata(&native).map_err(|e| VolumeError::from_io_at(&e, &abs_path))?;
            // A directory symlink or junction carries `FILE_ATTRIBUTE_DIRECTORY` on the link
            // itself, and only `RemoveDirectoryW` removes it: that takes the link, never its
            // target's contents, and works for a broken one too.
            use std::os::windows::fs::MetadataExt as _;
            const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
            let result = if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
                std::fs::remove_dir(&native)
            } else {
                std::fs::remove_file(&native)
            };
            result.map_err(|e| {
                log::warn!(
                    target: "local_windows",
                    "delete: {} failed: {e} (kind={:?}, os error={:?})",
                    LogPrivate::path(&abs_path),
                    e.kind(),
                    e.raw_os_error()
                );
                VolumeError::from_io_at(&e, &abs_path)
            })
        }))
    }

    /// `force` is a replacing `std::fs::rename`; everything else goes through the no-replace
    /// `rename_local_exclusive` (`rename.rs`).
    fn rename<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let from_abs = self.resolve(from);
        let to_abs = self.resolve(to);
        Box::pin(blocking("rename", move || {
            if !force && from_abs != to_abs {
                rename_local_exclusive(&from_abs, &to_abs)
            } else {
                native(&from_abs).and_then(|from| std::fs::rename(from, native(&to_abs)?))
            }
            .map_err(|e| rename_volume_error(&e, &from_abs, &to_abs))
        }))
    }

    /// The same per-name probe as `LocalPosixVolume`'s: a name is a conflict when something
    /// answers a stat there (a link followed), so a missing destination folder reads empty.
    fn scan_for_conflicts<'a>(
        &'a self,
        source_items: &'a [SourceItemInfo],
        dest_path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ScanConflict>, VolumeError>> + Send + 'a>> {
        let dest_abs = self.resolve(dest_path);
        let source_items = source_items.to_vec();
        Box::pin(blocking("scan_for_conflicts", move || {
            let mut conflicts = Vec::new();
            for item in &source_items {
                let dest_file_path = paths::child_app_path(&dest_abs, &item.name);
                let Ok(meta) = native(&dest_file_path).and_then(std::fs::metadata) else {
                    continue;
                };
                let dest_modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_secs()).ok());
                conflicts.push(ScanConflict {
                    source_path: item.name.clone(),
                    dest_path: dest_file_path.to_string_lossy().into_owned(),
                    source_size: item.size,
                    dest_size: meta.len(),
                    source_modified: item.modified,
                    dest_modified,
                    source_is_directory: item.is_directory,
                    dest_is_directory: meta.is_dir(),
                });
            }
            Ok(conflicts)
        }))
    }

    fn is_writable(&self) -> bool {
        true
    }

    /// A forced rename is `std::fs::rename`, which replaces in one filesystem step
    /// (`FILE_RENAME_FLAG_REPLACE_IF_EXISTS`, or `MoveFileExW` with
    /// `MOVEFILE_REPLACE_EXISTING`): a crash leaves the old file or the new one, never
    /// neither. A destination another program holds open, or a read-only one, is refused
    /// whole (verified on Windows 10 22H2, NTFS, `local_windows_test.rs`, 2026-11-18).
    fn supports_atomic_replace_rename(&self) -> bool {
        true
    }

    fn mount_class(&self) -> Option<MountClass> {
        Some(self.mount)
    }

    fn supported_dates<'a>(&'a self, path: &'a Path) -> Pin<Box<dyn Future<Output = SupportedDates> + Send + 'a>> {
        let abs_path = self.resolve(path);
        Box::pin(blocking("supported_dates", move || supported_dates_of(&abs_path)))
    }

    fn operations_are_local(&self) -> bool {
        true
    }

    fn get_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        let root = self.root.clone();
        Box::pin(blocking("get_space_info", move || get_space_info_for_path(&root)))
    }

    fn get_live_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        let root = self.root.clone();
        Box::pin(blocking("get_live_space_info", move || live_space_info_for_path(&root)))
    }
}
