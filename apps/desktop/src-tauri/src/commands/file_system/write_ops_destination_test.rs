//! A same-root move's destination, anchored on the boot volume's root.

use super::*;
use crate::file_system::volume::InMemoryVolume;

/// The same-drive move on Windows: the Move dialog's box holds a volume-relative
/// `/Users/…`, and the boot volume's root is `C:/`, so the move lands on that drive (the
/// engine once got the bare `/Users/…` and refused it as "a path shaped for another OS").
/// Windows rules on a stand-in rooted at `C:/`, run on every host.
#[test]
fn a_same_root_move_destination_lands_on_the_boot_volumes_drive() {
    let drive: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("C").with_root("C:/"));
    assert_eq!(
        same_root_destination_on(Some(drive), "/Users/Admin/w35/dst")
            .ok()
            .map(AppPathBuf::into_string),
        Some("C:/Users/Admin/w35/dst".to_string())
    );
}

/// On macOS and Linux the boot volume's root is `/`: the destination is the one sent, its
/// trailing `/` dropped. A `..` the person typed was resolved on the volume before this
/// (`wire_path::transfer_destination`), so one still here is a sender bug, refused: the
/// engine never sees a `..`.
#[test]
fn a_same_root_move_destination_on_a_posix_root_is_as_sent() {
    let root: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Root"));
    for (sent, start) in [
        ("/Users/test_user_1/dst", "/Users/test_user_1/dst"),
        ("/x/dst/", "/x/dst"),
    ] {
        assert_eq!(
            same_root_destination_on(Some(Arc::clone(&root)), sent)
                .ok()
                .map(AppPathBuf::into_string),
            Some(start.to_string())
        );
    }
    assert!(same_root_destination_on(Some(root), "/x/link/../dst").is_err());
}

/// The one shape that changed on a POSIX root: a relative box text anchors on `/`, as a
/// copy's does and as the dialog's validation (`anchorOnVolume`) already read it.
#[test]
fn a_relative_same_root_move_destination_anchors_on_the_root() {
    let root: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Root"));
    assert_eq!(
        same_root_destination_on(Some(root), "dst")
            .ok()
            .map(AppPathBuf::into_string),
        Some("/dst".to_string())
    );
}

/// The fast path's route guard FAILS CLOSED: a path with no native spelling on this host
/// can't be asked whether a zip, a `.git` portal, or a snapshot serves it, so it's refused
/// rather than skipped. Skipping let a mangled destination through unchecked (on Windows the
/// box's native `C:\…\a.zip/inner` anchors as a name holding `:` and `\`, which Win32 can't
/// store), and the local fast path has no file to act on for it either way.
#[test]
fn a_path_with_no_native_spelling_is_refused_never_skipped() {
    let scheme = AppPathBuf::from_wire("smb://testnas.local/share/a.zip/inner.txt").expect("a scheme path");
    for side in [ReadOnlySide::Source, ReadOnlySide::Destination] {
        let refused = reject_if_routed_over_a_parent(std::iter::once(&scheme), side);
        assert!(
            matches!(&refused, Err(WriteOperationError::IoError { path, .. }) if path == scheme.as_str()),
            "{refused:?}"
        );
    }
    // One unspellable path in a batch refuses the batch, wherever it sits.
    let dir = crate::test_support::TestDir::new("route_guard_fails_closed");
    let plain = cmdr_operations::testing::app_path_of(&dir.join("plain.txt"));
    assert!(reject_if_routed_over_a_parent([&plain, &scheme], ReadOnlySide::Source).is_err());
    assert!(reject_if_routed_over_a_parent([&scheme, &plain], ReadOnlySide::Source).is_err());
    assert!(reject_if_routed_over_a_parent([&plain], ReadOnlySide::Source).is_ok());
}

/// The Windows shape that slipped through: the boot volume rooted at `C:/` anchors a box's
/// native `C:\…` text as a RELATIVE name under its root, and that name has no native
/// spelling. Run on every host on a stand-in rooted at `C:/` (on macOS and Linux no drive
/// path has one either), through the same-root move's two steps.
#[test]
fn a_native_backslash_destination_on_a_drive_root_is_refused() {
    let drive: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("C").with_root("C:/"));
    let sent = r"C:\Users\test_user_1\photos.zip/inner";
    let refused = same_root_destination_on(Some(drive), sent)
        .and_then(|destination| reject_if_routed_over_a_parent([&destination], ReadOnlySide::Destination));
    assert!(refused.is_err(), "{refused:?}");
}

/// Sources too: a same-root move of a scheme path refuses before any engine call.
#[test]
fn a_local_transfer_of_a_scheme_path_is_refused() {
    let refused = local_transfer_request(&["smb://testnas.local/share/a.txt".to_string()], "/tmp/dst", None);
    assert!(
        matches!(refused, Err(WriteOperationError::IoError { .. })),
        "{:?}",
        refused.err()
    );
}
