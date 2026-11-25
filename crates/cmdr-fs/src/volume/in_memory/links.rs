//! `InMemoryVolume`'s symlinks: the knobs that turn them on, and how one is stored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{InMemoryEntry, InMemoryVolume, app_of, name_of, shown, slash, std_of};
use crate::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};
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
        let normalized = self.normalize(&app_of(path)).into_owned();
        let mut entries = self.entries.write_ignore_poison();
        Self::insert_link(&mut entries, normalized, target);
    }

    /// Gives the symlink row already at `path` its `target`, stored as given, for a
    /// volume built from listings ([`Self::with_entries`]), whose link rows carry none.
    /// The row keeps its listing (a link to a folder still lists as one). A path
    /// holding no symlink row is left alone.
    pub fn with_link_target(self, path: &Path, target: &Path) -> Self {
        {
            let app = app_of(path);
            let normalized = self.normalize(&app);
            let mut entries = self.entries.write_ignore_poison();
            if let Some(entry) = entries.get_mut(&*normalized).filter(|e| e.metadata.is_symlink) {
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
    /// The volume's paths are `/`-separated app text, so the walk splits on `/` alone
    /// and joins by the app path's own join, the same on every OS (a `Path` on Windows
    /// split `a\b` into two names). A name that isn't UTF-8 walks as its escapes, which
    /// key its row, ❌ never a lossy spelling that names no row (or a sibling).
    pub fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        let app = app_of(path);
        self.resolve_links(&self.normalize(&app))
            .map(|resolved| std_of(&resolved))
    }

    /// [`Self::canonicalize`] for an app path, answered as one: the store's keys are
    /// app paths, so the resolved key is the answer, on every host.
    pub fn canonical_app_path(&self, path: &AppPath) -> Option<AppPathBuf> {
        self.resolve_links(path)
    }

    /// The walk behind [`Self::canonicalize`], over `path`'s `/`-separated names from `/`.
    fn resolve_links(&self, path: &AppPath) -> Option<AppPathBuf> {
        let entries = self.entries.read_ignore_poison();
        let mut pending: Vec<String> = Vec::new();
        let push_reversed = |pending: &mut Vec<String>, text: &str| {
            pending.extend(text.split('/').rev().map(str::to_owned));
        };
        push_reversed(&mut pending, path.as_str());
        let mut resolved: AppPathBuf = slash().to_owned();
        let mut hops = 0;
        while let Some(part) = pending.pop() {
            match part.as_str() {
                "" | "." => continue,
                ".." => {
                    resolved = Self::parent_of(&resolved).to_owned();
                    continue;
                }
                _ => {}
            }
            let next = resolved.join(AppName::from_app_spelling(&part).ok()?).ok()?;
            let entry = entries.get(&next)?;
            match &entry.link_target {
                Some(target) => {
                    hops += 1;
                    if hops > MAX_LINK_HOPS {
                        return None;
                    }
                    let target = app_of(target);
                    if target.as_str().starts_with('/') {
                        resolved = slash().to_owned();
                    }
                    push_reversed(&mut pending, target.as_str());
                }
                None => resolved = next,
            }
        }
        // The root itself is always there; anything else was found on the way.
        Some(resolved)
    }
}

impl InMemoryVolume {
    /// A symlink row holding `target`, stored as given: a leaf with no bytes,
    /// reported as the link whatever it points at.
    pub(super) fn insert_link(entries: &mut HashMap<AppPathBuf, InMemoryEntry>, normalized: AppPathBuf, target: &Path) {
        let metadata = FileEntry {
            size: Some(0),
            modified_at: Some(Self::now_secs()),
            ..FileEntry::new(name_of(&normalized), shown(&normalized), false, true)
        };
        Self::touch_parent_of(entries, &normalized);
        let mut entry = InMemoryEntry::new(metadata, Some(super::InMemoryContent::bytes(Vec::new())));
        entry.link_target = Some(target.to_path_buf());
        entries.insert(normalized, entry);
    }
}
