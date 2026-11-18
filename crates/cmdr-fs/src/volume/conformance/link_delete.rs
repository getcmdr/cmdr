//! What [`Volume::delete`] and [`Volume::entry_kind`] promise about a symbolic
//! link to a folder: it's a leaf, and deleting it removes the link alone.

use crate::app_path::AppPath;
use std::path::Path;

use crate::volume::{EntryKind, Volume, VolumeError};

/// A symbolic link to a folder reads as a link ([`Volume::entry_kind`]), and
/// [`Volume::delete`] on it removes the LINK, with the folder it points at and
/// everything in it left exactly as they were.
///
/// `link` must already exist on `volume` as a symbolic link to the directory
/// `target`, which holds `child_name` directly inside it.
///
/// **Why this one is worth a shared assertion.** A recursive delete walks a
/// tree by asking what each entry is, and treats a link as a leaf: one
/// `delete` of the link itself. A backend whose `entry_kind` followed the link
/// would send the walker INTO the target, and one whose `delete` followed it
/// would refuse (`rmdir` on a link) or, worse, act on the target. Either way a
/// folder outside the selection is at stake (#505).
///
/// Backends with no links (MTP, S3, WebDAV, SMB) have nothing to run this
/// against.
pub async fn assert_delete_of_a_link_to_a_folder_removes_the_link_alone(
    volume: &dyn Volume,
    link: &Path,
    target: &Path,
    child_name: &str,
) {
    let kind = volume
        .entry_kind(&AppPath::legacy_unchecked(link))
        .await
        .unwrap_or_else(|e| panic!("entry_kind({}) must answer for a link, got {e:?}", link.display()));
    assert_eq!(
        kind,
        EntryKind::Symlink,
        "entry_kind({}) on {} must report the link AS a link, never what it points at",
        link.display(),
        volume.name(),
    );
    let before = volume
        .list_directory(&AppPath::legacy_unchecked(target), None)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "fixture precondition: listing {} must work, got {e:?}",
                target.display()
            )
        });
    assert!(
        before.iter().any(|e| e.name == child_name),
        "fixture precondition: {} must hold {child_name}",
        target.display(),
    );

    let outcome = volume.delete(&AppPath::legacy_unchecked(link)).await;
    assert!(
        outcome.is_ok(),
        "delete({}) on {} must remove the link itself, got {outcome:?}",
        link.display(),
        volume.name(),
    );
    assert!(
        matches!(
            volume.entry_kind(&AppPath::legacy_unchecked(link)).await,
            Err(VolumeError::NotFound(_))
        ),
        "the link {} must be gone after its delete",
        link.display(),
    );
    let after = volume
        .list_directory(&AppPath::legacy_unchecked(target), None)
        .await
        .unwrap_or_else(|e| panic!("the link's target {} must still be there, got {e:?}", target.display()));
    assert!(
        after.iter().any(|e| e.name == child_name),
        "deleting a link must leave its target alone, but {child_name} is gone from {}",
        target.display(),
    );
}
