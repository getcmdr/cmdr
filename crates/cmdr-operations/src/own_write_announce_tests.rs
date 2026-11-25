//! Own writes are announced before they exist, on every write path: each test
//! drives one path end to end on an engine of its own and pins the exact
//! sequence of announces it made through its `OwnWrites` seam
//! (`host::testing::RecordingOwnWrites`), with whether each path existed at
//! that moment.
//!
//! A creation announces a path that doesn't exist yet; a removal or a rename's
//! source announces one that still does. The whole sequence is pinned, so an
//! announce moved after its syscall goes red on order or count, not only on the
//! existence bit, and a path that stopped announcing fails on a missing entry.

use cmdr_fs::app_path;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::archive_edit::ArchiveEditRequest;
use super::host::testing::{FakeVolumes, RecordingEngineEvents, RecordingOwnWrites};
use super::manager::OperationSummaryText;
use super::state::WriteOperationState;
use super::transfer::move_op::test_support::run_cross_fs_move_on;
use super::types::{ConflictResolution, VolumeCopyConfig, WriteOperationConfig};
use super::{
    BulkRenameRow, CollectorEventSink, OperationEventSink, Operations, SourceFingerprint, copy_volumes_with_progress,
    move_within_same_volume_with_progress,
};
use cmdr_archive::mutator::{AddEntry, AddSource, Changeset};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::testing::{TestDir, wait_until_async};
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_operation_log::types::Initiator;

const WAIT: Duration = Duration::from_secs(5);

/// A root with `src/` and `dst/`, and an engine of the test's own that records
/// every announce.
struct Rig {
    dir: TestDir,
    engine: Operations,
    volumes: Arc<FakeVolumes>,
    writes: Arc<RecordingOwnWrites>,
}

impl Rig {
    fn new(label: &str) -> Self {
        let dir = TestDir::new(label);
        fs::create_dir(dir.join("src")).expect("src");
        fs::create_dir(dir.join("dst")).expect("dst");
        let volumes = Arc::new(FakeVolumes::default());
        let writes = Arc::new(RecordingOwnWrites::default());
        let engine = Operations::builder(Arc::clone(&volumes) as _, Arc::new(RecordingEngineEvents::default()))
            .own_writes(Arc::clone(&writes) as _)
            .build();
        Self {
            dir,
            engine,
            volumes,
            writes,
        }
    }

    fn join(&self, path: &str) -> PathBuf {
        self.dir.join(path)
    }

    /// A fresh state on this rig's engine, for a test that drives an engine
    /// body directly.
    fn state(&self) -> Arc<WriteOperationState> {
        Arc::new(self.engine.new_operation_state(Duration::from_millis(200)))
    }

    /// Registers a real local-FS `root` volume, so the instant ops with no
    /// volume id take the timed `Volume` path production takes.
    fn with_root_volume(self) -> Self {
        self.volumes.register(
            "root",
            Arc::new(cmdr_local::LocalVolume::local_folder(
                "Test root",
                "/",
                crate::host::testing::pane_order_host(),
            )),
        );
        self
    }

    /// The recorded sequence as `(path relative to the root, existed)`, with
    /// Cmdr's random scratch suffixes (`a.txt.cmdr-tmp-<uuid>`,
    /// `.cmdr-staging-<op>`) folded to their kind, so the pin reads as the shape
    /// it is.
    fn shape(&self) -> Vec<(String, bool)> {
        self.writes
            .announced_under(&self.dir)
            .into_iter()
            .map(|(path, existed)| (fold_scratch_names(&path), existed))
            .collect()
    }
}

fn fold_scratch_names(path: &Path) -> String {
    path.components()
        .map(|c| {
            let name = c.as_os_str().to_string_lossy();
            let folded = name.find(".cmdr-").and_then(|at| {
                let (kind, _) = name[at + ".cmdr-".len()..].split_once('-')?;
                Some(format!("{}.cmdr-{kind}-*", &name[..at]))
            });
            folded.unwrap_or_else(|| name.into_owned())
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn announced(entries: &[(&str, bool)]) -> Vec<(String, bool)> {
    entries.iter().map(|(p, e)| ((*p).to_string(), *e)).collect()
}

async fn wait_settled(events: &CollectorEventSink) {
    wait_until_async(WAIT, "the operation to settle", || {
        !events.settled.lock_ignore_poison().is_empty()
    })
    .await;
    assert!(
        events.errors.lock_ignore_poison().is_empty(),
        "the operation must succeed: {:?}",
        events.errors.lock_ignore_poison()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_local_copy_announces_its_destination_and_temp_before_landing() {
    let rig = Rig::new("announce_copy");
    fs::write(rig.join("src/a.txt"), b"a").expect("source");

    let events = Arc::new(CollectorEventSink::new());
    rig.engine
        .copy_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![rig.join("src/a.txt")],
            rig.join("dst"),
            WriteOperationConfig::default(),
            vec![],
            None,
            Initiator::User,
            None,
            None,
        )
        .await
        .expect("start copy");
    wait_settled(&events).await;

    assert!(rig.join("dst/a.txt").exists());
    // The copy loop announces the destination, then `stage_and_land_file`
    // announces its temp and the destination again before the landing rename.
    assert_eq!(
        rig.shape(),
        announced(&[
            ("dst/a.txt", false),
            ("dst/a.txt.cmdr-tmp-*", false),
            ("dst/a.txt", false),
        ])
    );
}

/// Copies `src/a.txt` onto an existing `dst/a.txt` under `resolution`.
async fn copy_onto_existing(rig: &Rig, resolution: ConflictResolution) {
    fs::write(rig.join("src/a.txt"), b"new").expect("source");
    fs::write(rig.join("dst/a.txt"), b"old").expect("existing");

    let events = Arc::new(CollectorEventSink::new());
    rig.engine
        .copy_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![rig.join("src/a.txt")],
            rig.join("dst"),
            WriteOperationConfig {
                conflict_resolution: resolution,
                ..WriteOperationConfig::default()
            },
            vec![],
            None,
            Initiator::User,
            None,
            None,
        )
        .await
        .expect("start copy");
    wait_settled(&events).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_kept_as_a_numbered_name_announces_its_placeholder_before_creating_it() {
    let rig = Rig::new("announce_copy_rename");
    copy_onto_existing(&rig, ConflictResolution::Rename).await;

    assert_eq!(fs::read(rig.join("dst/a (1).txt")).expect("kept copy"), b"new");
    // `find_unique_name` reserves the numbered name with a 0-byte placeholder,
    // so that name is announced before the placeholder exists; the landing then
    // announces it again (now taken by the placeholder) like any overwrite.
    assert_eq!(
        rig.shape(),
        announced(&[
            ("dst/a (1).txt", false),
            ("dst/a (1).txt", true),
            ("dst/a (1).txt.cmdr-tmp-*", false),
            ("dst/a (1).txt", true),
            ("dst/a (1).txt.cmdr-temp-*", false),
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_overwriting_copy_announces_its_aside_before_moving_the_original_there() {
    let rig = Rig::new("announce_copy_overwrite");
    copy_onto_existing(&rig, ConflictResolution::Overwrite).await;

    assert_eq!(fs::read(rig.join("dst/a.txt")).expect("replaced"), b"new");
    // The original steps aside under a scratch name before the landing rename,
    // so that name is announced before the original is renamed onto it.
    assert_eq!(
        rig.shape(),
        announced(&[
            ("dst/a.txt", true),
            ("dst/a.txt.cmdr-tmp-*", false),
            ("dst/a.txt", true),
            ("dst/a.txt.cmdr-temp-*", false),
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_same_fs_move_announces_both_ends_before_the_rename() {
    let rig = Rig::new("announce_same_fs_move");
    fs::write(rig.join("src/a.txt"), b"a").expect("source");

    let events = Arc::new(CollectorEventSink::new());
    rig.engine
        .move_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![crate::testing::app_path_of(&rig.join("src/a.txt"))],
            crate::testing::app_path_of(&rig.join("dst")),
            WriteOperationConfig::default(),
            vec![],
            None,
            Initiator::User,
            None,
            None,
        )
        .await
        .expect("start move");
    wait_settled(&events).await;

    assert!(rig.join("dst/a.txt").exists());
    assert_eq!(rig.shape(), announced(&[("src/a.txt", true), ("dst/a.txt", false)]));
}

#[test]
fn a_cross_fs_move_announces_its_final_path_before_the_staged_rename() {
    let rig = Rig::new("announce_cross_fs_move");
    fs::write(rig.join("src/a.txt"), b"a").expect("source");

    run_cross_fs_move_on(
        &rig.state(),
        &[rig.join("src/a.txt")],
        &rig.join("dst"),
        ConflictResolution::Stop,
        "announce-cross-fs",
    )
    .expect("cross-fs move");

    assert!(rig.join("dst/a.txt").exists());
    // The copy phase lands in the staging folder like any copy, then the final
    // name is announced before the staged rename. The source delete announces
    // nothing.
    assert_eq!(
        rig.shape(),
        announced(&[
            ("dst/.cmdr-staging-*/a.txt", false),
            ("dst/.cmdr-staging-*/a.txt.cmdr-tmp-*", false),
            ("dst/.cmdr-staging-*/a.txt", false),
            ("dst/a.txt", false),
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_announces_each_file_then_its_folder_while_they_still_exist() {
    let rig = Rig::new("announce_delete");
    fs::create_dir(rig.join("src/folder")).expect("folder");
    fs::write(rig.join("src/folder/a.txt"), b"a").expect("file");

    let events = Arc::new(CollectorEventSink::new());
    rig.engine
        .delete_files_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            vec![crate::testing::app_path_of(&rig.join("src/folder"))],
            WriteOperationConfig::default(),
            None,
            Initiator::User,
            None,
        )
        .await
        .expect("start delete");
    wait_settled(&events).await;

    assert!(!rig.join("src/folder").exists());
    assert_eq!(
        rig.shape(),
        announced(&[("src/folder/a.txt", true), ("src/folder", true)])
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_trash_announces_each_item_while_it_still_exists() {
    let rig = Rig::new("announce_trash");
    fs::write(rig.join("src/a.txt"), b"a").expect("file");
    let _trash = crate::testing::TrashCleanup::of(&[rig.join("src/a.txt")]);

    let events = CollectorEventSink::new();
    super::delete::trash::trash_files_with_progress(
        &events,
        "announce-trash",
        &rig.state(),
        &[rig.join("src/a.txt")],
        None,
    )
    .expect("trash");

    assert!(!rig.join("src/a.txt").exists());
    assert_eq!(rig.shape(), announced(&[("src/a.txt", true)]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_announces_both_ends_before_the_syscall() {
    let rig = Rig::new("announce_rename");
    fs::write(rig.join("src/a.txt"), b"a").expect("file");

    rig.engine
        .rename_managed(
            crate::testing::app_path_of(&rig.join("src/a.txt")),
            crate::testing::rename_target(&crate::testing::app_path_of(&rig.join("src/b.txt"))),
            false,
            "root".to_string(),
            Initiator::User,
        )
        .await
        .expect("rename");

    assert!(rig.join("src/b.txt").exists());
    assert_eq!(rig.shape(), announced(&[("src/a.txt", true), ("src/b.txt", false)]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_folder_and_a_new_file_are_announced_before_they_exist() {
    let rig = Rig::new("announce_create").with_root_volume();
    let parent = rig.join("dst").to_string_lossy().into_owned();

    rig.engine
        .create_directory_managed(None, parent.clone(), "new folder".to_string(), Initiator::User)
        .await
        .expect("mkdir");
    rig.engine
        .create_file_managed(None, parent, "new.txt".to_string(), Initiator::User)
        .await
        .expect("mkfile");

    assert!(rig.join("dst/new folder").is_dir());
    assert!(rig.join("dst/new.txt").is_file());
    assert_eq!(
        rig.shape(),
        announced(&[("dst/new folder", false), ("dst/new.txt", false)])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bulk_rename_announces_both_ends_of_every_hop() {
    let rig = Rig::new("announce_bulk_rename");
    let row = |id: &str, from: &str, to: &str| {
        let source = rig.join("src").join(from);
        fs::write(&source, id.as_bytes()).expect("source");
        BulkRenameRow {
            row_id: id.to_string(),
            expected_fingerprint: SourceFingerprint::capture_local(&source).expect("fingerprint"),
            source,
            destination: rig.join("src").join(to),
        }
    };
    let rows = vec![row("1", "a.txt", "x.txt"), row("2", "b.txt", "y.txt")];

    let events = Arc::new(CollectorEventSink::new());
    rig.engine
        .start_bulk_rename(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            "root".to_string(),
            rows,
            Initiator::User,
        )
        .expect("start bulk rename");
    wait_settled(&events).await;

    assert!(rig.join("src/x.txt").exists() && rig.join("src/y.txt").exists());
    assert_eq!(
        rig.shape(),
        announced(&[
            ("src/a.txt", true),
            ("src/x.txt", false),
            ("src/b.txt", true),
            ("src/y.txt", false),
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_archive_edit_announces_its_temp_then_the_archive_it_replaces() {
    let rig = Rig::new("announce_archive_edit");
    let archive = rig.join("dst/a.zip");
    {
        use std::io::Write as _;
        let mut writer = zip::ZipWriter::new(fs::File::create(&archive).expect("zip"));
        writer
            .start_file("keep.txt", zip::write::SimpleFileOptions::default())
            .expect("entry");
        writer.write_all(b"keep").expect("bytes");
        writer.finish().expect("finish");
    }

    let events = Arc::new(CollectorEventSink::new());
    let request = ArchiveEditRequest {
        archive_path: archive.clone(),
        // A unique parent keeps the op off every shared lane.
        parent_volume_id: format!("announce-archive-{}", uuid::Uuid::new_v4()),
        changeset: Changeset {
            adds: vec![AddEntry {
                inner_path: "added.txt".to_string(),
                source: AddSource::Bytes(b"new".to_vec()),
            }],
            ..Default::default()
        },
        summary: OperationSummaryText::default(),
        skipped_count: 0,
        preview_id: None,
    };
    rig.engine
        .archive_edit_start(Arc::clone(&events) as Arc<dyn OperationEventSink>, request, 0)
        .await
        .expect("start archive edit");
    wait_settled(&events).await;

    // The mutator builds a sibling temp, then announces the archive it is about
    // to replace before the swap rename.
    assert_eq!(
        rig.shape(),
        announced(&[("dst/a.zip.cmdr-tmp-*", false), ("dst/a.zip", true)])
    );
}

/// A fresh compress into a local folder (`StagedWrite::begin_generated`): the
/// zip it publishes and the temp it streams into, so compressing into
/// `~/Downloads` never toasts Cmdr's own output.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fresh_compress_announces_its_zip_and_temp_before_they_exist() {
    let rig = Rig::new("announce_fresh_compress");
    fs::write(rig.join("src/a.txt"), b"a").expect("source");
    let source: Arc<dyn Volume> = Arc::new(cmdr_local::LocalVolume::local_folder(
        "Source",
        rig.join("src"),
        crate::host::testing::pane_order_host(),
    ));

    let events = Arc::new(CollectorEventSink::new());
    rig.engine
        .compress_start(
            Arc::clone(&events) as Arc<dyn OperationEventSink>,
            source,
            vec![PathBuf::from("a.txt")],
            rig.join("dst/a.zip"),
            // A unique parent keeps the op off every shared lane.
            format!("announce-compress-{}", uuid::Uuid::new_v4()),
            ConflictResolution::Stop,
            0,
            None,
            None,
            Initiator::User,
        )
        .await
        .expect("start compress");
    wait_settled(&events).await;

    assert!(rig.join("dst/a.zip").exists());
    // The final name, then the temp the zip streams into; the landing rename
    // carries no third announce.
    assert_eq!(
        rig.shape(),
        announced(&[("dst/a.zip", false), ("dst/a.zip.cmdr-tmp-*", false)])
    );
}

/// The volume engine landing on a local disk (`StagedWrite::begin` over `EngineHost::announce_on`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_volume_copy_onto_a_local_disk_announces_its_destination_and_temp() {
    let rig = Rig::new("announce_volume_copy");
    let source: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    source.create_file(app_path!("/a.txt"), b"a").await.expect("source");
    let dest: Arc<dyn Volume> = Arc::new(cmdr_local::LocalVolume::local_folder(
        "Dest",
        rig.join("dst"),
        crate::host::testing::pane_order_host(),
    ));

    copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        "announce-volume-copy",
        &rig.state(),
        source,
        &crate::testing::app_paths_of(&[PathBuf::from("/a.txt")]),
        dest,
        &crate::testing::app_path_of(Path::new("/")),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("volume copy");

    assert!(rig.join("dst/a.txt").exists());
    // The final name, then the temp the bytes stream into; the landing rename
    // carries no third announce.
    assert_eq!(
        rig.shape(),
        announced(&[("dst/a.txt", false), ("dst/a.txt.cmdr-tmp-*", false)])
    );
}

/// The volume engine's same-volume move on a local disk
/// (`EngineHost::announce_on`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_same_volume_move_on_a_local_disk_announces_both_ends() {
    let rig = Rig::new("announce_volume_move");
    fs::write(rig.join("src/a.txt"), b"a").expect("source");
    let volume: Arc<dyn Volume> = Arc::new(cmdr_local::LocalVolume::local_folder(
        "Local",
        rig.dir.to_path_buf(),
        crate::host::testing::pane_order_host(),
    ));

    move_within_same_volume_with_progress(
        Arc::new(CollectorEventSink::new()),
        "announce-volume-move",
        &rig.state(),
        volume,
        &[PathBuf::from("/src/a.txt")],
        Path::new("/dst"),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("same-volume move");

    assert!(rig.join("dst/a.txt").exists());
    assert_eq!(rig.shape(), announced(&[("src/a.txt", true), ("dst/a.txt", false)]));
}
