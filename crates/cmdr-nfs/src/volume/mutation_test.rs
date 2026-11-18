//! Names on the export against `fake_nfs.rs`: the claim before a rename, a
//! delete that never recurses, folders made one and many at a time, metadata
//! put on a folder, and the parent chain proven before every mutation.

use cmdr_fs::app_path::AppPath;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use cmdr_fs::metadata::{CarriedMetadata, ClassOutcome, MetadataClasses};
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{DirectoryCreation, Volume, VolumeError};
use tokio_util::sync::CancellationToken;

use super::super::fake_nfs::{FakeNfs, proc};
use super::super::{NfsVolume, connect_with};
use crate::dial::DialOptions;

/// `nfsstat3` numbers a refusal knob answers with.
const NFS3ERR_IO: u32 = 5;

async fn over_pipe(fake: &FakeNfs) -> NfsVolume {
    connect_with(
        "fake",
        fake.params(),
        VolumeHost::builder().runtime(tokio::runtime::Handle::current()).build(),
        fake.duplex_dialer(DialOptions::PRODUCTION),
        CancellationToken::new(),
    )
    .await
    .expect("the fake mounts")
}

fn at(volume: &NfsVolume, remote: &str) -> PathBuf {
    PathBuf::from(format!("{}{remote}", volume.root().as_std_legacy().display()))
}

#[tokio::test]
async fn a_forceless_rename_refuses_a_taken_name_and_touches_neither() {
    let fake = FakeNfs::new();
    fake.add_file("/source.txt", b"source");
    fake.add_file("/target.txt", b"the user's target file");
    let volume = over_pipe(&fake).await;

    let renamed = volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/source.txt")),
            &AppPath::legacy_unchecked(&at(&volume, "/target.txt")),
            false,
        )
        .await;

    assert!(matches!(renamed, Err(VolumeError::AlreadyExists(_))), "{renamed:?}");
    assert_eq!(fake.contents("/source.txt").unwrap(), b"source");
    assert_eq!(fake.contents("/target.txt").unwrap(), b"the user's target file");
    assert_eq!(
        fake.calls(proc::RENAME),
        0,
        "❌ v3 RENAME replaces, so it's never sent onto a taken name"
    );
}

#[tokio::test]
async fn a_forceless_rename_claims_the_name_then_lands_onto_the_claim() {
    let fake = FakeNfs::new();
    fake.add_dir("/a");
    fake.add_dir("/b");
    fake.add_file("/a/photo.jpg", b"pixels");
    let volume = over_pipe(&fake).await;

    volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/a/photo.jpg")),
            &AppPath::legacy_unchecked(&at(&volume, "/b/photo.jpg")),
            false,
        )
        .await
        .unwrap();

    assert!(!fake.exists("/a/photo.jpg"));
    assert_eq!(fake.contents("/b/photo.jpg").unwrap(), b"pixels");
    assert_eq!(
        fake.last_create().map(|create| create.how),
        Some(1),
        "the claim is GUARDED"
    );
}

#[tokio::test]
async fn a_forceless_folder_rename_claims_with_mkdir() {
    let fake = FakeNfs::new();
    fake.add_dir("/album");
    fake.add_file("/album/one.jpg", b"1");
    let volume = over_pipe(&fake).await;

    volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/album")),
            &AppPath::legacy_unchecked(&at(&volume, "/renamed")),
            false,
        )
        .await
        .unwrap();

    assert_eq!(fake.contents("/renamed/one.jpg").unwrap(), b"1");
    assert!(!fake.exists("/album"));
    assert_eq!(fake.calls(proc::MKDIR), 1);
    // The cached `/album` handle now names `/renamed`, so it must be gone.
    assert!(matches!(
        volume
            .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/album")), None)
            .await,
        Err(VolumeError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_rename_that_fails_after_the_claim_takes_the_claim_away() {
    let fake = FakeNfs::new();
    fake.add_file("/source.txt", b"source");
    let volume = over_pipe(&fake).await;
    fake.refuse_next(proc::RENAME, NFS3ERR_IO);

    let renamed = volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/source.txt")),
            &AppPath::legacy_unchecked(&at(&volume, "/target.txt")),
            false,
        )
        .await;

    assert!(renamed.is_err());
    assert!(
        !fake.exists("/target.txt"),
        "❌ never a 0-byte claim left at the user's name"
    );
    assert_eq!(fake.contents("/source.txt").unwrap(), b"source");
}

#[tokio::test]
async fn a_rename_onto_itself_does_nothing_and_claims_nothing() {
    let fake = FakeNfs::new();
    fake.add_file("/same.txt", b"same");
    let volume = over_pipe(&fake).await;

    volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/same.txt")),
            &AppPath::legacy_unchecked(&at(&volume, "/same.txt")),
            false,
        )
        .await
        .unwrap();

    assert_eq!(fake.contents("/same.txt").unwrap(), b"same");
    assert_eq!(fake.calls(proc::CREATE), 0);
}

#[tokio::test]
async fn a_forced_rename_replaces_in_one_call() {
    let fake = FakeNfs::new();
    fake.add_file("/source.txt", b"source");
    fake.add_file("/target.txt", b"old target");
    let volume = over_pipe(&fake).await;

    volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/source.txt")),
            &AppPath::legacy_unchecked(&at(&volume, "/target.txt")),
            true,
        )
        .await
        .unwrap();

    assert_eq!(fake.contents("/target.txt").unwrap(), b"source");
    assert_eq!((fake.calls(proc::RENAME), fake.calls(proc::CREATE)), (1, 0));
}

#[tokio::test]
async fn a_forced_rename_onto_a_folder_with_something_in_it_is_refused() {
    let fake = FakeNfs::new();
    fake.add_dir("/source-dir");
    fake.add_dir("/album");
    fake.add_file("/album/keep.jpg", b"keep");
    let volume = over_pipe(&fake).await;

    let renamed = volume
        .rename(
            &AppPath::legacy_unchecked(&at(&volume, "/source-dir")),
            &AppPath::legacy_unchecked(&at(&volume, "/album")),
            true,
        )
        .await;

    assert!(renamed.is_err());
    assert_eq!(fake.contents("/album/keep.jpg").unwrap(), b"keep");
}

#[tokio::test]
async fn delete_removes_a_file_an_empty_folder_and_a_link_but_never_recurses() {
    let fake = FakeNfs::new();
    fake.add_file("/file.txt", b"x");
    fake.add_dir("/empty");
    fake.add_dir("/full");
    fake.add_file("/full/keep.txt", b"keep");
    fake.add_link("/link", "full");
    let volume = over_pipe(&fake).await;

    volume
        .delete(&AppPath::legacy_unchecked(&at(&volume, "/file.txt")))
        .await
        .unwrap();
    volume
        .delete(&AppPath::legacy_unchecked(&at(&volume, "/empty")))
        .await
        .unwrap();
    volume
        .delete(&AppPath::legacy_unchecked(&at(&volume, "/link")))
        .await
        .unwrap();
    let refused = volume.delete(&AppPath::legacy_unchecked(&at(&volume, "/full"))).await;

    assert!(!fake.exists("/file.txt") && !fake.exists("/empty") && !fake.exists("/link"));
    assert!(
        matches!(
            refused,
            Err(VolumeError::IoError {
                raw_os_error: Some(libc::ENOTEMPTY),
                ..
            })
        ),
        "{refused:?}"
    );
    assert_eq!(
        fake.contents("/full/keep.txt").unwrap(),
        b"keep",
        "the link went, ❌ never its target"
    );
}

#[tokio::test]
async fn a_mutation_acts_on_the_folder_the_path_names_now() {
    // ❗ Another client renames `/a/b` to `/z/b` and makes a new `/a/b`. Our
    // cached `/a/b` handle still works and now names `/z/b`: a delete through
    // it would remove the wrong file.
    let fake = FakeNfs::new();
    fake.add_dir("/a");
    fake.add_dir("/a/b");
    fake.add_file("/a/b/f.txt", b"old");
    fake.add_dir("/z");
    let volume = over_pipe(&fake).await;
    volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/a/b")), None)
        .await
        .unwrap();
    fake.rename("/a/b", "/z/b");
    fake.add_dir("/a/b");
    fake.add_file("/a/b/f.txt", b"new");

    volume
        .delete(&AppPath::legacy_unchecked(&at(&volume, "/a/b/f.txt")))
        .await
        .unwrap();

    assert!(!fake.exists("/a/b/f.txt"));
    assert_eq!(
        fake.contents("/z/b/f.txt").unwrap(),
        b"old",
        "the moved folder is untouched"
    );
}

#[tokio::test]
async fn create_directory_refuses_a_taken_name_and_mkdir_all_is_honest() {
    let fake = FakeNfs::new();
    fake.add_dir("/album");
    fake.add_file("/notes", b"in the way");
    let volume = over_pipe(&fake).await;

    let again = volume
        .create_directory(&AppPath::legacy_unchecked(&at(&volume, "/album")))
        .await;
    let made = volume
        .create_directory_all(&AppPath::legacy_unchecked(&at(&volume, "/deep/er/still")))
        .await
        .unwrap();
    let found = volume
        .create_directory_all(&AppPath::legacy_unchecked(&at(&volume, "/album")))
        .await
        .unwrap();
    let blocked = volume
        .create_directory_all(&AppPath::legacy_unchecked(&at(&volume, "/notes/inside/deeper")))
        .await;

    assert!(matches!(again, Err(VolumeError::AlreadyExists(_))), "{again:?}");
    assert_eq!(made, DirectoryCreation::Created);
    assert!(fake.exists("/deep/er/still"));
    assert_eq!(found, DirectoryCreation::AlreadyExisted);
    assert!(
        matches!(&blocked, Err(VolumeError::NotADirectory(path)) if path.ends_with("/notes")),
        "the file in the way is named: {blocked:?}"
    );
}

#[tokio::test]
async fn apply_metadata_dates_and_modes_a_folder() {
    let fake = FakeNfs::new();
    fake.add_dir("/born");
    fake.add_file("/born/inside.txt", b"x");
    let volume = over_pipe(&fake).await;
    let carried = CarriedMetadata {
        created: Some(UNIX_EPOCH + Duration::from_secs(1_425_168_000)),
        mode: Some(0o700),
        ..CarriedMetadata::default()
    };

    let report = volume
        .apply_metadata(
            &AppPath::legacy_unchecked(&at(&volume, "/born")),
            &carried,
            Some(UNIX_EPOCH + Duration::from_secs(1_611_909_015)),
        )
        .await;

    assert_eq!((report.modified, report.mode), (ClassOutcome::Kept, ClassOutcome::Kept));
    assert_eq!(report.created, ClassOutcome::NotApplicable, "v3 has no creation date");
    let (mode, _, mtime) = fake.mode_and_times("/born").unwrap();
    assert_eq!((mode & 0o777, mtime.seconds), (0o700, 1_611_909_015));
}

#[tokio::test]
async fn the_caps_keep_mode_and_folder_dates_and_drop_creation_dates_and_xattrs() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let caps = volume.metadata_caps();

    assert!(caps.stores.contains(MetadataClasses::MODE) && caps.reports.contains(MetadataClasses::MODE));
    assert!(caps.folder_dates && caps.folder_stores.contains(MetadataClasses::MODE));
    for dropped in [MetadataClasses::CREATED, MetadataClasses::XATTRS] {
        assert!(!caps.stores.contains(dropped), "the copy's loss note names it");
    }
}
