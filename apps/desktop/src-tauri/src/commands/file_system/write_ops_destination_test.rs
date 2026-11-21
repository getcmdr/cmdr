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
        same_root_destination_on(Some(drive), "/Users/Admin/w35/dst"),
        PathBuf::from("C:/Users/Admin/w35/dst")
    );
}

/// On macOS and Linux the boot volume's root is `/`: the destination is the one sent, a
/// `..` the person typed included (the OS resolves it, as a copy's).
#[test]
fn a_same_root_move_destination_on_a_posix_root_is_as_sent() {
    let root: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Root"));
    for sent in ["/Users/test_user_1/dst", "/x/link/../dst", "/x/dst/"] {
        assert_eq!(
            same_root_destination_on(Some(Arc::clone(&root)), sent),
            PathBuf::from(sent)
        );
    }
}
