//! The counters, and the probe's one promise: a pair that loses nothing never
//! asks the source anything.
#![cfg(unix)] // POSIX-only cells: `LocalPosixVolume`, POSIX modes, or errnos (windows-port plan, tasks 35 and 36).

use std::future::Future;
use std::pin::Pin;

use cmdr_fs::metadata::{CarriedMetadata, MetadataCaps, MetadataClasses};

use super::*;
use cmdr_fs::volume::{InMemoryVolume, StreamLength, VolumeError};

/// A stream that fails the test the moment anyone asks what it carries.
struct NeverAsked;

impl VolumeReadStream for NeverAsked {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        Box::pin(async { None })
    }
    fn total_size(&self) -> StreamLength {
        StreamLength::Known(0)
    }
    fn bytes_read(&self) -> u64 {
        0
    }
    fn modified_at(&self) -> Option<std::time::SystemTime> {
        None
    }
    fn metadata(&self, wanted: MetadataClasses) -> CarriedMetadata {
        panic!("a pair that loses nothing asked the source for {wanted:?}")
    }
    fn metadata_outline(&self, wanted: MetadataClasses) -> CarriedMetadata {
        panic!("a pair that loses nothing asked the source for {wanted:?}")
    }
}

fn volume_with(caps: MetadataCaps) -> InMemoryVolume {
    InMemoryVolume::new("Double").with_metadata_caps(caps)
}

/// Invariant 10: an extract onto local disk, and a copy between two shares,
/// keep everything the source reports, so the probe costs nothing.
#[test]
fn a_pair_that_loses_nothing_never_asks_the_source() {
    let archive = volume_with(MetadataCaps {
        reports: MetadataClasses::MODE.union(MetadataClasses::XATTRS),
        ..MetadataCaps::NONE
    });
    let local = volume_with(cmdr_local::LocalPosixVolume::METADATA_CAPS);
    let share = volume_with(cmdr_smb::volume::SmbVolume::METADATA_CAPS);

    assert!(file_losses(&archive, &local, &NeverAsked).is_empty());
    assert!(file_losses(&share, &share, &NeverAsked).is_empty());
}

#[test]
fn nothing_noted_is_nothing_left_behind() {
    assert_eq!(MetadataLosses::default().left_behind("op"), None);
}

/// Each engine class lands in its person-facing class, folders add their own,
/// and the event lists them in one fixed order with a count each.
#[test]
fn the_counts_come_out_per_class_in_a_fixed_order() {
    let losses = MetadataLosses::default();
    losses.note_file(MetadataClasses::XATTRS | MetadataClasses::CREATED);
    losses.note_file(MetadataClasses::XATTRS);
    losses.note_folder(FolderLosses {
        created: true,
        modified: true,
        xattrs: false,
        flags: false,
    });
    losses.note_folder(FolderLosses {
        modified: true,
        ..FolderLosses::default()
    });

    let left = losses.left_behind("op").expect("something stayed behind");

    assert_eq!(
        left.classes,
        vec![
            LostMetadata {
                class: LostMetadataClass::CreationDates,
                item_count: 2
            },
            LostMetadata {
                class: LostMetadataClass::ExtendedAttributes,
                item_count: 2
            },
            LostMetadata {
                class: LostMetadataClass::FolderDates,
                item_count: 2
            },
        ]
    );
}
