//! Bytes from outside the store: a file or folder a visitor dropped into the
//! browser demo from their own computer (`crates/cmdr-listing-wasm/src/imports.rs`).
//! An imported file holds its real bytes ([`InMemoryContent::Bytes`]) beside the
//! seeded files' synthetic ones, so a read, a copy, a search, and a folder size
//! treat it like any other file.
//!
//! Both doors keep the no-clobber promise `create_file` makes: a taken name is
//! refused, never replaced. Putting it beside a name that's there is the
//! caller's choice (the demo stages each drop in a fresh folder and lets the
//! app's transfer dialog settle collisions).

use std::path::Path;

use crate::app_path::{AppPath, AppPathBuf};

use super::{InMemoryContent, InMemoryEntry, InMemoryVolume};
use crate::entry::FileEntry;
use crate::ignore_poison::RwLockIgnorePoison;
use crate::volume::VolumeError;

impl InMemoryVolume {
    /// Puts a folder at `path`, dated `modified_at` (seconds; now when `None`).
    ///
    /// Refuses a taken name (`AlreadyExists`) and a parent the store doesn't
    /// hold as a folder (`NotFound`).
    pub async fn import_folder(&self, path: &Path, modified_at: Option<u64>) -> Result<(), VolumeError> {
        let normalized = self.normalize(&super::app_of(path)).into_owned();
        {
            let mut entries = self.entries.write_ignore_poison();
            Self::check_import_target(&entries, &normalized)?;
            let metadata = FileEntry {
                modified_at: Some(modified_at.unwrap_or_else(Self::now_secs)),
                created_at: Some(Self::now_secs()),
                permissions: 0o755,
                ..FileEntry::new(super::name_of(&normalized), super::shown(&normalized), true, false)
            };
            Self::touch_parent_of(&mut entries, &normalized);
            entries.insert(normalized.clone(), InMemoryEntry::new(metadata, None));
        }
        self.patch_created_at(&normalized).await;
        Ok(())
    }

    /// Puts a file holding `bytes` at `path`, dated `modified_at` (seconds; now
    /// when `None`), stored as real bytes.
    ///
    /// Refuses a taken name (`AlreadyExists`), a parent the store doesn't hold
    /// as a folder (`NotFound`), and bytes the disk has no room for
    /// (`StorageFull`, under [`with_capacity`](Self::with_capacity)), all
    /// before it stores anything.
    pub async fn import_file(&self, path: &Path, bytes: Vec<u8>, modified_at: Option<u64>) -> Result<(), VolumeError> {
        let normalized = self.normalize(&super::app_of(path)).into_owned();
        let content = InMemoryContent::bytes(bytes);
        self.check_room_for(content.len())?;
        {
            let mut entries = self.entries.write_ignore_poison();
            Self::check_import_target(&entries, &normalized)?;
            let metadata = FileEntry {
                size: Some(content.len()),
                modified_at: Some(modified_at.unwrap_or_else(Self::now_secs)),
                created_at: Some(Self::now_secs()),
                permissions: 0o644,
                ..FileEntry::new(super::name_of(&normalized), super::shown(&normalized), false, false)
            };
            Self::touch_parent_of(&mut entries, &normalized);
            entries.insert(normalized.clone(), InMemoryEntry::new(metadata, Some(content)));
        }
        self.patch_created_at(&normalized).await;
        Ok(())
    }

    /// The refusals both doors share: a taken name, and a parent that isn't a
    /// folder here. The volume root counts as held.
    fn check_import_target(
        entries: &std::collections::HashMap<AppPathBuf, InMemoryEntry>,
        normalized: &AppPath,
    ) -> Result<(), VolumeError> {
        if entries.contains_key(normalized) {
            return Err(VolumeError::AlreadyExists(super::shown(normalized)));
        }
        let parent = Self::parent_of(normalized);
        let parent_held = *parent == *super::slash() || entries.get(parent).is_some_and(|e| e.metadata.is_directory);
        if !parent_held {
            return Err(VolumeError::NotFound(super::shown(parent)));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "imports_test.rs"]
mod tests;
