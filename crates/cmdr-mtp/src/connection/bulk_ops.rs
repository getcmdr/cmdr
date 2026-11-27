//! Bulk and recursive MTP operations (scan, recursive download/upload).

use log::debug;

use super::errors::MtpConnectionError;
use super::storage_path::{storage_path, storage_root};
use super::{MtpConnectionManager, std_path_for_host, twins};
use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, ListedFolder};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{CopyScanResult, ScanBoundary, ScanStop};

/// A scan the user stopped. ❗ The typed `Cancelled` variant, so `map_mtp_error`
/// turns it into `VolumeError::Cancelled` and every caller classifies on the
/// variant rather than on this sentence.
fn scan_cancelled(device_id: &str) -> MtpConnectionError {
    MtpConnectionError::Cancelled {
        device_id: device_id.to_string(),
        message: "Operation cancelled by user".to_string(),
    }
}

impl MtpConnectionManager {
    /// Scans an MTP path recursively to get statistics for a copy operation.
    ///
    /// # Arguments
    ///
    /// * `device_id` - The connected device ID
    /// * `storage_id` - The storage ID within the device
    /// * `path` - Virtual path on the device to scan
    ///
    /// # Returns
    ///
    /// Statistics including file count, directory count, and total bytes.
    pub async fn scan_for_copy(
        &self,
        device_id: &str,
        storage_id: u32,
        path: &str,
    ) -> Result<CopyScanResult, MtpConnectionError> {
        self.scan_for_copy_with_stop(device_id, storage_id, path, &ScanStop::none())
            .await
    }

    /// [`scan_for_copy`](Self::scan_for_copy) that a person can stop.
    ///
    /// ❗ The seam is one PTP listing wide: this walk's whole cost is one
    /// `list_directory` per directory over a USB pipe (~17 s for a 1,000-entry
    /// folder), so `stop` is consulted before each of them and before each entry.
    /// It cannot be finer — a listing in flight is inside `mtp-rs` and this layer
    /// has nothing to interrupt it with.
    pub(crate) async fn scan_for_copy_with_stop(
        &self,
        device_id: &str,
        storage_id: u32,
        path: &str,
        stop: &ScanStop,
    ) -> Result<CopyScanResult, MtpConnectionError> {
        // A silent boundary records nothing, so the path it's handed is only a name.
        let caller_path = storage_path(path).unwrap_or_else(|| storage_root().to_owned());
        self.scan_for_copy_with_boundary(
            device_id,
            storage_id,
            path,
            &caller_path,
            &ScanBoundary::silent().stopping_at(stop.clone()),
        )
        .await
    }

    /// The existing recursive walk, reporting selected entries to the batch's boundary.
    ///
    /// `caller_path` is `path` as the batch's caller spelled it: every entry
    /// reaches the boundary under it (`ScanRecorder`'s path contract), since the
    /// device path is this backend's own.
    pub(crate) async fn scan_for_copy_with_boundary(
        &self,
        device_id: &str,
        storage_id: u32,
        path: &str,
        caller_path: &AppPath,
        boundary: &ScanBoundary<'_>,
    ) -> Result<CopyScanResult, MtpConnectionError> {
        debug!(
            "MTP scan_for_copy: device={}, storage={}, path={}",
            device_id,
            storage_id,
            LogPrivate::path(path)
        );

        boundary.check().await.map_err(|_| scan_cancelled(device_id))?;

        // Try to list the path as a directory
        match self.list_directory(device_id, storage_id, path).await {
            Ok(entries) if !entries.is_empty() => {
                // Directory with contents: recurse using entries directly
                boundary
                    .dir(&std_path_for_host(caller_path))
                    .await
                    .map_err(|_| scan_cancelled(device_id))?;
                let mut result = self
                    .scan_entries(device_id, storage_id, entries, caller_path, boundary)
                    .await?;
                result.dir_count += 1;
                result.top_level_is_directory = true;
                Ok(result)
            }
            Ok(_) => {
                // Empty result: either an empty directory or a file (some MTP devices
                // return empty for files instead of an error). Check parent to disambiguate.
                boundary.check().await.map_err(|_| scan_cancelled(device_id))?;
                if let Some(result) = self.try_scan_as_file(device_id, storage_id, path).await {
                    boundary
                        .file(&std_path_for_host(caller_path), result.total_bytes)
                        .await
                        .map_err(|_| scan_cancelled(device_id))?;
                    return Ok(result);
                }
                // Empty directory
                boundary
                    .dir(&std_path_for_host(caller_path))
                    .await
                    .map_err(|_| scan_cancelled(device_id))?;
                Ok(CopyScanResult {
                    file_count: 0,
                    dir_count: 1,
                    total_bytes: 0,
                    // MTP has no hardlinks: source footprint == write footprint.
                    dedup_bytes: 0,
                    top_level_is_directory: true,
                    // This path listed the folder, never its parent, so nothing
                    // here carried the folder's own date.
                    top_level_modified_at: None,
                    top_level_created_at: None,
                })
            }
            Err(e) => {
                // list_directory failed, likely because path is a file, not a directory.
                boundary.check().await.map_err(|_| scan_cancelled(device_id))?;
                debug!(
                    "MTP scan_for_copy: list_directory failed for '{}', checking if it's a file: {}",
                    LogPrivate::path(path),
                    LogError(&e)
                );
                if let Some(result) = self.try_scan_as_file(device_id, storage_id, path).await {
                    boundary
                        .file(&std_path_for_host(caller_path), result.total_bytes)
                        .await
                        .map_err(|_| scan_cancelled(device_id))?;
                    return Ok(result);
                }
                Err(e)
            }
        }
    }

    /// Accumulates scan stats from a list of directory entries and every folder
    /// under them, depth first in listing order.
    ///
    /// Unlike `scan_for_copy`, this function takes entries that are already known
    /// from a parent listing. For file entries, it counts them directly without
    /// any USB calls. For directory entries, it lists their contents (one USB call
    /// per directory) and walks them.
    ///
    /// This ensures exactly one `list_directory` call per directory in the tree,
    /// with zero calls for files. The folders the walk is inside live on a `Vec`,
    /// ❌ never in a future per level: that nests a poll per level, and a deep
    /// enough tree overflowed the 2 MiB tokio worker a copy's scan runs on (#1541).
    async fn scan_entries(
        &self,
        device_id: &str,
        storage_id: u32,
        entries: Vec<FileEntry>,
        caller_dir: &AppPath,
        boundary: &ScanBoundary<'_>,
    ) -> Result<CopyScanResult, MtpConnectionError> {
        let mut file_count = 0usize;
        let mut dir_count = 0usize;
        let mut total_bytes = 0u64;
        // Each folder the walk is inside, in the caller's spelling, with the
        // entries of it still to walk.
        let mut inside: Vec<(AppPathBuf, std::vec::IntoIter<FileEntry>)> =
            vec![(caller_dir.to_owned(), entries.into_iter())];

        while let Some((dir, entries)) = inside.last_mut() {
            // Joined as the caller's folder joins (`ListedFolder`): typed under a canonical
            // one, so a name holding `\` stays one name on Windows. Decided once per
            // stretch of entries (a level resumes once after each child folder).
            let caller_folder = ListedFolder::new(dir);
            let mut descend = None;
            for entry in entries.by_ref() {
                // Per entry, and so BEFORE the subdirectory listing below: that
                // listing is the round trip, and asking after it is a Cancel the
                // person waits out.
                // An OS junk file the transfer leaves out is no part of it (#314, #1305).
                if boundary.leaves_out(&entry.name, entry.is_directory) {
                    continue;
                }
                // A listed row's name is one plain name (`directory_ops::entry_for_object`),
                // spelled as the app spells its row (the listed-name door, #1310).
                let Some(caller_child) = AppName::from_listed_text(&entry.name)
                    .ok()
                    .and_then(|name| caller_folder.join(&name).ok())
                else {
                    continue;
                };
                if entry.is_directory {
                    boundary
                        .dir(&std_path_for_host(&caller_child))
                        .await
                        .map_err(|_| scan_cancelled(device_id))?;
                    dir_count += 1;
                    // One list_directory call per subdirectory
                    let children = self.list_directory(device_id, storage_id, &entry.path).await?;
                    boundary.check().await.map_err(|_| scan_cancelled(device_id))?;
                    if !children.is_empty() {
                        descend = Some((caller_child, children));
                        break;
                    }
                } else {
                    boundary
                        .file(&std_path_for_host(&caller_child), entry.size.unwrap_or(0))
                        .await
                        .map_err(|_| scan_cancelled(device_id))?;
                    file_count += 1;
                    total_bytes += entry.size.unwrap_or(0);
                }
            }
            match descend {
                Some((child, children)) => inside.push((child, children.into_iter())),
                None => {
                    inside.pop();
                }
            }
        }

        debug!(
            "MTP scan_entries: {} files, {} dirs, {} bytes",
            file_count, dir_count, total_bytes
        );

        Ok(CopyScanResult {
            file_count,
            dir_count,
            total_bytes,
            // MTP has no hardlinks: source footprint == write footprint.
            dedup_bytes: total_bytes,
            // `scan_entries` is only called on known-directory input
            // (caller already listed the path). Setting `true` keeps downstream
            // callers from re-issuing a type probe.
            top_level_is_directory: true,
            // The caller listed the folder's CONTENTS; its own date was in a
            // parent listing this walk never made.
            top_level_modified_at: None,
            top_level_created_at: None,
        })
    }

    /// Helper to check if a path is a file by listing its parent directory.
    /// Returns Some(CopyScanResult) if path is a file, None otherwise.
    async fn try_scan_as_file(&self, device_id: &str, storage_id: u32, path: &str) -> Option<CopyScanResult> {
        let path_buf = storage_path(path)?;
        let parent = twins::folder_of(&path_buf)?;

        let parent_entries = self.list_directory(device_id, storage_id, parent.as_str()).await.ok()?;

        // By path, not name: a twin's row path is the one entry that's its object.
        let entry = parent_entries.iter().find(|e| e.path == path_buf.as_str())?;

        if entry.is_directory {
            // It's a directory, not a file. Let caller handle it.
            return None;
        }

        debug!(
            "MTP scan_for_copy: path '{}' is a file with size {}",
            LogPrivate::file_path(path),
            entry.size.unwrap_or(0)
        );

        Some(CopyScanResult {
            file_count: 1,
            dir_count: 0,
            total_bytes: entry.size.unwrap_or(0),
            // MTP has no hardlinks: source footprint == write footprint.
            dedup_bytes: entry.size.unwrap_or(0),
            top_level_is_directory: false,
            top_level_modified_at: entry.modified_at,
            top_level_created_at: entry.created_at,
        })
    }
}
