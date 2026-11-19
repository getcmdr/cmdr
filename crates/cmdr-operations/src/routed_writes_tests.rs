//! Every entry point that acts on the drive itself refuses a path a route
//! serves (inside a `.zip`, or inside a repo's `.git/<category>/` trees), and
//! leaves the drive untouched (#463).

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::{InMemoryVolume, RoutedKind, Volume, VolumeError};
use cmdr_operation_log::types::Initiator;

use crate::event_sinks::CollectorEventSink;
use crate::mutation_error::MutationError;
use crate::rename::{BulkRenameRow, RenameStartError};
use crate::testing::TestOperations;
use crate::types::{ReadOnlySide, VolumeCopyConfig, WriteOperationConfig, WriteOperationError};

const BRANCHES: &str = "/repo/.git/branches";
const ZIP: &str = "/repo/bundle.zip";

/// An engine over one in-memory drive holding a real `.git/branches/main/a.txt`
/// and `bundle.zip/in.txt`, both served by read-only routes, as the app's
/// portal and archive routes serve them.
async fn drive_with_routes(label: &str) -> (TestOperations, String, Arc<InMemoryVolume>) {
    let engine = TestOperations::new();
    let volume_id = format!("{label}-{}", std::process::id());
    let drive = Arc::new(InMemoryVolume::new(&volume_id));
    for dir in ["/repo", "/repo/.git", BRANCHES, "/repo/.git/branches/main", ZIP] {
        drive
            .create_directory(&AppPath::legacy_unchecked(Path::new(dir)))
            .await
            .unwrap();
    }
    drive
        .create_file(app_path!("/repo/.git/branches/main/a.txt"), b"a")
        .await
        .unwrap();
    drive
        .create_file(app_path!("/repo/bundle.zip/in.txt"), b"i")
        .await
        .unwrap();
    engine
        .volumes()
        .register(&volume_id, Arc::clone(&drive) as Arc<dyn Volume>);
    let portal: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("main"));
    engine.volumes().route(BRANCHES, RoutedKind::GitPortal, portal);
    let archive: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("bundle.zip"));
    engine.volumes().route(ZIP, RoutedKind::Archive, archive);
    (engine, volume_id, drive)
}

async fn row(drive: &InMemoryVolume, from: &str, to: &str) -> BulkRenameRow {
    BulkRenameRow {
        row_id: "1".to_string(),
        source: PathBuf::from(from),
        destination: PathBuf::from(to),
        expected_fingerprint: crate::SourceFingerprint::capture_remote(drive, Path::new(from))
            .await
            .expect("a fingerprint"),
    }
}

fn is_read_only(error: &WriteOperationError, side: ReadOnlySide) -> bool {
    matches!(error, WriteOperationError::ReadOnlyDevice { side: s, .. } if *s == side)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bulk_rename_under_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("bulk-git").await;
    let from = "/repo/.git/branches/main/a.txt";
    let rows = vec![row(&drive, from, "/repo/.git/branches/main/b.txt").await];

    let result = engine
        .start_renames(Arc::new(CollectorEventSink::new()), volume_id, rows, Initiator::Agent)
        .await;

    assert!(
        matches!(&result, Err(RenameStartError::Engine { error }) if is_read_only(error, ReadOnlySide::Source)),
        "{result:?}"
    );
    assert!(drive.exists(&AppPath::legacy_unchecked(Path::new(from))).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bulk_rename_inside_an_archive_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("bulk-zip").await;
    let rows = vec![row(&drive, "/repo/bundle.zip/in.txt", "/repo/bundle.zip/out.txt").await];

    let result = engine
        .start_renames(Arc::new(CollectorEventSink::new()), volume_id, rows, Initiator::Agent)
        .await;

    assert!(matches!(&result, Err(RenameStartError::Engine { .. })), "{result:?}");
    assert!(drive.exists(app_path!("/repo/bundle.zip/in.txt")).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_as_move_out_of_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("by-move-git").await;
    let from = PathBuf::from("/repo/.git/branches/main/a.txt");

    let result = engine
        .start_rename_by_move(
            Arc::new(CollectorEventSink::new()),
            volume_id,
            vec![(from.clone(), "b.txt".to_string())],
            "/repo/.git/branches/main".to_string(),
            VolumeCopyConfig::default(),
            Initiator::User,
            None,
        )
        .await;

    assert!(
        matches!(&result, Err(error) if is_read_only(error, ReadOnlySide::Source)),
        "{result:?}"
    );
    assert!(drive.exists(&AppPath::legacy_unchecked(&from)).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_as_move_into_an_archive_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("by-move-zip").await;
    drive.create_file(app_path!("/repo/top.txt"), b"t").await.unwrap();

    let result = engine
        .start_rename_by_move(
            Arc::new(CollectorEventSink::new()),
            volume_id,
            vec![(PathBuf::from("/repo/top.txt"), "top.txt".to_string())],
            ZIP.to_string(),
            VolumeCopyConfig::default(),
            Initiator::User,
            None,
        )
        .await;

    assert!(
        matches!(&result, Err(error) if is_read_only(error, ReadOnlySide::Destination)),
        "{result:?}"
    );
    assert!(drive.exists(app_path!("/repo/top.txt")).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_instant_rename_under_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("instant-git").await;
    let from = PathBuf::from("/repo/.git/branches/main/a.txt");

    let result = engine
        .rename_managed(
            from.clone(),
            PathBuf::from("/repo/.git/branches/main/b.txt"),
            false,
            volume_id,
            Initiator::User,
        )
        .await;

    assert!(
        matches!(
            &result,
            Err(MutationError::Volume {
                error: VolumeError::ReadOnly(_)
            })
        ),
        "{result:?}"
    );
    assert!(drive.exists(&AppPath::legacy_unchecked(&from)).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_folder_under_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("mkdir-git").await;

    let result = engine
        .create_directory_managed(
            Some(volume_id),
            "/repo/.git/branches/main".to_string(),
            "new".to_string(),
            Initiator::User,
        )
        .await;

    assert!(
        matches!(
            &result,
            Err(MutationError::Volume {
                error: VolumeError::ReadOnly(_)
            })
        ),
        "{result:?}"
    );
    assert!(!drive.exists(app_path!("/repo/.git/branches/main/new")).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_file_under_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("mkfile-git").await;

    let result = engine
        .create_file_managed(
            Some(volume_id),
            "/repo/.git/branches/main".to_string(),
            "new.txt".to_string(),
            Initiator::User,
        )
        .await;

    assert!(
        matches!(
            &result,
            Err(MutationError::Volume {
                error: VolumeError::ReadOnly(_)
            })
        ),
        "{result:?}"
    );
    assert!(!drive.exists(app_path!("/repo/.git/branches/main/new.txt")).await);
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_numbered_create_inside_an_archive_or_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("numbered").await;

    for dir in [ZIP, "/repo/.git/branches/main"] {
        let result = engine
            .create_numbered_file(
                Some(volume_id.clone()),
                Path::new(dir),
                "pasted",
                "txt",
                b"x",
                Initiator::User,
            )
            .await;

        assert!(
            matches!(
                &result,
                Err(MutationError::Volume {
                    error: VolumeError::ReadOnly(_)
                })
            ),
            "{dir}: {result:?}"
        );
        assert!(
            !drive
                .exists(&AppPath::legacy_unchecked(&Path::new(dir).join("pasted.txt")))
                .await,
            "{dir}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_under_dot_git_refuses() {
    let (engine, volume_id, drive) = drive_with_routes("delete-git").await;
    let doomed = PathBuf::from("/repo/.git/branches/main/a.txt");

    let result = engine
        .delete_files_start(
            Arc::new(CollectorEventSink::new()),
            vec![doomed.clone()],
            WriteOperationConfig::default(),
            Some(volume_id),
            Initiator::Agent,
            None,
        )
        .await;

    assert!(
        matches!(&result, Err(error) if is_read_only(error, ReadOnlySide::Source)),
        "{result:?}"
    );
    assert!(drive.exists(&AppPath::legacy_unchecked(&doomed)).await);
}
