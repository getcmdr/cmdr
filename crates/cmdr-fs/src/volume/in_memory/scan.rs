//! What [`InMemoryVolume`] counts and records for a copy's scan: the subtree
//! below a path, read from the store, as `scan_for_copy` and the scan boundary's
//! recorder see it.

use super::{InMemoryEntry, InMemoryVolume, join_tail, std_of};
use crate::app_path::{AppPath, PathArithmetic};
use crate::ignore_poison::RwLockIgnorePoison;
use crate::volume::VolumeError;

impl InMemoryVolume {
    /// Hands `recorder` the entry at `path` and every one below it, each named
    /// under `path` as the caller spelled it. Counted the way `scan_for_copy`
    /// counts: a directory flag makes a folder, anything else a file.
    /// `scan_for_copy`'s count of `path`, leaving out what `os_junk` says to
    /// below it (never `path` itself: a selected file goes).
    pub(super) fn scan_subtree(
        &self,
        path: &AppPath,
        os_junk: crate::os_junk_files::OsJunkFiles,
    ) -> Result<crate::volume::CopyScanResult, VolumeError> {
        let normalized = self.normalize(path);
        let entries = self.entries.read().map_err(|_| VolumeError::IoError {
            message: "Lock poisoned".into(),
            raw_os_error: None,
        })?;

        // Check if the path is a file
        if let Some(entry) = entries.get(&*normalized)
            && !entry.metadata.is_directory
        {
            return Ok(crate::volume::CopyScanResult {
                file_count: 1,
                dir_count: 0,
                total_bytes: entry.metadata.size.unwrap_or(0),
                // In-memory volume has no hardlinks: footprints are equal.
                dedup_bytes: entry.metadata.size.unwrap_or(0),
                top_level_is_directory: false,
                top_level_modified_at: entry.metadata.modified_at,
                top_level_created_at: entry.metadata.created_at,
            });
        }

        // Recursively scan all descendants
        let mut file_count = 0;
        let mut dir_count = 0;
        let mut total_bytes = 0u64;

        for (entry_path, entry) in entries.iter() {
            // Skip the root path itself, only count descendants
            if **entry_path == *normalized || !entry_path.starts_with(&normalized) {
                continue;
            }
            if Self::left_out(entry_path, entry, os_junk) {
                continue;
            }
            if entry.metadata.is_directory {
                dir_count += 1;
            } else {
                file_count += 1;
                total_bytes += entry.metadata.size.unwrap_or(0);
            }
        }

        Ok(crate::volume::CopyScanResult {
            file_count,
            dir_count,
            total_bytes,
            // In-memory volume has no hardlinks: footprints are equal.
            dedup_bytes: total_bytes,
            // We only reach this branch if the path isn't a known file
            // entry. In-memory roots and unknown paths both report `true`
            // to match how callers use the flag (fall through to the
            // directory-recursion path). Empty roots or unknown paths
            // behave like empty directories, which is the existing
            // contract on this backend.
            top_level_is_directory: true,
            top_level_modified_at: entries.get(&*normalized).and_then(|entry| entry.metadata.modified_at),
            top_level_created_at: entries.get(&*normalized).and_then(|entry| entry.metadata.created_at),
        })
    }

    /// Whether a scan under `os_junk` leaves out the descendant at `path`.
    fn left_out(path: &AppPath, entry: &InMemoryEntry, os_junk: crate::os_junk_files::OsJunkFiles) -> bool {
        path.file_name()
            .is_some_and(|name| os_junk.leaves_out_str(name, entry.metadata.is_directory))
    }

    pub(super) fn record_subtree(
        &self,
        path: &AppPath,
        recorder: &dyn crate::volume::ScanRecorder,
        os_junk: crate::os_junk_files::OsJunkFiles,
    ) {
        let normalized = self.normalize(path);
        let entries = self.entries.read_ignore_poison();
        // The recorder lives in the engines and still takes `std::path`: one crossing a row.
        match entries.get(&*normalized) {
            Some(entry) if !entry.metadata.is_directory => {
                recorder.file(&std_of(path), entry.metadata.size.unwrap_or(0));
                return;
            }
            _ => recorder.dir(&std_of(path)),
        }
        for (entry_path, entry) in entries.iter() {
            let Some(relative) = entry_path.strip_prefix(&normalized) else {
                continue;
            };
            if relative.is_empty() || Self::left_out(entry_path, entry, os_junk) {
                continue;
            }
            let Some(as_called) = join_tail(path, &relative) else {
                continue;
            };
            if entry.metadata.is_directory {
                recorder.dir(&std_of(&as_called));
            } else {
                recorder.file(&std_of(&as_called), entry.metadata.size.unwrap_or(0));
            }
        }
    }
}
