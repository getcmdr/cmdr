//! The local disk on Windows: a drive (`C:/`) or a UNC share (`//server/share/`), one volume
//! each (plan D1), over `std::fs` through each path's `\\?\` form (`paths.rs`).
//!
//! The sibling of `LocalPosixVolume`, not a Windows arm inside it. What's real and
//! what's waiting is listed by name in the windows-port plan, § "Windows build state", and each
//! capability this volume doesn't answer yet keeps the trait's conservative default: a copy, a
//! symlink, xattrs, a write-access probe, and a stream are "not supported" rather than a guess.

pub(crate) mod paths;
pub mod recycle_bin;
pub(crate) mod rename;

use std::borrow::Cow;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, NotAnAppPath, PathArithmetic, is_escape};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::Unblock;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{
    EntryKind, ListingProgress, MountClass, MutationEvent, Presence, ScanConflict, SourceItemInfo, SpaceInfo,
    SupportedDates, Volume, VolumeError,
};
use tokio::task::spawn_blocking;

use super::anchor::anchor_under;
use crate::reading::{ListingDir, ListingTally, Named, Row, list_reporting_progress, single_entry, supported_dates_of};
use crate::space::{get_space_info_for_path, live_space_info_for_path};
use paths::{app_of, native_of, not_an_app_path};
use rename::rename_native_exclusive;
pub use rename::{rename_local_exclusive, rename_local_exclusive_at};

/// A volume backed by a local Windows filesystem (NTFS, ReFS, FAT, exFAT), rooted at a drive
/// or a UNC share. It takes and hands back app paths (`C:/Users/test_user_1`), never `\`.
pub struct LocalWindowsVolume {
    name: String,
    /// The app path of the root: `C:/`, `//server/share/`, or a folder under one.
    root: AppPathBuf,
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

/// A path this volume answers for: its app path, and the verbatim path the OS opens.
struct Located {
    app: AppPathBuf,
    os: PathBuf,
}

impl Located {
    fn error(&self, err: &io::Error) -> VolumeError {
        Named::new(self.app.as_str().to_owned()).error(err)
    }
}

impl LocalWindowsVolume {
    /// A volume for a folder on a local disk. `root` is a native path, spelled either way
    /// (`C:\x` or the app path's `C:/x`), with its 8.3 short names expanded (a listing never
    /// shows one, so a row would never sit under a short root).
    pub fn local_folder(name: impl Into<String>, root: impl Into<PathBuf>, host: VolumeHost) -> Self {
        Self::on_mount(name, root, MountClass::LocalDisk, host)
    }

    /// A volume rooted at a mount, with the [`MountClass`] its registration found. A root
    /// holding an 8.3 short name is spelled long, since a listing never shows one; only such
    /// a root reads the disk here (`super::short_names`).
    pub fn on_mount(name: impl Into<String>, root: impl Into<PathBuf>, mount: MountClass, host: VolumeHost) -> Self {
        let root = super::short_names::long_root(root.into(), cmdr_fs::app_path::expand_short_names);
        Self::on_mount_at(name, root_app(&root), mount, host)
    }

    /// [`Self::local_folder`] at an app path.
    pub fn local_folder_at(name: impl Into<String>, root: AppPathBuf, host: VolumeHost) -> Self {
        Self::on_mount_at(name, root, MountClass::LocalDisk, host)
    }

    /// [`Self::on_mount`] at an app path. A root no OS call can open (a POSIX `/`) makes every
    /// call refuse typed, ❌ never a guess at a drive.
    pub fn on_mount_at(name: impl Into<String>, root: AppPathBuf, mount: MountClass, host: VolumeHost) -> Self {
        Self {
            name: name.into(),
            root,
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

    /// A caller's path as this volume's absolute app path and the OS's path for it
    /// (`super::anchor::anchor_under`).
    ///
    /// # Errors
    ///
    /// The typed refusal as an `io::Error` (`paths.rs`): a `..`, a path no OS call opens, a
    /// name holding `:`, and a name that isn't Unicode text (its escape goes live in 10f).
    fn locate(&self, path: &AppPath) -> io::Result<Located> {
        let app = anchor_under(&self.root, path).map_err(not_an_app_path)?;
        if app.as_str().chars().any(is_escape) {
            return Err(not_an_app_path(NotAnAppPath::NotUnicode));
        }
        let os = native_of(&app)?;
        Ok(Located { app, os })
    }

    /// [`Self::locate`], with a refusal named after the path as the caller spelled it.
    fn located(&self, path: &AppPath) -> Result<Located, VolumeError> {
        self.locate(path)
            .map_err(|e| Named::new(path.as_str().to_owned()).error(&e))
    }
}

/// The app path of an old-form constructor's native root; a root that isn't one keeps its
/// text through the bridge, so every call refuses it typed, as before.
fn root_app(root: &Path) -> AppPathBuf {
    app_of(root).unwrap_or_else(|_| AppPath::legacy_unchecked(root).into_owned())
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

    fn root(&self) -> &AppPath {
        &self.root
    }

    fn rerooted(&self, new_root: &AppPath) -> Option<Arc<dyn Volume>> {
        let new_root = new_root
            .to_canonical()
            .map_or_else(|_| new_root.to_owned(), Cow::into_owned);
        Some(Arc::new(LocalWindowsVolume::on_mount_at(
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
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        let at = match self.located(path) {
            Ok(at) => at,
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        let is_volume_root = at.app == self.root;
        #[cfg(any(test, feature = "testing"))]
        let tally = Arc::new(ListingTally::with_hook(self.listing_hook.clone()));
        #[cfg(not(any(test, feature = "testing")))]
        let tally = Arc::new(ListingTally::default());
        let named = Named::new(at.app.as_str().to_owned());
        let dir = ListingDir::new(Cow::Owned(at.os), Some(Cow::Owned(at.app)));
        Box::pin(list_reporting_progress(
            dir,
            named,
            is_volume_root,
            self.host.clone(),
            tally,
            on_progress,
        ))
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        let at = self.located(path);
        Box::pin(blocking("metadata", move || {
            let at = at?;
            single_entry(&at.os, Row::of(&at.os, Some(&at.app))).map_err(|e| at.error(&e))
        }))
    }

    fn presence<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = Presence> + Send + 'a>> {
        // Only "not found" is a definite miss: a sharing violation, an access refusal, a dead
        // share, or a path the OS can't be handed say nothing about the path.
        let at = self.locate(path);
        Box::pin(blocking("presence", move || {
            match at.and_then(|at| std::fs::symlink_metadata(at.os)) {
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
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        let at = self.located(path);
        Box::pin(blocking("is_directory", move || {
            let at = at?;
            std::fs::symlink_metadata(&at.os)
                .map(|metadata| metadata.is_dir())
                .map_err(|e| at.error(&e))
        }))
    }

    fn entry_kind<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<EntryKind, VolumeError>> + Send + 'a>> {
        let at = self.located(path);
        Box::pin(blocking("entry_kind", move || {
            let at = at?;
            let file_type = std::fs::symlink_metadata(&at.os).map_err(|e| at.error(&e))?.file_type();
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
        parent_path: &'a AppPath,
        mutation: MutationEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            match self.locate(parent_path) {
                Ok(at) => {
                    // The listing host hears of the folder by its app path; it still takes
                    // `std::path` for one, until the app's listing host goes typed (10c).
                    let host_path = at.app.as_std_legacy().into_owned();
                    crate::mutation::patch_after_local_mutation(
                        self.host.listings(),
                        volume_id,
                        &host_path,
                        &ListingDir::new(Cow::Owned(at.os), Some(Cow::Owned(at.app))),
                        mutation,
                    );
                }
                Err(e) => log::warn!(
                    "notify_mutation: no local path for {}: {e}",
                    LogPrivate::path(parent_path.as_str())
                ),
            }
        })
    }

    fn local_path(&self) -> Option<AppPathBuf> {
        Some(self.root.clone())
    }

    fn create_file<'a>(
        &'a self,
        path: &'a AppPath,
        content: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let at = self.located(path);
        let content = content.to_vec();
        Box::pin(blocking("create_file", move || -> Result<(), VolumeError> {
            use std::io::Write;
            let at = at?;
            // `create_new` is the no-clobber contract (`CREATE_NEW`): an existing name is
            // `AlreadyExists`, never a truncated file.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&at.os)
                .map_err(|e| at.error(&e))?;
            file.write_all(&content).map_err(|e| at.error(&e))
        }))
    }

    fn create_directory<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let at = self.located(path);
        Box::pin(blocking("create_dir", move || {
            let at = at?;
            std::fs::create_dir(&at.os).map_err(|e| at.error(&e))
        }))
    }

    /// A FILE's read-only attribute (Windows' Locked) as `clear_blockers` clears it, never
    /// through a link. Only a reversal or a consented move asks (`Volume::clear_blockers`).
    fn clear_blockers<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = Unblock> + Send + 'a>> {
        let at = self.locate(path);
        Box::pin(blocking("clear_blockers", move || match at {
            Ok(at) => cmdr_fs::metadata::clear_blockers(&at.os),
            Err(_) => Unblock::NothingToClear,
        }))
    }

    /// ❗ A read-only FILE refuses as `PermissionDenied` (`ERROR_ACCESS_DENIED`), the way
    /// macOS's `unlink` refuses a Locked one: `std::fs::remove_file` would delete it anyway
    /// (it clears the attribute on the way, verified on Windows 10 22H2 with Rust 1.100,
    /// 2026-11-21), so a move's source sweep would take a read-only original nobody agreed
    /// to. What the person agreed to unlock is cleared before this call; what Cmdr made goes
    /// through `clear_blockers` and a retry. A folder's read-only bit is Explorer's
    /// customization marker, never a lock.
    fn delete<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let at = self.located(path);
        Box::pin(blocking("delete", move || {
            let at = at?;
            let metadata = std::fs::symlink_metadata(&at.os).map_err(|e| at.error(&e))?;
            if metadata.is_file() && metadata.permissions().readonly() {
                const ERROR_ACCESS_DENIED: i32 = 5;
                return Err(at.error(&io::Error::from_raw_os_error(ERROR_ACCESS_DENIED)));
            }
            // A directory symlink or junction carries `FILE_ATTRIBUTE_DIRECTORY` on the link
            // itself, and only `RemoveDirectoryW` removes it: that takes the link, never its
            // target's contents, and works for a broken one too.
            use std::os::windows::fs::MetadataExt as _;
            const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
            let result = if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
                std::fs::remove_dir(&at.os)
            } else {
                std::fs::remove_file(&at.os)
            };
            result.map_err(|e| {
                log::warn!(
                    target: "local_windows",
                    "delete: {} failed: {e} (kind={:?}, os error={:?})",
                    LogPrivate::path(at.app.as_str()),
                    e.kind(),
                    e.raw_os_error()
                );
                at.error(&e)
            })
        }))
    }

    /// `force` is a replacing `std::fs::rename`; everything else goes through the no-replace
    /// `rename_local_exclusive` (`rename.rs`).
    fn rename<'a>(
        &'a self,
        from: &'a AppPath,
        to: &'a AppPath,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        let ends = self.located(from).and_then(|from| Ok((from, self.located(to)?)));
        Box::pin(blocking("rename", move || {
            let (from, to) = ends?;
            if !force && from.app != to.app {
                rename_native_exclusive(&from.os, &to.os)
            } else {
                std::fs::rename(&from.os, &to.os)
            }
            .map_err(|e| match e.kind() {
                // `crate::rename_volume_error`'s rule: the destination is what's taken.
                io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty => to.error(&e),
                _ => from.error(&e),
            })
        }))
    }

    /// The same per-name probe as `LocalPosixVolume`'s: a name is a conflict when something
    /// answers a stat there (a link followed), so a missing destination folder reads empty.
    fn scan_for_conflicts<'a>(
        &'a self,
        source_items: &'a [SourceItemInfo],
        dest_path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ScanConflict>, VolumeError>> + Send + 'a>> {
        let dest = self.located(dest_path);
        let source_items = source_items.to_vec();
        Box::pin(blocking("scan_for_conflicts", move || {
            let dest = dest?;
            let mut conflicts = Vec::new();
            for item in &source_items {
                // An item whose name names no child of the destination can't collide: it's
                // refused typed, alone, and logged (a conflict carries no per-item error),
                // and the batch's other items are still checked, as on POSIX.
                let dest_file = match AppName::from_app_spelling(&item.name) {
                    Ok(name) => dest
                        .app
                        .join(name)
                        .map_err(|why| VolumeError::InvalidName(why.to_string().into())),
                    Err(refused) => Err(VolumeError::from(refused)),
                };
                let dest_file = match dest_file {
                    Ok(dest_file) => dest_file,
                    Err(refused) => {
                        log::warn!("scan_for_conflicts: skipping a source name: {}", LogError(&refused));
                        continue;
                    }
                };
                let Ok(meta) = native_of(&dest_file).and_then(std::fs::metadata) else {
                    continue;
                };
                let dest_modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_secs()).ok());
                conflicts.push(ScanConflict {
                    source_path: item.name.clone(),
                    dest_path: dest_file.into_string(),
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

    /// Whether the drive under the root keeps a Recycle Bin, the ONE answer the volume list and
    /// the Recycle Bin arm's refusal read too (`recycle_bin.rs`): a removable or `subst`
    /// drive, or a bin turned off, says no though its mount class is `LocalDisk`. A share's
    /// class says no first, with no call to the drive.
    fn has_trash(&self) -> bool {
        self.mount.has_trash() && recycle_bin::has_recycle_bin_at(&self.root)
    }

    fn supported_dates<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = SupportedDates> + Send + 'a>> {
        let at = self.locate(path);
        Box::pin(blocking("supported_dates", move || match at {
            Ok(at) => supported_dates_of(&at.os),
            Err(_) => SupportedDates::default(),
        }))
    }

    fn operations_are_local(&self) -> bool {
        true
    }

    /// The root's space; a refusal or an OS error names the root's app path (`space.rs`'s
    /// Windows arm crosses the `\\?\` path back up).
    fn get_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        let root = self.located(&self.root);
        Box::pin(blocking("get_space_info", move || get_space_info_for_path(&root?.os)))
    }

    fn get_live_space_info<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<SpaceInfo, VolumeError>> + Send + 'a>> {
        let root = self.located(&self.root);
        Box::pin(blocking("get_live_space_info", move || {
            live_space_info_for_path(&root?.os)
        }))
    }
}
