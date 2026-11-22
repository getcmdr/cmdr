//! The user flags a copy carries onto this OS's local disk, held to what its caps claim:
//! Hidden and Locked on macOS (`chflags`) and Windows (the hidden attribute, and a FILE's
//! read-only one), and none on Linux (`chattr` needs a capability a desktop app lacks). The
//! shared conformance suite leaves the flags to each backend's own suite; this is the local
//! disk's.

use crate::test_support::{at, volume_at};
use cmdr_fs::metadata::{CarriedMetadata, ClassOutcome, MetadataClasses, Unblock, UserFlags};
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::Volume;

/// What `volume` reports of `rel`'s flags: a file's off its read stream (what a copy off it
/// reads), a folder's off `folder_metadata`.
async fn reported_flags(volume: &impl Volume, rel: &str, is_folder: bool) -> UserFlags {
    if is_folder {
        return volume
            .folder_metadata(&at(volume, rel), MetadataClasses::FLAGS)
            .await
            .flags;
    }
    let stream = volume
        .open_read_stream(&at(volume, rel))
        .await
        .unwrap_or_else(|e| panic!("{rel} must be readable: {e:?}"));
    stream.metadata(MetadataClasses::FLAGS).flags
}

#[tokio::test]
async fn a_landed_file_keeps_hidden_and_locked_where_the_caps_claim_flags() {
    let dir = AppTestDir::new("local_flags_file");
    let volume = volume_at(&dir);
    volume
        .create_file(&at(&volume, "frozen.txt"), b"keep me")
        .await
        .unwrap();
    let flags = UserFlags::HIDDEN | UserFlags::LOCKED;

    let report = volume
        .apply_metadata(
            &at(&volume, "frozen.txt"),
            &CarriedMetadata {
                flags,
                ..CarriedMetadata::default()
            },
            None,
        )
        .await;

    if volume.metadata_caps().stores.contains(MetadataClasses::FLAGS) {
        assert_eq!(report.flags, ClassOutcome::Kept, "{report:?}");
        assert!(
            volume.metadata_caps().reports.contains(MetadataClasses::FLAGS),
            "a disk that keeps the flags reports them too"
        );
        assert_eq!(reported_flags(&volume, "frozen.txt", false).await, flags);
        assert!(
            matches!(
                volume.clear_blockers(&at(&volume, "frozen.txt")).await,
                Unblock::Cleared(_)
            ),
            "what the copy locked, Cmdr can unlock"
        );
    } else {
        assert_eq!(report.flags, ClassOutcome::NotApplicable, "{report:?}");
    }
    volume.delete(&at(&volume, "frozen.txt")).await.unwrap();
}

/// A folder takes Hidden, and on Windows never Locked: its read-only bit is Explorer's
/// customization marker.
#[tokio::test]
async fn a_landed_folder_keeps_hidden_where_the_caps_claim_flags() {
    let dir = AppTestDir::new("local_flags_folder");
    let volume = volume_at(&dir);
    volume.create_directory(&at(&volume, "quiet")).await.unwrap();

    let report = volume
        .apply_metadata(
            &at(&volume, "quiet"),
            &CarriedMetadata {
                flags: UserFlags::HIDDEN,
                ..CarriedMetadata::default()
            },
            None,
        )
        .await;

    if volume.metadata_caps().folder_stores.contains(MetadataClasses::FLAGS) {
        assert_eq!(report.flags, ClassOutcome::Kept, "{report:?}");
        assert_eq!(reported_flags(&volume, "quiet", true).await, UserFlags::HIDDEN);
    } else {
        assert_eq!(report.flags, ClassOutcome::NotApplicable, "{report:?}");
    }
}
