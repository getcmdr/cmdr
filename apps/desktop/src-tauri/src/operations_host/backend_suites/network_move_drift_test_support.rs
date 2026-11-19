//! A folder moved OFF a live server while someone keeps working in it (#139).
//!
//! The move copies the folder, then sweeps the source. Anything saved over or
//! added on the server in between exists only there, so the sweep must leave it
//! and the completion must say so. The in-memory proof is
//! `transfer/volume/move_source_drift_tests.rs`; these run the same window
//! against a real server's listings and clocks.
//!
//! The window is reproduced by a local destination that edits the server the
//! moment the first file takes its final name: that copy is done, and the sweep
//! hasn't started.

use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use cmdr_fs::volume::Volume;

use super::network_semantics_test_support::{Transfer, local_volume, seed, transfer, try_read};
use super::network_transfer_test_support::clean_deep;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::VolumeError;
use cmdr_operations::ConflictResolution;
use cmdr_operations::testing::{AppearedDuringMove, forward_volume_methods};

type Edit = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

/// A destination that runs `edit` once, right after the first file lands.
struct EditSourceAtFirstLanding {
    inner: Arc<dyn Volume>,
    edit: Mutex<Option<Edit>>,
}

impl Volume for EditSourceAtFirstLanding {
    forward_volume_methods!(
        inner => name,
        root,
        lane_key,
        list_directory,
        get_metadata,
        exists,
        is_directory,
        create_file,
        create_directory,
        create_directory_all,
        delete,
        get_space_info,
        local_path,
        supports_streaming,
        supports_export,
        supports_local_fs_access,
        operations_are_local,
        max_concurrent_ops,
        create_directory_errors_on_existing_dir,
        scan_for_copy,
        scan_for_copy_batch,
        scan_for_conflicts,
        open_read_stream,
        write_from_stream,
        write_is_single_shot,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn rename<'a>(
        &'a self,
        from: &'a AppPath,
        to: &'a AppPath,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            self.inner.rename(from, to, force).await?;
            let edit = self.edit.lock_ignore_poison().take();
            if let Some(edit) = edit {
                edit().await;
            }
            Ok(())
        })
    }
}

/// The folder being moved: three files, so the edit lands while others are
/// still to come or already done.
const WORK: [(&str, &[u8]); 3] = [("a.txt", b"alpha"), ("b.txt", b"bravo"), ("c.txt", b"charlie")];

/// Moves `dir/Work` off the server while `edit` runs on it, and returns what the
/// completion reported.
async fn move_work_off_the_server(label: &str, remote: &Arc<dyn Volume>, dir: &Path, edit: Edit) -> AppearedDuringMove {
    seed(remote.as_ref(), &dir.join("Work"), &WORK).await;
    let (_local_dir, local) = local_volume(label);
    let dest = Arc::new(EditSourceAtFirstLanding {
        inner: local,
        edit: Mutex::new(Some(edit)),
    });
    let dest_dyn: Arc<dyn Volume> = Arc::clone(&dest) as Arc<dyn Volume>;

    let finished = transfer(
        label,
        Transfer::Move,
        remote,
        &[dir.join("Work")],
        &dest_dyn,
        Path::new(""),
        ConflictResolution::Stop,
    )
    .await;

    assert!(
        dest.edit.lock_ignore_poison().is_none(),
        "{label}: precondition: the edit ran before the sweep"
    );
    let complete = finished.events.complete.lock_ignore_poison();
    complete[0]
        .appeared_during_move
        .clone()
        .unwrap_or_else(|| panic!("{label}: the move kept something, so it must say so"))
}

/// A file saved over on the server after its copy stays, with its NEW bytes;
/// the untouched originals still go.
pub(super) async fn a_file_saved_over_mid_move_off_the_server_stays(remote: Arc<dyn Volume>, dir: PathBuf) {
    let saved = dir.join("Work/b.txt");
    let server = Arc::clone(&remote);
    let path = saved.clone();
    // Longer than the original, so the size gives it away even inside one
    // second of the server's mtime clock.
    let edit: Edit = Box::new(move || {
        Box::pin(async move {
            server
                .delete(&AppPath::legacy_unchecked(&path))
                .await
                .expect("removing the old save");
            server
                .create_file(&AppPath::legacy_unchecked(&path), b"bravo, saved again")
                .await
                .expect("the new save");
        })
    });

    let left = move_work_off_the_server("drift-saved-over", &remote, &dir, edit).await;

    assert_eq!(
        try_read(remote.as_ref(), &saved).await.as_deref(),
        Some(&b"bravo, saved again"[..])
    );
    assert!(
        !remote.exists(&AppPath::legacy_unchecked(&dir.join("Work/a.txt"))).await,
        "an untouched original goes"
    );
    assert!(
        !remote.exists(&AppPath::legacy_unchecked(&dir.join("Work/c.txt"))).await,
        "an untouched original goes"
    );
    assert_eq!((left.changed_count, left.item_count), (1, 0));
    assert_eq!(left.folder_name, "Work");

    clean_deep(remote.as_ref(), &dir).await;
}

/// A file that turned up in the folder mid-move stays, and so does its folder.
pub(super) async fn a_file_added_mid_move_off_the_server_stays(remote: Arc<dyn Volume>, dir: PathBuf) {
    let added = dir.join("Work/download.zip");
    let server = Arc::clone(&remote);
    let path = added.clone();
    let edit: Edit = Box::new(move || {
        Box::pin(async move {
            server
                .create_file(&AppPath::legacy_unchecked(&path), b"arrived mid-move")
                .await
                .expect("the newcomer");
        })
    });

    let left = move_work_off_the_server("drift-added", &remote, &dir, edit).await;

    assert_eq!(
        try_read(remote.as_ref(), &added).await.as_deref(),
        Some(&b"arrived mid-move"[..])
    );
    for (name, _) in WORK {
        assert!(
            !remote
                .exists(&AppPath::legacy_unchecked(&dir.join("Work").join(name)))
                .await,
            "{name} was carried, so it goes"
        );
    }
    assert_eq!((left.item_count, left.changed_count), (1, 0));
    assert_eq!(left.folder_name, "Work");

    clean_deep(remote.as_ref(), &dir).await;
}
