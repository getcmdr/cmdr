//! The patch arithmetic, against a recording listing host.
//!
//! ❗ A wrong patch here is a pane showing a file that no longer exists, or
//! missing one that does, after an operation the user watched succeed.

use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::volume::VolumeError;
use crate::volume::host::listings::RecordingListings;

/// A backend whose paths are already the app's, with a fixed set of entries.
struct FakeBackend {
    listings: Arc<RecordingListings>,
    entries: HashMap<String, bool>,
    /// Paths this backend refuses to translate, standing in for "not on this
    /// volume".
    off_volume: Vec<String>,
}

impl FakeBackend {
    fn new(entries: &[(&str, bool)]) -> Self {
        Self {
            listings: Arc::new(RecordingListings::default()),
            entries: entries.iter().map(|(p, d)| ((*p).to_string(), *d)).collect(),
            off_volume: Vec::new(),
        }
    }

    fn refusing(mut self, path: &str) -> Self {
        self.off_volume.push(path.to_string());
        self
    }

    fn changes(&self) -> Vec<(String, PathBuf, DirectoryChange)> {
        self.listings.changes()
    }
}

impl PatchSource for FakeBackend {
    fn patch_volume_id(&self) -> &str {
        "test-volume"
    }

    fn patch_listings(&self) -> &dyn ListingHost {
        self.listings.as_ref()
    }

    fn patch_stat<'a>(&'a self, path: &'a AppPath) -> Walking<'a, FileEntry> {
        Box::pin(async move {
            let key = path.as_str().to_string();
            match self.entries.get(&key) {
                Some(is_dir) => {
                    let name = path.file_name().map(str::to_string).unwrap_or_default();
                    Ok(FileEntry::new(name, key, *is_dir, false))
                }
                None => Err(VolumeError::NotFound(key)),
            }
        })
    }

    fn patch_display_path(&self, path: &Path) -> Option<PathBuf> {
        let key = path.to_string_lossy().into_owned();
        (!self.off_volume.contains(&key)).then(|| path.to_path_buf())
    }
}

#[tokio::test]
async fn a_new_file_is_added_to_its_parents_listing() {
    let backend = FakeBackend::new(&[("/dir/new.txt", false)]);

    patch_created(&backend, Path::new("/dir/new.txt")).await;

    let changes = backend.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, "test-volume");
    assert_eq!(changes[0].1, PathBuf::from("/dir"));
    assert!(matches!(&changes[0].2, DirectoryChange::Added(entry) if entry.name == "new.txt"));
}

#[tokio::test]
async fn a_deleted_file_needs_no_stat_to_leave_its_listing() {
    // ❗ Deliberately not in `entries`: the thing is gone, so a stat that had to
    // succeed would mean a delete could never be patched at all.
    let backend = FakeBackend::new(&[]);

    patch_deleted(&backend, Path::new("/dir/gone.txt")).await;

    let changes = backend.changes();
    assert_eq!(changes.len(), 1);
    assert!(matches!(&changes[0].2, DirectoryChange::Removed(name) if name == "gone.txt"));
}

#[tokio::test]
async fn a_rename_inside_one_directory_is_a_single_renamed_change() {
    let backend = FakeBackend::new(&[("/dir/after.txt", false)]);

    patch_renamed(&backend, Path::new("/dir/before.txt"), Path::new("/dir/after.txt")).await;

    let changes = backend.changes();
    assert_eq!(changes.len(), 1, "one listing moved, so one change");
    assert!(matches!(
        &changes[0].2,
        DirectoryChange::Renamed { old_name, new_entry } if old_name == "before.txt" && new_entry.name == "after.txt"
    ));
}

#[tokio::test]
async fn a_rename_across_directories_is_a_loss_here_and_a_gain_there() {
    // ❗ A single `Renamed` would leave whichever pane it wasn't sent to stale.
    let backend = FakeBackend::new(&[("/to/moved.txt", false)]);

    patch_renamed(&backend, Path::new("/from/moved.txt"), Path::new("/to/moved.txt")).await;

    let changes = backend.changes();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].1, PathBuf::from("/from"));
    assert!(matches!(&changes[0].2, DirectoryChange::Removed(name) if name == "moved.txt"));
    assert_eq!(changes[1].1, PathBuf::from("/to"));
    assert!(matches!(&changes[1].2, DirectoryChange::Added(entry) if entry.name == "moved.txt"));
}

#[tokio::test]
async fn a_stat_that_does_not_answer_is_no_patch_rather_than_a_failure() {
    // ❗ The mutation already landed. A patch is a courtesy, and this function
    // has no way to report a problem even if it wanted to.
    let backend = FakeBackend::new(&[]);

    patch_created(&backend, Path::new("/dir/never-stats.txt")).await;

    assert!(backend.changes().is_empty());
}

#[tokio::test]
async fn a_parent_that_is_not_on_this_volume_is_no_patch() {
    let backend = FakeBackend::new(&[("/elsewhere/f.txt", false)]).refusing("/elsewhere");

    patch_created(&backend, Path::new("/elsewhere/f.txt")).await;

    assert!(backend.changes().is_empty());
}

#[tokio::test]
async fn a_path_with_no_parent_is_no_patch() {
    let backend = FakeBackend::new(&[("/", true)]);

    patch_created(&backend, Path::new("/")).await;

    assert!(backend.changes().is_empty());
}

/// Whether a rename stays in one listing is `Path` equality on the two parents, and
/// `Path::parent` already drops a `.` and a doubled `/` before the last name: two
/// spellings of one folder make one `Renamed` change, filed and stat-ed under the clean
/// parent. A typed app path must answer the same parent, or a rename inside one folder
/// turns into a remove plus an add.
#[tokio::test]
async fn a_rename_between_two_spellings_of_one_folder_is_a_single_renamed_change() {
    let backend = FakeBackend::new(&[("/dir/after.txt", false)]);

    patch_renamed(&backend, Path::new("/dir/./before.txt"), Path::new("/dir//after.txt")).await;

    let changes = backend.changes();
    assert_eq!(changes.len(), 1, "one folder, two spellings, one change");
    assert_eq!(changes[0].1.as_os_str(), "/dir");
    assert!(matches!(
        &changes[0].2,
        DirectoryChange::Renamed { old_name, new_entry } if old_name == "before.txt" && new_entry.name == "after.txt"
    ));
}

/// On a scheme path the parent is `Path::parent`'s slice, the folder as the pane holds it.
#[tokio::test]
async fn a_scheme_paths_change_is_filed_under_its_folder() {
    let backend = FakeBackend::new(&[("sftp://test_user_1@nas:22/srv/new.txt", false)]);

    patch_created(&backend, Path::new("sftp://test_user_1@nas:22/srv/new.txt")).await;

    let changes = backend.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].1.as_os_str(), "sftp://test_user_1@nas:22/srv");
}

/// ❗ A remote name holding `\` or `:` is one name on every host: the patch is filed under
/// its real folder and stats the real child, where a Windows `Path` split it at the `\`.
#[tokio::test]
async fn a_remote_name_with_a_backslash_stays_one_name() {
    for name in ["a\\b.txt", "12:00 notes", "C:"] {
        let child = format!("sftp://test_user_1@nas:22/srv/{name}");
        let backend = FakeBackend::new(&[(child.as_str(), false)]);

        patch_created(&backend, Path::new(&child)).await;

        let changes = backend.changes();
        assert_eq!(changes.len(), 1, "{name:?}");
        assert_eq!(changes[0].1.as_os_str(), "sftp://test_user_1@nas:22/srv", "{name:?}");
        assert!(
            matches!(&changes[0].2, DirectoryChange::Added(entry) if entry.name == name),
            "{name:?}"
        );
    }
}

/// A parent with no canonical twin (a relative folder a local engine hands its backend, a
/// `..` spelling) joins its child as `Path::join` joined it, ❌ never drops the patch: the
/// scan walk's own rule for such a folder.
#[tokio::test]
async fn a_parent_with_no_canonical_twin_still_patches_its_child() {
    for parent in ["rel/dir", "/a/../b"] {
        let child = format!("{parent}/new.txt");
        let backend = FakeBackend::new(&[(child.as_str(), false), (format!("{parent}/after.txt").as_str(), false)]);

        patch_mutation(&backend, Path::new(parent), MutationEvent::Created("new.txt".into())).await;
        patch_mutation(
            &backend,
            Path::new(parent),
            MutationEvent::Renamed {
                from: "before.txt".into(),
                to: "after.txt".into(),
            },
        )
        .await;

        let changes = backend.changes();
        assert_eq!(changes.len(), 2, "{parent:?}");
        assert!(
            matches!(&changes[0].2, DirectoryChange::Added(entry) if entry.name == "new.txt"),
            "{parent:?}"
        );
        assert!(
            matches!(&changes[1].2, DirectoryChange::Renamed { new_entry, .. } if new_entry.name == "after.txt"),
            "{parent:?}"
        );
    }
}
