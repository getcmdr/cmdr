//! MTP mutation operations: delete, create folder, rename, and move.
//!
//! ## A cached handle is checked before it's acted on
//!
//! Delete, rename, and move address an object by a handle the path cache
//! holds, and a device that frees a handle may hand it to another object (MTP
//! 1.1 § 3.4.1 forbids that within a session; some firmware does it anyway, and
//! Android re-keys handles across a media rescan). So each one reads the
//! object's `ObjectInfo` under the same device-lock hold as the mutation, and
//! acts only when its name and parent are the path's ([`is_the_object_at`]).
//! On a mismatch nothing is touched: the path is forgotten and resolved once
//! more from a fresh listing ([`MtpConnectionManager::on_the_object_at`]), and
//! a second mismatch is `ObjectNotFound`.

use log::{debug, warn};
use mtp_rs::{CancelToken, ObjectHandle, ObjectInfo, Storage, StorageId};
use std::future::Future;

use super::errors::MtpConnectionError;
use super::storage_path::{child, child_and_text, folder_or_root, storage_path, storage_path_on};
use super::{MtpConnectionManager, MtpObjectInfo, acquire_device_lock, twins};
use cmdr_fs::app_path::{AppPath, PathArithmetic};
use cmdr_fs::log_private::LogPrivate;

/// What one attempt at a path-addressed mutation found.
enum Attempt<T> {
    /// It ran, on the object at the path.
    Done(T),
    /// The cached handle names another object, or none: nothing was touched.
    StaleHandle,
}

/// Whether `info` describes the object at `path`, whose folder resolves to
/// `parent`: its name is the path's last component and its parent is that
/// folder. A root child may report `ObjectHandle::ALL` as its parent (the
/// Android root quirk `handle_resolver.rs` also accepts).
fn is_the_object_at(info: &ObjectInfo, path: &AppPath, parent: ObjectHandle) -> bool {
    let named_so = path.file_name().is_some_and(|name| name == info.filename.as_str());
    let filed_so = info.parent == parent || (parent == ObjectHandle::ROOT && info.parent == ObjectHandle::ALL);
    named_so && filed_so
}

/// How far a delete may reach.
///
/// Fieldless on purpose, with **no `Default` and no `From<bool>`**: which scope
/// a call wants is a decision, and a zero value would hand recursion to whoever
/// didn't make it. `Volume::delete` means the same thing on every backend
/// (`crates/cmdr-fs/src/volume/mod.rs`), so `MtpVolume` passes `SingleNode`;
/// `Tree` exists for the one IPC command that asks for a recursive delete by
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtpDeleteScope {
    /// One file, or one **empty** directory. A directory that still has
    /// children is refused with [`MtpConnectionError::DirectoryNotEmpty`] (or
    /// [`MtpConnectionError::HoldsUnlistedItems`] when no child lists a row) and
    /// nothing is deleted.
    SingleNode,
    /// The whole subtree, children first. Nothing in the app calls it: every
    /// delete goes through `MtpVolume`, which walks the tree itself so each
    /// node gets its own error attribution.
    Tree,
}

impl MtpConnectionManager {
    /// Deletes an object from the MTP device, as far as `scope` allows.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The connected device ID
    /// * `storage_id` - The storage ID within the device
    /// * `object_path` - Virtual path on the device
    /// * `scope` - Whether a non-empty directory is refused or recursed into
    pub async fn delete_object(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        scope: MtpDeleteScope,
    ) -> Result<(), MtpConnectionError> {
        self.delete_object_with_cancel(device_id, storage_id, object_path, scope, None)
            .await
    }

    /// Like [`delete_object`](Self::delete_object) but accepts a cooperative
    /// cancel token. Cancellation is checked:
    /// 1. Before each recursive child delete (between iterations of the
    ///    children loop, `Tree` only).
    /// 2. Before each per-handle `GetObjectInfo` USB roundtrip inside
    ///    `list_objects` (via `mtp-rs`'s `list_objects_with_cancel`).
    /// 3. Before issuing the final `DeleteObject` PTP request (via
    ///    `delete_with_cancel`).
    ///
    /// A flipped token bails with `MtpConnectionError::Cancelled` within ≈one
    /// USB roundtrip's latency, so `OperationIntent::Stopped` actually stops
    /// the wire activity instead of just the loop above it.
    ///
    /// **The emptiness answer costs nothing.** The directory branch already
    /// lists its children before it can do anything else, so `SingleNode`
    /// refuses on the listing it was going to fetch anyway — one fewer USB
    /// roundtrip than deleting, never one more.
    pub(crate) async fn delete_object_with_cancel(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        scope: MtpDeleteScope,
        cancel: Option<&CancelToken>,
    ) -> Result<(), MtpConnectionError> {
        // Fast bail before any I/O if cancel is already set. Cheap and avoids
        // taking the devices registry lock on a doomed call.
        if let Some(t) = cancel
            && t.is_cancelled()
        {
            return Err(MtpConnectionError::Cancelled {
                device_id: device_id.to_string(),
                message: "Delete cancelled before start".to_string(),
            });
        }

        // Foreground priority: a user delete preempts the background scan. A
        // `Tree` delete's recursive child calls nest the guard count (harmless —
        // it just keeps the scan yielded for the whole subtree delete).
        let _fg = self.foreground_guard(device_id).await;

        debug!(
            "MTP delete_object: device={}, storage={}, path={}, scope={:?}, cancel={}",
            device_id,
            storage_id,
            LogPrivate::path(object_path),
            scope,
            cancel.is_some()
        );

        self.on_the_object_at(device_id, storage_id, object_path, move || {
            self.delete_once(device_id, storage_id, object_path, scope, cancel)
        })
        .await?;

        // Remove from path cache
        let object_path_normalized = storage_path_on(device_id, object_path)?;
        {
            let devices = self.devices.lock().await;
            if let Some(entry) = devices.get(device_id)
                && let Ok(mut cache_map) = entry.path_cache.write()
                && let Some(storage_cache) = cache_map.get_mut(&storage_id)
            {
                storage_cache.remove_path(&object_path_normalized);
            }
        }

        // Invalidate the parent directory's listing cache
        if let Some(parent) = twins::folder_of(&object_path_normalized) {
            self.invalidate_listing_cache(device_id, storage_id, parent).await;
        }

        debug!("MTP delete complete: {}", LogPrivate::path(object_path));
        Ok(())
    }

    /// One attempt at [`delete_object_with_cancel`](Self::delete_object_with_cancel),
    /// on the handle the cache holds now.
    async fn delete_once(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        scope: MtpDeleteScope,
        cancel: Option<&CancelToken>,
    ) -> Result<Attempt<()>, MtpConnectionError> {
        let (object_handle, parent_handle) = self.resolve_with_parent(device_id, storage_id, object_path).await?;
        let device_arc = self.device_arc(device_id).await?;

        let device = acquire_device_lock(&device_arc, device_id, "delete_object").await?;

        // Get the storage
        let storage = device
            .storage(StorageId(u64::from(storage_id)))
            .await
            .map_err(|e| self.map_device_error(e, device_id))?;

        // The object info says whether it's a directory, and that the handle is
        // still the object at this path.
        let Some(object_info) = self
            .object_at(&storage, device_id, object_handle, object_path, parent_handle)
            .await?
        else {
            return Ok(Attempt::StaleHandle);
        };

        let is_dir = object_info.is_folder();

        if is_dir {
            // The children have to be listed either way: MTP can't delete a
            // folder that still holds anything, so `Tree` needs the list to
            // recurse and `SingleNode` needs it to know whether to refuse.
            // Threading the cancel token into `list_objects_with_cancel` is what
            // makes the 950-entry `/DCIM/Camera` listing bail at the next
            // per-handle USB boundary instead of running all 950 GetObjectInfo
            // roundtrips to completion.
            let mut children = storage
                .list_objects_with_cancel(Some(object_handle), cancel)
                .await
                .map_err(|e| self.map_device_error(e, device_id))?;
            for child_info in &mut children {
                self.as_reported(device_id, child_info);
            }

            drop(storage);
            drop(device);

            // A child no path can address (`a/b`, `..`) lists no row
            // (`directory_ops::entry_for_object`), so a folder holding only such
            // children looks empty: its refusal says so, ❌ never a bare "still has
            // contents" that contradicts the pane.
            let parent_path = storage_path_on(device_id, object_path)?;
            let unlisted = || MtpConnectionError::HoldsUnlistedItems {
                device_id: device_id.to_string(),
                path: parent_path.as_str().to_string(),
            };
            if scope == MtpDeleteScope::SingleNode
                && !children.is_empty()
                && children
                    .iter()
                    .all(|child_info| child(&parent_path, &child_info.filename).is_none())
            {
                debug!(
                    "MTP delete refused (folder holds only unlisted objects): {}",
                    LogPrivate::dir_path(object_path)
                );
                return Err(unlisted());
            }

            if scope == MtpDeleteScope::SingleNode && !children.is_empty() {
                // Refuse, and delete nothing — including the path-cache
                // bookkeeping below, which describes an object that's still
                // there. `Volume::delete`'s contract is "one file or one EMPTY
                // directory", and callers lean on the refusal to protect data:
                // the same-volume move's source cleanup keeps a skipped child's
                // only copy purely by letting this fail.
                debug!(
                    "MTP delete refused (folder still has {} entries): {}",
                    children.len(),
                    LogPrivate::dir_path(object_path)
                );
                return Err(MtpConnectionError::DirectoryNotEmpty {
                    device_id: device_id.to_string(),
                    path: object_path.to_string(),
                });
            }

            // Recursively delete children. The token is checked between
            // iterations and inside each child's own list/delete USB call.
            let mut holds_unlisted = false;
            for child_info in children {
                if let Some(t) = cancel
                    && t.is_cancelled()
                {
                    return Err(MtpConnectionError::Cancelled {
                        device_id: device_id.to_string(),
                        message: "Delete cancelled between children".to_string(),
                    });
                }

                // A child no path can address (`a/b`, `..`) can't be deleted by path:
                // ❌ never a joined path, which would name another object. The rest
                // go, and the folder, now looking empty, refuses as holding it.
                let (Some(child_path), child_path_str) = child_and_text(&parent_path, &child_info.filename) else {
                    holds_unlisted = true;
                    continue;
                };

                // Cache the child handle for the recursive call
                {
                    let devices = self.devices.lock().await;
                    if let Some(entry) = devices.get(device_id)
                        && let Ok(mut cache_map) = entry.path_cache.write()
                    {
                        let storage_cache = cache_map.entry(storage_id).or_default();
                        storage_cache.insert(child_path.clone(), child_info.handle);
                    }
                }

                // Use Box::pin for recursive async call
                Box::pin(self.delete_object_with_cancel(device_id, storage_id, &child_path_str, scope, cancel)).await?;
            }
            if holds_unlisted {
                return Err(unlisted());
            }

            // Re-acquire device and storage lock to delete the now-empty folder
            let device = acquire_device_lock(&device_arc, device_id, "delete_object (empty folder)").await?;
            let storage = device
                .storage(StorageId(u64::from(storage_id)))
                .await
                .map_err(|e| self.map_device_error(e, device_id))?;

            storage
                .delete_with_cancel(object_handle, cancel)
                .await
                .map_err(|e| self.map_device_error(e, device_id))?;
        } else {
            // For files, just delete directly. The cancel check inside
            // delete_with_cancel bails before the PTP `DeleteObject` request
            // is issued, so a cancelled token never touches the wire here.
            storage
                .delete_with_cancel(object_handle, cancel)
                .await
                .map_err(|e| self.map_device_error(e, device_id))?;
        }
        Ok(Attempt::Done(()))
    }

    /// Runs `attempt`, and once more from a fresh listing when the cached handle
    /// turned out to name another object (module docs § "A cached handle is
    /// checked before it's acted on"). A second stale answer is
    /// [`MtpConnectionError::ObjectNotFound`]: whatever the path names, the device
    /// won't say it's there.
    async fn on_the_object_at<T, Fut>(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        attempt: impl Fn() -> Fut,
    ) -> Result<T, MtpConnectionError>
    where
        Fut: Future<Output = Result<Attempt<T>, MtpConnectionError>>,
    {
        if let Attempt::Done(done) = attempt().await? {
            return Ok(done);
        }
        let path = storage_path_on(device_id, object_path)?;
        warn!(
            "MTP on {device_id}:{storage_id}: the cached handle for {} points at another object; resolving it afresh",
            LogPrivate::path(path.as_str())
        );
        self.forget_stale_path(device_id, storage_id, &path).await;
        match attempt().await? {
            Attempt::Done(done) => Ok(done),
            Attempt::StaleHandle => Err(MtpConnectionError::ObjectNotFound {
                device_id: device_id.to_string(),
                path: path.as_str().to_string(),
            }),
        }
    }

    /// The handles of `object_path` and of the folder holding it.
    async fn resolve_with_parent(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
    ) -> Result<(ObjectHandle, ObjectHandle), MtpConnectionError> {
        let object = self.resolve_path_to_handle(device_id, storage_id, object_path).await?;
        let path = storage_path_on(device_id, object_path)?;
        let parent = folder_or_root(&path).as_str().to_string();
        let parent = self.resolve_path_to_handle(device_id, storage_id, &parent).await?;
        Ok((object, parent))
    }

    /// The `ObjectInfo` of `handle` when it's still the object at `object_path`
    /// in the folder `parent`, else `None`: a handle the device no longer knows
    /// reads the same as one it gave to another object.
    async fn object_at(
        &self,
        storage: &Storage,
        device_id: &str,
        handle: ObjectHandle,
        object_path: &str,
        parent: ObjectHandle,
    ) -> Result<Option<ObjectInfo>, MtpConnectionError> {
        let mut info = match storage.get_object_info(handle).await {
            Ok(info) => info,
            Err(e) if e.is_stale_handle() => return Ok(None),
            Err(e) => return Err(self.map_device_error(e, device_id)),
        };
        self.as_reported(device_id, &mut info);
        Ok(storage_path(object_path)
            .is_some_and(|path| is_the_object_at(&info, &path, parent))
            .then_some(info))
    }

    /// Creates a new folder on the MTP device.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The connected device ID
    /// * `storage_id` - The storage ID within the device
    /// * `parent_path` - Parent folder path (for example, "/DCIM")
    /// * `folder_name` - Name of the new folder
    pub async fn create_folder(
        &self,
        device_id: &str,
        storage_id: u32,
        parent_path: &str,
        folder_name: &str,
    ) -> Result<MtpObjectInfo, MtpConnectionError> {
        // Foreground priority: a user create preempts the background scan.
        let _fg = self.foreground_guard(device_id).await;

        debug!(
            "MTP create_folder: device={}, storage={}, parent={:?}, new_name={:?}",
            device_id,
            storage_id,
            LogPrivate::dir_path(parent_path),
            LogPrivate::dir_name(folder_name)
        );

        let parent_handle = self.resolve_path_to_handle(device_id, storage_id, parent_path).await?;
        let device_arc = self.device_arc(device_id).await?;

        let device = acquire_device_lock(&device_arc, device_id, "create_folder").await?;

        // Get the storage
        let storage = device
            .storage(StorageId(u64::from(storage_id)))
            .await
            .map_err(|e| self.map_device_error(e, device_id))?;

        // Create the folder
        let parent_opt = if parent_handle == ObjectHandle::ROOT {
            None
        } else {
            Some(parent_handle)
        };

        let new_handle = storage
            .create_folder(parent_opt, folder_name)
            .await
            .map_err(|e| self.map_device_error(e, device_id))?;

        // Release device lock
        drop(storage);
        drop(device);

        // Build the new folder path
        let parent_path_normalized = storage_path_on(device_id, parent_path)?;
        let (new_path, new_path_str) = child_and_text(&parent_path_normalized, folder_name);

        // Update path cache
        {
            let devices = self.devices.lock().await;
            if let Some(entry) = devices.get(device_id)
                && let Ok(mut cache_map) = entry.path_cache.write()
            {
                let storage_cache = cache_map.entry(storage_id).or_default();
                if let Some(new_path) = new_path {
                    storage_cache.insert(new_path, new_handle);
                }
            }
        }

        // Invalidate the parent directory's listing cache
        self.invalidate_listing_cache(device_id, storage_id, &parent_path_normalized)
            .await;

        debug!("MTP folder created: {}", LogPrivate::dir_path(&new_path_str));

        Ok(MtpObjectInfo {
            handle: new_handle.0 as u32,
            name: folder_name.to_string(),
            path: new_path_str,
            is_directory: true,
            size: None,
        })
    }

    /// Renames an object on the MTP device.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The connected device ID
    /// * `storage_id` - The storage ID within the device
    /// * `object_path` - Current path of the object
    /// * `new_name` - New name for the object
    pub async fn rename_object(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        new_name: &str,
    ) -> Result<MtpObjectInfo, MtpConnectionError> {
        // Foreground priority: a user rename preempts the background scan.
        let _fg = self.foreground_guard(device_id).await;

        debug!(
            "MTP rename_object: device={}, storage={}, path={}, new_name={}",
            device_id,
            storage_id,
            LogPrivate::path(object_path),
            LogPrivate::name(new_name)
        );

        let (object_handle, object_info) = self
            .on_the_object_at(device_id, storage_id, object_path, move || {
                self.rename_once(device_id, storage_id, object_path, new_name)
            })
            .await?;
        let is_dir = object_info.is_folder();
        let old_size = object_info.size;

        // Update path cache
        let old_path = storage_path_on(device_id, object_path)?;
        let parent = folder_or_root(&old_path);
        let (new_path, new_path_str) = child_and_text(parent, new_name);

        {
            let devices = self.devices.lock().await;
            if let Some(entry) = devices.get(device_id)
                && let Ok(mut cache_map) = entry.path_cache.write()
                && let Some(storage_cache) = cache_map.get_mut(&storage_id)
            {
                storage_cache.remove_path(&old_path);
                if let Some(new_path) = new_path {
                    storage_cache.insert(new_path, object_handle);
                }
            }
        }

        // Invalidate the parent directory's listing cache (rename affects the parent listing)
        self.invalidate_listing_cache(device_id, storage_id, parent).await;
        if is_dir {
            self.invalidate_listing_subtree(device_id, storage_id, &old_path).await;
        }

        debug!(
            "MTP rename complete: {} -> {}",
            LogPrivate::path(object_path),
            LogPrivate::path(&new_path_str)
        );

        Ok(MtpObjectInfo {
            handle: object_handle.0 as u32,
            name: new_name.to_string(),
            path: new_path_str,
            is_directory: is_dir,
            size: if is_dir { None } else { Some(old_size) },
        })
    }

    /// One attempt at [`rename_object`](Self::rename_object), on the handle the
    /// cache holds now. Answers the handle it renamed and the object's info.
    async fn rename_once(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        new_name: &str,
    ) -> Result<Attempt<(ObjectHandle, ObjectInfo)>, MtpConnectionError> {
        let (object_handle, parent_handle) = self.resolve_with_parent(device_id, storage_id, object_path).await?;
        let device_arc = self.device_arc(device_id).await?;

        let device = acquire_device_lock(&device_arc, device_id, "rename_object").await?;

        // Get the storage
        let storage = device
            .storage(StorageId(u64::from(storage_id)))
            .await
            .map_err(|e| self.map_device_error(e, device_id))?;

        let Some(object_info) = self
            .object_at(&storage, device_id, object_handle, object_path, parent_handle)
            .await?
        else {
            return Ok(Attempt::StaleHandle);
        };

        // Set the new filename using storage.rename()
        storage
            .rename(object_handle, new_name)
            .await
            .map_err(|e| self.map_device_error(e, device_id))?;
        Ok(Attempt::Done((object_handle, object_info)))
    }

    /// Moves an object to a new parent folder on the MTP device.
    ///
    /// Falls back to copy+delete if the device doesn't support MoveObject.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The connected device ID
    /// * `storage_id` - The storage ID within the device
    /// * `object_path` - Current path of the object
    /// * `new_parent_path` - New parent folder path
    pub async fn move_object(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        new_parent_path: &str,
    ) -> Result<MtpObjectInfo, MtpConnectionError> {
        // Foreground priority: a user move preempts the background scan.
        let _fg = self.foreground_guard(device_id).await;

        debug!(
            "MTP move_object: device={}, storage={}, path={}, new_parent={}",
            device_id,
            storage_id,
            LogPrivate::path(object_path),
            LogPrivate::dir_path(new_parent_path)
        );

        let (object_handle, object_info, move_result) = self
            .on_the_object_at(device_id, storage_id, object_path, move || {
                self.move_once(device_id, storage_id, object_path, new_parent_path)
            })
            .await?;

        let is_dir = object_info.is_folder();
        let object_size = object_info.size;
        let object_name = object_info.filename.clone();

        match move_result {
            Ok(()) => {
                // Move succeeded
                let old_path = storage_path_on(device_id, object_path)?;
                let new_parent = storage_path_on(device_id, new_parent_path)?;
                let (new_path, new_path_str) = child_and_text(&new_parent, &object_name);

                // Update path cache
                {
                    let devices = self.devices.lock().await;
                    if let Some(entry) = devices.get(device_id)
                        && let Ok(mut cache_map) = entry.path_cache.write()
                        && let Some(storage_cache) = cache_map.get_mut(&storage_id)
                    {
                        storage_cache.remove_path(&old_path);
                        if let Some(new_path) = new_path {
                            storage_cache.insert(new_path, object_handle);
                        }
                    }
                }

                // Invalidate listing cache for both old and new parent directories
                let old_parent = folder_or_root(&old_path);
                self.invalidate_listing_cache(device_id, storage_id, old_parent).await;
                if is_dir {
                    self.invalidate_listing_subtree(device_id, storage_id, &old_path).await;
                }
                self.invalidate_listing_cache(device_id, storage_id, &new_parent).await;

                debug!(
                    "MTP move complete: {} -> {}",
                    LogPrivate::path(object_path),
                    LogPrivate::path(&new_path_str)
                );

                Ok(MtpObjectInfo {
                    handle: object_handle.0 as u32,
                    name: object_name,
                    path: new_path_str,
                    is_directory: is_dir,
                    size: if is_dir { None } else { Some(object_size) },
                })
            }
            Err(e) => {
                // Move operation returned an error - might not be supported
                warn!(
                    "MTP MoveObject failed for {}: {:?}. Device may not support this operation.",
                    LogPrivate::path(object_path),
                    e
                );
                Err(MtpConnectionError::Other {
                    device_id: device_id.to_string(),
                    message: format!("Move operation not supported by device: {}", e),
                })
            }
        }
    }

    /// One attempt at [`move_object`](Self::move_object), on the handle the
    /// cache holds now. Answers the handle it asked to move, the object's info,
    /// and what `MoveObject` said, which the caller words.
    async fn move_once(
        &self,
        device_id: &str,
        storage_id: u32,
        object_path: &str,
        new_parent_path: &str,
    ) -> Result<Attempt<(ObjectHandle, ObjectInfo, Result<(), mtp_rs::Error>)>, MtpConnectionError> {
        let (object_handle, parent_handle) = self.resolve_with_parent(device_id, storage_id, object_path).await?;
        let new_parent_handle = self
            .resolve_path_to_handle(device_id, storage_id, new_parent_path)
            .await?;
        let device_arc = self.device_arc(device_id).await?;

        let device = acquire_device_lock(&device_arc, device_id, "move_object").await?;

        // Get the storage
        let storage = device
            .storage(StorageId(u64::from(storage_id)))
            .await
            .map_err(|e| self.map_device_error(e, device_id))?;

        let Some(object_info) = self
            .object_at(&storage, device_id, object_handle, object_path, parent_handle)
            .await?
        else {
            return Ok(Attempt::StaleHandle);
        };

        let move_result = storage.move_object(object_handle, new_parent_handle, None).await;
        Ok(Attempt::Done((object_handle, object_info, move_result)))
    }
}
