//! Same-named siblings, one row each.
//!
//! PTP lets a folder hold two objects with the same name (a camera merging card
//! folders after a counter wrap, an e-reader's own object database, Android's
//! MTP index gone stale), so a name alone can't say which one a row means. A
//! listing gives each twin's row its own path instead: a pick segment carrying
//! the object's handle sits right before the shared name,
//! `/DCIM/␀4242/IMG_1.JPG`. The grammar (and the rule that a pick never reaches
//! a person) is `cmdr_fs::twin_path`'s; what's here is that the id is an MTP
//! object handle. Every split is the app path's own (`storage_path.rs`), so a
//! twin's row reads the same on every OS.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};
use cmdr_fs::twin_path;
use mtp_rs::ObjectHandle;

/// The folder holding `path`'s object, stepping over a pick; `None` at a root.
pub(crate) fn folder_of(path: &AppPath) -> Option<&AppPath> {
    twin_path::folder_of_app(path)
}

/// The row path of `handle`, one of several objects in `folder` named `name`
/// (the last name of the path they share, so already one plain name).
pub(super) fn picked(folder: &AppPath, handle: ObjectHandle, name: &str) -> Option<AppPathBuf> {
    let pick = twin_path::pick_segment(handle.0);
    let picked = folder.join(AppName::from_app_spelling(&pick).ok()?).ok()?;
    picked.join(AppName::from_app_spelling(name).ok()?).ok()
}

/// The folder `parent` names when a caller hands over a twin's row path minus
/// its name (the parent of a row path), else `None`. A change reported "in"
/// such a parent is a change to a twin, in the folder this returns.
pub(crate) fn picked_folder(parent: &AppPath) -> Option<&AppPath> {
    parent
        .file_name()
        .and_then(twin_path::pick_id_of_name)
        .and(parent.parent())
}

/// The handle `path` picks for its own object, when it's a twin's row path.
/// A pick further up names an ancestor, which this doesn't report.
pub(crate) fn pick_in(path: &AppPath) -> Option<ObjectHandle> {
    twin_path::own_pick_app(path).map(ObjectHandle)
}

/// `path` without its own pick: the path every one of its twins shares. An
/// ordinary path comes back as it is.
pub(super) fn shared_path(path: &AppPath) -> AppPathBuf {
    twin_path::unpicked_app(path).into_owned()
}

/// Gives each twin in one folder's listing its own row path, in the entries
/// and in the path-cache updates alike (the listing builds the two in step, one
/// per object). Every other row keeps its plain path.
pub(super) fn give_twins_their_own_paths(
    folder: &AppPath,
    entries: &mut [cmdr_fs::entry::FileEntry],
    cache_updates: &mut [(AppPathBuf, ObjectHandle)],
) {
    debug_assert_eq!(entries.len(), cache_updates.len(), "one cache update per row");
    let mut seen: std::collections::HashMap<&AppPath, usize> = std::collections::HashMap::new();
    for (path, _) in cache_updates.iter() {
        *seen.entry(path.as_app_path()).or_default() += 1;
    }
    let shared: std::collections::HashSet<AppPathBuf> = seen
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(path, _)| path.to_owned())
        .collect();
    if shared.is_empty() {
        return;
    }
    for (entry, (path, handle)) in entries.iter_mut().zip(cache_updates.iter_mut()) {
        if !shared.contains(path.as_app_path()) {
            continue;
        }
        let Some(own) = path.file_name().and_then(|name| picked(folder, *handle, name)) else {
            continue;
        };
        debug_assert_eq!(
            entry.inode,
            Some(handle.0),
            "the row and its cache update are one object"
        );
        entry.path = own.as_str().to_string();
        *path = own;
    }
}
