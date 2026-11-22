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
