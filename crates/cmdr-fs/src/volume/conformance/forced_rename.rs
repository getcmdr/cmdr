//! What a confirmed replace (`rename` with `force == true`) may take away.

use crate::app_path::AppPath;
use std::path::Path;

use super::super::Volume;

/// [`Volume::rename`] with `force == true` never takes a folder that still holds
/// something: it refuses, and both ends stay exactly as they were.
///
/// `dir` must be a directory holding at least one entry, and `from` must exist
/// (a file or a folder: callers run it with each). The assertion checks both
/// preconditions first.
///
/// **Why this one is worth a shared assertion.** `force` is the user's "replace
/// what's there", given in a dialog that compares two entries, never a tree. The
/// local kernel refuses this shape by itself (`EISDIR`, `ENOTEMPTY`), but the
/// verbs other backends reach for don't: WebDAV's `MOVE` with `Overwrite: T`
/// deletes the whole destination tree first (RFC 4918 § 9.9.3), and a shell's
/// `mv -f` onto a folder moves the source INTO it, replacing a same-named entry
/// inside. Either one turns a confirmed rename into a lost folder.
pub async fn assert_forced_rename_keeps_a_non_empty_dir(volume: &dyn Volume, from: &Path, dir: &Path) {
    assert!(
        volume.exists(&AppPath::legacy_unchecked(from)).await,
        "fixture precondition: {} must exist",
        from.display()
    );
    let before = listing(volume, dir, "fixture precondition").await;
    assert!(
        !before.is_empty(),
        "fixture precondition: {} must be a directory that holds something",
        dir.display()
    );

    let outcome = volume
        .rename(&AppPath::legacy_unchecked(from), &AppPath::legacy_unchecked(dir), true)
        .await;
    assert!(
        outcome.is_err(),
        "rename({}, {}, force = true) onto a folder that holds {before:?} must refuse; it returned Ok",
        from.display(),
        dir.display(),
    );

    assert!(
        volume.exists(&AppPath::legacy_unchecked(from)).await,
        "a refused rename must leave the source in place, but {} is gone",
        from.display(),
    );
    let after = listing(volume, dir, "after the refused rename").await;
    assert_eq!(
        after,
        before,
        "a refused rename must not touch the folder, but {} changed",
        dir.display(),
    );
}

/// `dir`'s entries as sorted `(name, size)` pairs, so "unchanged" covers both
/// an entry going missing and one being overwritten in place.
async fn listing(volume: &dyn Volume, dir: &Path, what: &str) -> Vec<(String, Option<u64>)> {
    let mut entries: Vec<(String, Option<u64>)> = volume
        .list_directory(&AppPath::legacy_unchecked(dir), None)
        .await
        .unwrap_or_else(|e| panic!("{what}: listing {} must work, got {e:?}", dir.display()))
        .into_iter()
        .map(|entry| (entry.name, entry.size))
        .collect();
    entries.sort();
    entries
}
