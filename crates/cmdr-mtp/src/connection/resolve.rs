//! Path → handle: the cache lookup every device op starts with, the heal that
//! lists a parent when the cache doesn't know a path, and the forced re-list
//! that refreshes a handle the device re-keyed.

use log::debug;
use mtp_rs::ObjectHandle;
use std::future::Future;
use std::pin::Pin;

use super::cache::Cached;
use super::errors::MtpConnectionError;
use super::storage_path::{folder_or_root, storage_path_on};
use super::{MtpConnectionManager, twins};
use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic};
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;

impl MtpConnectionManager {
    /// Re-resolves the object handles along `dir`'s path so a subsequent
    /// [`resolve_path_to_handle`](Self::resolve_path_to_handle) returns a FRESH
    /// handle for `dir`, not a stale cached one. Used by the upload path when
    /// the device rejects a cached parent handle (it re-keyed its handles since
    /// the folder was last listed).
    ///
    /// Walks `dir`'s ancestors root-first (excluding `dir` itself), forcing a
    /// real USB re-list of each (the listing cache is invalidated first so the
    /// 5 s TTL can't serve a stale listing). Listing an ancestor repopulates the
    /// path cache for its children, so by the time we list `dir`'s parent, the
    /// fresh handle for `dir` is cached. Root (`ObjectHandle::ROOT`) is a
    /// constant, so the common case (a top-level folder like `/Documents`) heals
    /// with a single re-list of `/`. Best-effort: a failed re-list is logged and
    /// the walk stops; the caller's retry then fails cleanly with a
    /// destination-correct error rather than looping.
    pub(super) async fn refresh_dir_handle(&self, device_id: &str, storage_id: u32, dir: &AppPath) {
        // Ancestors root-first, excluding `dir` itself: ["/", "/a", ...] up to
        // and including `parent(dir)`. Listing `parent(dir)` refreshes `dir`'s
        // own handle.
        // A twin's pick segment names no folder of its own, so it's stepped over:
        // listing the folder above it refreshes the twin's handle.
        let mut ancestors: Vec<AppPathBuf> = std::iter::successors(dir.parent(), |folder| folder.parent())
            .filter(|ancestor| twins::picked_folder(ancestor).is_none())
            .map(AppPath::to_owned)
            .collect();
        ancestors.reverse();
        for ancestor in ancestors {
            self.invalidate_listing_cache(device_id, storage_id, &ancestor).await;
            if let Err(e) = self.list_directory(device_id, storage_id, ancestor.as_str()).await {
                debug!(
                    "refresh_dir_handle: re-list of {} failed while healing handle for {}: {}",
                    LogPrivate::dir_path(ancestor.as_str()),
                    LogPrivate::dir_path(dir.as_str()),
                    LogError(&e)
                );
                return;
            }
        }
    }

    /// Resolves a virtual path to an MTP object handle, listing its parent when
    /// the path cache doesn't know it.
    ///
    /// The cache only holds what some listing put there, and a path reaches an op
    /// by routes that never listed its parent: a pane restored after a reconnect,
    /// a search result, a go-to-path, a pane still showing a file another op
    /// removed. So a miss lists the parent (which resolves ITS parent the same
    /// way, up to the constant root) and looks again. A path still missing after
    /// that is gone from the device: [`MtpConnectionError::ObjectNotFound`]
    /// naming the path asked about, also when a folder above it is what's
    /// missing, because the user acted on the path, not on its folder.
    ///
    /// The parent listing goes through the 5 s listing cache on purpose. Both
    /// caches are written together (`finalize_listing`) and every mutation that
    /// drops a path also invalidates its parent's listing, so a fresh listing
    /// without the name is a real answer, and a copy of many already-gone files
    /// pays one listing rather than one per file.
    ///
    /// ❌ Never call this holding the device lock: the heal re-lists through it,
    /// and the tokio `Mutex` isn't reentrant.
    ///
    /// The heal finds every folder the cache doesn't know on the way up by cache
    /// reads alone, then lists their parents top-down from one loop, so each
    /// listing resolves its folder from the cache. ❌ Never let a listing heal
    /// its own folder by nesting another heal: that nested one listing future
    /// per missing level, and a path a few hundred levels below anything listed
    /// overflowed the op's stack (#1541).
    ///
    /// A named boxed future rather than an `async fn`: listing a folder resolves
    /// that folder through here, and without the `dyn` that cycle makes an
    /// infinitely sized future whose `Send` the compiler can't prove.
    pub(super) fn resolve_path_to_handle<'a>(
        &'a self,
        device_id: &'a str,
        storage_id: u32,
        path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ObjectHandle, MtpConnectionError>> + Send + 'a>> {
        Box::pin(async move {
            let path = storage_path_on(device_id, path)?;
            if path.is_root() {
                return Ok(ObjectHandle::ROOT);
            }

            if let Some(handle) = self.cached_handle(device_id, storage_id, &path).await? {
                return Ok(handle);
            }

            let not_found = || MtpConnectionError::ObjectNotFound {
                device_id: device_id.to_string(),
                path: path.as_str().to_string(),
            };
            // What the cache doesn't know, nearest first: `path`, then each
            // folder above it up to the first the cache knows (or the root).
            let mut unknown: Vec<&AppPath> = vec![&path];
            loop {
                let folder = folder_or_root(unknown[unknown.len() - 1]);
                if folder.is_root() || self.cached_handle(device_id, storage_id, folder).await?.is_some() {
                    break;
                }
                unknown.push(folder);
            }
            // Down again, farthest first: listing a folder's parent caches the
            // folder, so the next listing finds its own from the cache. The last
            // one listed is `path`'s own folder, so its lookup is the answer.
            let mut found = None;
            for &at in unknown.iter().rev() {
                let parent = folder_or_root(at).as_str();
                debug!(
                    "MTP resolve: {} isn't cached on {device_id}:{storage_id}, listing {} to find it",
                    LogPrivate::path(at.as_str()),
                    LogPrivate::dir_path(parent)
                );
                match self.list_directory(device_id, storage_id, parent).await {
                    Ok(_) => {}
                    // A missing folder above `path` is not-found for `path`: the
                    // user acted on it, not on its folder.
                    Err(MtpConnectionError::ObjectNotFound { .. }) => return Err(not_found()),
                    Err(e) => return Err(e),
                }
                found = self.cached_handle(device_id, storage_id, at).await?;
                if found.is_none() {
                    return Err(not_found());
                }
            }
            found.ok_or_else(not_found)
        })
    }

    /// Forgets a cached `path` whose handle turned out to name another object,
    /// and its folder's cached listing, so the next resolve lists afresh.
    pub(super) async fn forget_stale_path(&self, device_id: &str, storage_id: u32, path: &AppPath) {
        use cmdr_fs::ignore_poison::RwLockIgnorePoison;

        {
            let devices = self.devices.lock().await;
            // Recover rather than skip: a poisoned cache left as it is keeps
            // serving the handle this exists to forget.
            if let Some(entry) = devices.get(device_id)
                && let Some(storage_cache) = entry.path_cache.write_ignore_poison().get_mut(&storage_id)
            {
                storage_cache.remove_path(path);
            }
        }
        if let Some(parent) = twins::folder_of(path) {
            self.invalidate_listing_cache(device_id, storage_id, parent).await;
        }
    }

    /// Forgets a handle the device reported removed, on every storage: handles
    /// are device-wide, and the event doesn't say which storage held it.
    pub(super) async fn forget_removed_handle(&self, device_id: &str, handle: ObjectHandle) {
        use cmdr_fs::ignore_poison::RwLockIgnorePoison;

        let devices = self.devices.lock().await;
        if let Some(entry) = devices.get(device_id) {
            for storage_cache in entry.path_cache.write_ignore_poison().values_mut() {
                storage_cache.forget_handle(handle);
            }
        }
    }

    /// The handle the path cache holds for an already-normalized `path`, if any.
    /// `NotConnected` when the device isn't in the registry, and `AmbiguousName`
    /// when the folder's last listing had more than one sibling answering to it:
    /// the cache knows the path but not which object it means.
    async fn cached_handle(
        &self,
        device_id: &str,
        storage_id: u32,
        path: &AppPath,
    ) -> Result<Option<ObjectHandle>, MtpConnectionError> {
        use cmdr_fs::ignore_poison::RwLockIgnorePoison;

        let devices = self.devices.lock().await;
        let entry = devices.get(device_id).ok_or_else(|| MtpConnectionError::NotConnected {
            device_id: device_id.to_string(),
        })?;
        let cache_map = entry.path_cache.read_ignore_poison();
        match cache_map
            .get(&storage_id)
            .and_then(|storage_cache| storage_cache.lookup(path))
        {
            Some(Cached::Handle(handle)) => Ok(Some(handle)),
            Some(Cached::SameNamed) => Err(MtpConnectionError::AmbiguousName {
                device_id: device_id.to_string(),
                path: path.as_str().to_string(),
            }),
            None => Ok(None),
        }
    }
}
