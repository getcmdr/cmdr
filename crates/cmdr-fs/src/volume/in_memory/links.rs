//! `InMemoryVolume`'s symlinks: the knobs that turn them on, and how one is stored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{InMemoryEntry, InMemoryVolume};
use crate::entry::FileEntry;
use crate::ignore_poison::RwLockIgnorePoison;

/// How many links [`InMemoryVolume::canonicalize`] follows before it calls the path a
/// loop: Linux's `MAXSYMLINKS`, the same order as macOS's 32.
const MAX_LINK_HOPS: usize = 40;

impl InMemoryVolume {
    /// Makes this volume hold symlinks: [`Volume::can_hold_links`](crate::volume::Volume::can_hold_links)
    /// says so and [`Volume::create_symlink`](crate::volume::Volume::create_symlink) makes them.
    pub fn with_links(mut self) -> Self {
        self.holds_links = true;
        self
    }

    /// Claims links through `can_hold_links` but refuses every `create_symlink`:
    /// a link-capable backend on a filesystem that has none.
    pub fn with_links_refused(mut self) -> Self {
        self.holds_links = true;
        self.refuses_links = true;
        self
    }

    /// Stores a symlink at `path` directly, for a source a test reads from. The
    /// `create_symlink` route needs [`Self::with_links`].
    pub fn plant_symlink(&self, path: &Path, target: &Path) {
        let mut entries = self.entries.write_ignore_poison();
        Self::insert_link(&mut entries, self.normalize(path), target);
    }

    /// Gives the symlink row already at `path` its `target`, stored as given, for a
    /// volume built from listings ([`Self::with_entries`]), whose link rows carry none.
    /// The row keeps its listing (a link to a folder still lists as one). A path
    /// holding no symlink row is left alone.
    pub fn with_link_target(self, path: &Path, target: &Path) -> Self {
        {
            let mut entries = self.entries.write_ignore_poison();
            if let Some(entry) = entries.get_mut(&self.normalize(path)).filter(|e| e.metadata.is_symlink) {
                entry.link_target = Some(target.to_path_buf());
            }
        }
        self
    }

    /// `path` with every symlink along it resolved, the way `realpath` resolves
    /// one on a disk: a relative target against the link's folder, a link to a
    /// link followed on, `.` and `..` taken as they come. `None` when a part of
    /// it doesn't exist or the links loop (more than [`MAX_LINK_HOPS`]), where
    /// `realpath` fails too. Names match exactly: the volume has no case rules.
    ///
    /// The volume's paths are `/`-separated app text, so the walk splits and joins on `/`
    /// alone, the same on every OS (a `Path` on Windows split the root into `\` and joined
    /// with it).
    pub fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        let entries = self.entries.read_ignore_poison();
        let mut pending: Vec<String> = Vec::new();
        let push_reversed = |pending: &mut Vec<String>, text: &str| {
            pending.extend(text.split('/').rev().map(str::to_string));
        };
        push_reversed(&mut pending, &self.normalize(path).to_string_lossy());
        let mut resolved = String::from("/");
        let mut hops = 0;
        while let Some(part) = pending.pop() {
            match part.as_str() {
                "" | "." => continue,
                ".." => {
                    let cut = resolved.rfind('/').unwrap_or(0);
                    resolved.truncate(cut.max(1));
                    continue;
                }
                _ => {}
            }
            let next_text = if resolved == "/" {
                format!("/{part}")
            } else {
                format!("{resolved}/{part}")
            };
            let entry = entries.get(Path::new(&next_text))?;
            match &entry.link_target {
                Some(target) => {
                    hops += 1;
                    if hops > MAX_LINK_HOPS {
                        return None;
                    }
                    let target = target.to_string_lossy();
                    if target.starts_with('/') {
                        resolved = String::from("/");
                    }
                    push_reversed(&mut pending, &target);
                }
                None => resolved = next_text,
            }
        }
        // The root itself is always there; anything else was found on the way.
        Some(PathBuf::from(resolved))
    }

    /// A symlink row holding `target`, stored as given: a leaf with no bytes,
    /// reported as the link whatever it points at.
    pub(super) fn insert_link(entries: &mut HashMap<PathBuf, InMemoryEntry>, normalized: PathBuf, target: &Path) {
        let name = normalized
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let metadata = FileEntry {
            size: Some(0),
            modified_at: Some(Self::now_secs()),
            ..FileEntry::new(name, normalized.display().to_string(), false, true)
        };
        Self::touch_parent_of(entries, &normalized);
        let mut entry = InMemoryEntry::new(metadata, Some(super::InMemoryContent::bytes(Vec::new())));
        entry.link_target = Some(target.to_path_buf());
        entries.insert(normalized, entry);
    }
}
