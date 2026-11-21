//! What a file→file Overwrite does today at each of the five places a
//! safe-replace lands, so a restructure of that landing can prove it changed
//! nothing outside the case it meant to change.
//!
//! The grid is site × outcome. Sites: the serial driver (`copy_serial.rs`), the
//! concurrent driver (`copy_concurrent_task.rs`), a deep merge child
//! (`merge.rs`), a cross-volume move (`move_cross.rs`), and a one-pass extract
//! out of a compressed tar (`sequential_extract.rs`). Outcomes:
//!
//! - **lands**: the original holds the new bytes, the journal records the
//!   ORIGINAL's path (never a temp) and calls the op not rollbackable, Locked
//!   goes on at the original's name, and no `.cmdr-tmp-*` is left.
//! - **the write fails**: the original is untouched, no temp is left, and the
//!   op stays rollbackable.
//! - **the delete of the original is refused**: the original is untouched, the
//!   temp is taken away, the refusal arrives typed, and the op stays
//!   rollbackable.
//! - **the delete went through, then the rename was refused**: the new bytes
//!   are rescued to ` (recovered)`, the failure names that path
//!   (`NewDataKeptAt`), and the op isn't rollbackable (the original is gone).
//!
//! Every cell also asserts through `safety_oracle.rs`, with a dest-only sibling
//! beside the replaced file: a failed replace never takes a byte from either
//! side, and never touches what the source doesn't shadow.
//!
//! One more cell drops a concurrent task between `delete(orig)` and the rename
//! (`a_concurrent_task_dropped_mid_swap_keeps_its_temp_for_the_sweep_to_land`).
//!
//! Older pins of single cells stay where they are (`copy_crashsafe_tests.rs`,
//! `move_failure_tests.rs`, `swap_landing_tests.rs`, `copy_flags_tests.rs`,
//! `journal_capture_volume_tests.rs`); this grid is the one place all five sites
//! answer the same four questions.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use cmdr_archive::{ArchiveFormat, ArchiveVolume, TarCodec};
use cmdr_fs::ignore_poison::IgnorePoison as _;
use cmdr_fs::metadata::{CarriedMetadata, UserFlags};
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError, VolumeReadStream};
use cmdr_operation_log::store::{open_read_connection, read_operation};
use cmdr_operation_log::types::{EntryType, ExecutionStatus, Initiator, OpKind, RollbackState};
use tokio::sync::Notify;

use super::faulty_volume::{FaultyOp, FaultyVolume, forward_volume_methods};
use super::safety_oracle::{SafetySpec, assert_operation_was_safe};
use crate::event_sinks::CollectorEventSink;
use crate::in_flight_temps::{ItemKind, Ledger, Record, TrackedItem};
use crate::journal;
use crate::testing::{TestOperationLog, TestOperations};
use crate::types::{ConflictResolution, VolumeCopyConfig, WriteOperationError};

const OLD: &[u8] = b"the user's notes, before";
const NEW: &[u8] = b"the new notes from the source";
const KEEP: &[u8] = b"a dest-only file the source never shadows";
const FRESH: &[u8] = b"a source-only file, new to the destination";
const SIBLING_A: &[u8] = b"sibling a, new to the destination";
const SIBLING_Z: &[u8] = b"sibling z, new to the destination";

/// The five places a safe-replace lands.
#[derive(Clone, Copy, Debug)]
enum Site {
    /// One source file, so the serial driver runs it.
    Serial,
    /// Three source files, so the concurrent driver runs them.
    Concurrent,
    /// A folder merging into the user's folder: the clash is a deep child.
    MergeChild,
    /// One source file, moved.
    Move,
    /// A folder out of a `.tar.gz`, merging into the user's folder.
    Extract,
}

impl Site {
    const ALL: [Site; 5] = [
        Site::Serial,
        Site::Concurrent,
        Site::MergeChild,
        Site::Move,
        Site::Extract,
    ];

    /// The user's file the Overwrite replaces.
    fn orig(self) -> &'static str {
        match self {
            Site::Serial | Site::Concurrent | Site::Move => "/dest/notes.txt",
            Site::MergeChild | Site::Extract => "/dest/album/notes.txt",
        }
    }

    /// Where the rescue puts the new bytes when the swap's landing is refused
    /// and the original can't go back either.
    fn recovered(self) -> &'static str {
        match self {
            Site::Serial | Site::Concurrent | Site::Move => "/dest/notes (recovered).txt",
            Site::MergeChild | Site::Extract => "/dest/album/notes (recovered).txt",
        }
    }

    /// The source file whose bytes replace `orig`, on an in-memory source.
    fn source_notes(self) -> &'static str {
        match self {
            Site::Serial | Site::Concurrent | Site::Move => "/notes.txt",
            Site::MergeChild | Site::Extract => "/album/notes.txt",
        }
    }

    /// The archive and its flags don't travel: a tar member reports no flags.
    fn carries_flags(self) -> bool {
        !matches!(self, Site::Extract)
    }

    fn kind(self) -> OpKind {
        match self {
            Site::Move => OpKind::Move,
            _ => OpKind::Copy,
        }
    }
}

/// The fault a cell arms, if any.
#[derive(Clone, Copy, Debug)]
enum Arm {
    Nothing,
    /// The write of the replacing file fails: its source's first read on an
    /// in-memory source, the destination's first write for the extract (whose
    /// archive order puts `notes.txt` first).
    WriteFails,
    /// The destination refuses to move `orig` aside, and the original stays.
    MoveAsideRefused,
    /// The destination refuses the temp's rename onto `orig` (the move aside
    /// before it went through); putting the original back forwards.
    RenameOntoOrigRefused,
    /// The destination's `orig` takes nothing once the original went aside:
    /// neither the temp nor the original going back.
    OrigTakesNothing,
}

fn refusal(what: &str) -> VolumeError {
    VolumeError::PermissionDenied {
        path: what.to_string(),
        raw_os_error: None,
    }
}

/// A finished cell: the result, both sides, and the journal it wrote.
struct Ran {
    site: Site,
    result: Result<(), WriteOperationError>,
    source: Arc<dyn Volume>,
    /// The in-memory source under any wrapper; `None` for the archive.
    source_inner: Option<Arc<InMemoryVolume>>,
    dest: Arc<FaultyVolume<InMemoryVolume>>,
    dest_inner: Arc<InMemoryVolume>,
    source_faulty: Option<Arc<FaultyVolume<InMemoryVolume>>>,
    /// The archive's root on its volume, for the oracle's source walk.
    archive_root: Option<String>,
    log: TestOperationLog,
    op_id: String,
    /// Held for its `Drop`: it owns the `.tar.gz` the archive volume reads.
    _archive: Option<TarGzFixture>,
}

/// Runs one Overwrite at `site` with `arm`'s fault armed, journaled the way the
/// app journals a volume op.
async fn run(site: Site, arm: Arm) -> Ran {
    let op_id = format!("op-safe-replace-{site:?}-{arm:?}");
    let log = TestOperationLog::new("safe-replace-grid");
    let engine = TestOperations::journaling_into(&log);

    // The destination: the user's file, and a dest-only sibling.
    let dest_inner = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest_inner.create_directory(app_path!("/dest")).await.unwrap();
    if matches!(site, Site::MergeChild | Site::Extract) {
        dest_inner.create_directory(app_path!("/dest/album")).await.unwrap();
        dest_inner
            .create_file(app_path!("/dest/album/keep.txt"), KEEP)
            .await
            .unwrap();
    } else {
        dest_inner.create_file(app_path!("/dest/keep.txt"), KEEP).await.unwrap();
    }
    dest_inner
        .create_file(&AppPath::legacy_unchecked(Path::new(site.orig())), OLD)
        .await
        .unwrap();
    if let Arm::OrigTakesNothing = arm {
        dest_inner.set_rename_to_failing(Path::new(site.orig()));
    }
    let mut dest = FaultyVolume::wrapping(Arc::clone(&dest_inner));
    dest = match (arm, site) {
        (Arm::WriteFails, Site::Extract) => {
            dest.failing_call(FaultyOp::WriteFromStream, 1, refusal("simulated write refusal"))
        }
        (Arm::MoveAsideRefused, _) => dest.failing_call_at(
            FaultyOp::RenameFrom,
            site.orig(),
            1,
            refusal("simulated move-aside refusal"),
        ),
        (Arm::RenameOntoOrigRefused, _) => {
            dest.failing_call_at(FaultyOp::Rename, site.orig(), 1, refusal("simulated rename refusal"))
        }
        _ => dest,
    };
    let dest = dest.arc();

    // The source.
    let (source, source_inner, source_faulty, sources, archive) = if let Site::Extract = site {
        let archive = TarGzFixture::new(&[("album/notes.txt", NEW), ("album/fresh.txt", FRESH)]);
        let source = archive.volume();
        let sources = vec![archive.inner("album")];
        (source, None, None, sources, Some(archive))
    } else {
        let inner = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
        let sources = match site {
            Site::Serial | Site::Move => vec![PathBuf::from("/notes.txt")],
            Site::Concurrent => {
                inner.create_file(app_path!("/a.txt"), SIBLING_A).await.unwrap();
                inner.create_file(app_path!("/z.txt"), SIBLING_Z).await.unwrap();
                vec![
                    PathBuf::from("/a.txt"),
                    PathBuf::from("/notes.txt"),
                    PathBuf::from("/z.txt"),
                ]
            }
            Site::MergeChild => {
                inner.create_directory(app_path!("/album")).await.unwrap();
                inner.create_file(app_path!("/album/fresh.txt"), FRESH).await.unwrap();
                vec![PathBuf::from("/album")]
            }
            Site::Extract => unreachable!("handled above"),
        };
        inner
            .create_file(&AppPath::legacy_unchecked(Path::new(site.source_notes())), NEW)
            .await
            .unwrap();
        inner.set_carried_metadata(
            Path::new(site.source_notes()),
            CarriedMetadata {
                flags: UserFlags::LOCKED,
                ..CarriedMetadata::default()
            },
        );
        let mut faulty = FaultyVolume::wrapping(Arc::clone(&inner));
        if let Arm::WriteFails = arm {
            faulty = faulty.failing_call_at(
                FaultyOp::ReadStream,
                site.source_notes(),
                1,
                refusal("simulated read refusal"),
            );
        }
        let faulty = faulty.arc();
        (
            Arc::clone(&faulty) as Arc<dyn Volume>,
            Some(inner),
            Some(faulty),
            sources,
            None,
        )
    };

    let state = Arc::new(
        engine
            .new_operation_state(Duration::from_millis(50))
            .with_journal_volumes("src-vol".into(), "dst-vol".into()),
    );
    journal::open_volume_op(
        state.scope(),
        &op_id,
        site.kind(),
        Initiator::User,
        "src-vol",
        Some("dst-vol"),
        0,
    );
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Overwrite,
        ..VolumeCopyConfig::default()
    };
    let events = Arc::new(CollectorEventSink::new());
    let dest_dyn = Arc::clone(&dest) as Arc<dyn Volume>;
    let result = match site {
        Site::Move => {
            super::move_cross::move_volumes_with_progress(
                events,
                &op_id,
                &state,
                Arc::clone(&source),
                &sources,
                dest_dyn,
                Path::new("/dest"),
                &config,
            )
            .await
        }
        _ => {
            super::copy::copy_volumes_with_progress(
                events,
                &op_id,
                &state,
                Arc::clone(&source),
                &sources,
                dest_dyn,
                Path::new("/dest"),
                &config,
            )
            .await
        }
    }
    .map_err(|failure| failure.error);
    let status = if result.is_ok() {
        ExecutionStatus::Done
    } else {
        ExecutionStatus::Failed
    };
    journal::finalize_op(state.scope(), site.kind(), status);

    Ran {
        site,
        result,
        source,
        source_inner,
        dest,
        dest_inner,
        source_faulty,
        archive_root: archive.as_ref().map(|a| a.path.display().to_string()),
        log,
        op_id,
        _archive: archive,
    }
}

impl Ran {
    fn label(&self) -> String {
        format!("{:?}", self.site)
    }

    async fn dest_file(&self, path: &str) -> Option<Vec<u8>> {
        read_all(&*self.dest_inner, Path::new(path)).await
    }

    /// Every `.cmdr-tmp-*` anywhere under the destination.
    async fn temps_left(&self) -> Vec<String> {
        let mut found = Vec::new();
        let mut dirs = vec![PathBuf::from("/dest")];
        while let Some(dir) = dirs.pop() {
            for entry in self
                .dest_inner
                .list_directory(&AppPath::legacy_unchecked(&dir), None)
                .await
                .unwrap()
            {
                if entry.is_directory {
                    dirs.push(PathBuf::from(&entry.path));
                } else if entry.name.contains(".cmdr-tmp-") {
                    found.push(entry.path);
                }
            }
        }
        found
    }

    /// Whether the moved file is still at the source (only asked of `Move`).
    async fn source_still_holds_the_file(&self) -> bool {
        let inner = self.source_inner.as_ref().expect("an in-memory source");
        read_all(&**inner, Path::new(self.site.source_notes())).await.as_deref() == Some(NEW)
    }

    /// The three oracle clauses. `delivered` holds only where the op finished.
    async fn assert_safe(&self, delivered: bool) {
        let label = self.label();
        let (source_root, dest_root) = match (self.site, &self.archive_root) {
            (Site::Extract, Some(root)) => (root.as_str(), "/dest"),
            _ => ("/", "/dest"),
        };
        let mut source_files: Vec<(&str, &[u8])> = vec![("/notes.txt", NEW)];
        let mut deliveries: Vec<(&str, &[u8])> = Vec::new();
        let untouched: Vec<(&str, &[u8])>;
        match self.site {
            Site::Serial | Site::Move => {
                untouched = vec![("/keep.txt", KEEP)];
                deliveries.push(("/notes.txt", NEW));
            }
            Site::Concurrent => {
                source_files.extend([("/a.txt", SIBLING_A), ("/z.txt", SIBLING_Z)]);
                untouched = vec![("/keep.txt", KEEP)];
                deliveries.extend([("/notes.txt", NEW), ("/a.txt", SIBLING_A), ("/z.txt", SIBLING_Z)]);
            }
            Site::MergeChild | Site::Extract => {
                source_files.push(("/album/fresh.txt", FRESH));
                untouched = vec![("/album/keep.txt", KEEP)];
                deliveries.extend([("/album/notes.txt", NEW), ("/album/fresh.txt", FRESH)]);
            }
        }
        assert_operation_was_safe(
            &self.source,
            &(Arc::clone(&self.dest) as Arc<dyn Volume>),
            &SafetySpec {
                label: &label,
                source_root,
                dest_root,
                source_files: &source_files,
                delivered: if delivered { &deliveries } else { &[] },
                untouched_dest: &untouched,
            },
        )
        .await;
    }

    /// The journal's word on the op, and every file path it recorded.
    fn journal(&self) -> (RollbackState, Vec<String>) {
        let conn = open_read_connection(self.log.db()).expect("read conn");
        let row = read_operation(&conn, &self.op_id).expect("read").expect("op row");
        let detail = cmdr_operation_log::query::get_operation(&conn, &self.op_id, 1_000, 0)
            .expect("read detail")
            .expect("the op is journaled");
        let files = detail
            .items
            .iter()
            .filter(|i| i.entry_type == EntryType::File)
            .filter_map(|i| i.dest_path.clone())
            .collect();
        (row.rollback_state, files)
    }

    /// The op stops being rollbackable at exactly one moment: when the
    /// original is (or may be) gone, which is once the landing's `delete(orig)`
    /// went through. Before that, a failure leaves the user's file where it
    /// was, and what else the op wrote can still be undone.
    fn assert_rollbackable_while_the_original_survives(&self, original_survived: bool) {
        let label = self.label();
        let (rollback, _) = self.journal();
        let expected = if original_survived {
            RollbackState::Rollbackable
        } else {
            RollbackState::NotRollbackable
        };
        assert_eq!(
            rollback, expected,
            "{label}: rollbackable exactly while the original survives (survived: {original_survived})"
        );
    }
}

async fn read_all(volume: &dyn Volume, path: &Path) -> Option<Vec<u8>> {
    let mut stream = volume.open_read_stream(&AppPath::legacy_unchecked(path)).await.ok()?;
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        buf.extend_from_slice(&chunk.ok()?);
    }
    Some(buf)
}

/// A `.tar.gz` on disk with an `ArchiveVolume` over it (a compressed tar is
/// sequential, so it takes the one-pass extract). Members land in the order
/// given.
pub(super) struct TarGzFixture {
    /// Held for its `Drop`: it owns the directory `path` lives in.
    _dir: TestDir,
    path: PathBuf,
}

impl TarGzFixture {
    pub(super) fn new(files: &[(&str, &[u8])]) -> Self {
        let mut builder = ::tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = ::tar::Header::new_ustar();
            header.set_mtime(1_700_000_000);
            header.set_mode(0o644);
            header.set_size(data.len() as u64);
            header.set_entry_type(::tar::EntryType::Regular);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).expect("append file");
        }
        let plain = builder.into_inner().expect("finish tar");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&plain).expect("gz write");
        let dir = TestDir::new("safe-replace-grid");
        let path = dir.join("fixture.tar.gz");
        std::fs::write(&path, gz.finish().expect("gz finish")).expect("write fixture");
        Self { _dir: dir, path }
    }

    pub(super) fn volume(&self) -> Arc<dyn Volume> {
        let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("parent").with_local_fs_access());
        Arc::new(ArchiveVolume::new(
            parent,
            self.path.clone(),
            ArchiveFormat::Tar(TarCodec::Gzip),
            VolumeHost::detached(),
        ))
    }

    /// Where the `.tar.gz` is: the archive volume's root, for the oracle.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn inner(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }
}

// ── The grid ────────────────────────────────────────────────────────────────

/// The Overwrite lands: new bytes at the original's name, journaled at that
/// name and not rollbackable, Locked on the original's name, no temp.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_site_lands_the_new_bytes_at_the_originals_name() {
    for site in Site::ALL {
        let ran = run(site, Arm::Nothing).await;
        let label = ran.label();
        assert!(ran.result.is_ok(), "{label}: the overwrite lands, got {:?}", ran.result);
        assert_eq!(ran.dest_file(site.orig()).await.as_deref(), Some(NEW), "{label}");
        assert_eq!(ran.temps_left().await, Vec::<String>::new(), "{label}: no temp left");
        if site.carries_flags() {
            assert_eq!(
                ran.dest_inner
                    .carried_metadata(Path::new(site.orig()))
                    .map(|carried| carried.flags),
                Some(UserFlags::LOCKED),
                "{label}: Locked goes on at the original's name"
            );
        }
        let (rollback, files) = ran.journal();
        assert_eq!(
            rollback,
            RollbackState::NotRollbackable,
            "{label}: an op that overwrote can't roll back"
        );
        assert!(
            files.iter().any(|f| f == site.orig()),
            "{label}: the journal records the original's path, got {files:?}"
        );
        assert!(
            !files.iter().any(|f| f.contains(".cmdr-tmp-")),
            "{label}: the journal never names a temp, got {files:?}"
        );
        if let Site::Move = site {
            assert!(
                !ran.source_still_holds_the_file().await,
                "{label}: a move takes the source"
            );
        }
        ran.assert_safe(true).await;
    }
}

/// The write fails: the original is untouched and no temp is left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_write_leaves_the_original_and_no_temp_at_every_site() {
    for site in Site::ALL {
        let ran = run(site, Arm::WriteFails).await;
        let label = ran.label();
        let fired = match &ran.source_faulty {
            Some(source) => source.fault_fired(FaultyOp::ReadStream),
            None => ran.dest.fault_fired(FaultyOp::WriteFromStream),
        };
        assert!(fired, "{label}: the write fault fired");
        assert!(ran.result.is_err(), "{label}: a failed write fails the op");
        assert_eq!(ran.dest_file(site.orig()).await.as_deref(), Some(OLD), "{label}");
        assert_eq!(ran.temps_left().await, Vec::<String>::new(), "{label}: no temp left");
        if let Site::Move = site {
            assert!(ran.source_still_holds_the_file().await, "{label}: the source stays");
        }
        ran.assert_rollbackable_while_the_original_survives(true);
        ran.assert_safe(false).await;
    }
}

/// The destination refuses to move `orig` aside and keeps it: the original is
/// untouched, the temp is taken away at once, and the refusal arrives typed,
/// naming the original.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_move_aside_of_the_original_keeps_it_and_takes_the_temp_away_at_every_site() {
    for site in Site::ALL {
        let ran = run(site, Arm::MoveAsideRefused).await;
        let label = ran.label();
        assert!(
            ran.dest.fault_fired(FaultyOp::RenameFrom),
            "{label}: the move-aside fault fired"
        );
        match &ran.result {
            Err(WriteOperationError::PermissionDenied { path, .. }) => {
                assert_eq!(path, site.orig(), "{label}: the refusal names the original");
            }
            other => panic!("{label}: the refusal arrives typed, got {other:?}"),
        }
        assert_eq!(ran.dest_file(site.orig()).await.as_deref(), Some(OLD), "{label}");
        assert_eq!(ran.temps_left().await, Vec::<String>::new(), "{label}: no temp left");
        if let Site::Move = site {
            assert!(ran.source_still_holds_the_file().await, "{label}: the source stays");
        }
        ran.assert_rollbackable_while_the_original_survives(true);
        ran.assert_safe(false).await;
    }
}

/// The original went aside and the temp's rename onto its name was refused:
/// the original goes back to its name, the temp goes, and the refusal arrives
/// typed. Nothing was replaced, so the op stays rollbackable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_landing_puts_the_original_back_at_every_site() {
    for site in Site::ALL {
        let ran = run(site, Arm::RenameOntoOrigRefused).await;
        let label = ran.label();
        assert!(
            ran.dest.fault_fired(FaultyOp::Rename),
            "{label}: the rename fault fired"
        );
        match &ran.result {
            Err(WriteOperationError::PermissionDenied { path, .. }) => {
                assert_eq!(path, site.orig(), "{label}: the refusal names the original");
            }
            other => panic!("{label}: the refusal arrives typed, got {other:?}"),
        }
        assert_eq!(
            ran.dest_file(site.orig()).await.as_deref(),
            Some(OLD),
            "{label}: the original is back at its name"
        );
        assert_eq!(ran.temps_left().await, Vec::<String>::new(), "{label}: no temp left");
        if let Site::Move = site {
            assert!(ran.source_still_holds_the_file().await, "{label}: the source stays");
        }
        ran.assert_rollbackable_while_the_original_survives(true);
        ran.assert_safe(false).await;
    }
}

/// The original went aside, and its name takes neither the temp nor the
/// original going back: the new bytes leave temp space for ` (recovered)` and
/// the failure names where they went, and ❗ the original stays whole at its
/// aside (recorded, for the volume's next sweep to put back).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_that_takes_nothing_rescues_the_new_bytes_and_keeps_the_original_aside_at_every_site() {
    for site in Site::ALL {
        let ran = run(site, Arm::OrigTakesNothing).await;
        let label = ran.label();
        match &ran.result {
            Err(WriteOperationError::NewDataKeptAt { path, kept_at, .. }) => {
                assert_eq!(path, site.orig(), "{label}");
                assert_eq!(kept_at, site.recovered(), "{label}: the failure names the rescue");
            }
            other => panic!("{label}: the failure names where the new bytes went, got {other:?}"),
        }
        assert_eq!(
            ran.dest_file(site.recovered()).await.as_deref(),
            Some(NEW),
            "{label}: the rescue holds the new bytes"
        );
        let dir = Path::new(site.orig()).parent().expect("a parent");
        let asides: Vec<PathBuf> = ran
            .dest_inner
            .list_directory(&AppPath::legacy_unchecked(dir), None)
            .await
            .unwrap()
            .into_iter()
            .map(|entry| PathBuf::from(entry.path))
            .filter(|path| path.to_string_lossy().contains(".cmdr-temp-"))
            .collect();
        let [aside] = asides.as_slice() else {
            panic!("{label}: ❗ exactly one aside holds the original: {asides:?}");
        };
        assert_eq!(read_all(&*ran.dest_inner, aside).await.as_deref(), Some(OLD), "{label}");
        assert_eq!(ran.temps_left().await, Vec::<String>::new(), "{label}: no temp left");
        if let Site::Move = site {
            assert!(ran.source_still_holds_the_file().await, "{label}: the source stays");
        }
    }
}

// ── A task dropped mid-swap ─────────────────────────────────────────────────

/// A destination that holds the rename onto `held_onto` forever, after saying
/// it got there: the swap's move aside went through, and the task sits
/// between it and the landing.
struct HoldsTheSwapRename {
    inner: Arc<InMemoryVolume>,
    held_onto: PathBuf,
    reached: Arc<Notify>,
    /// The temp the held rename would have landed, and what it held then.
    held_temp: StdMutex<Option<(PathBuf, Option<Vec<u8>>)>>,
}

impl Volume for HoldsTheSwapRename {
    forward_volume_methods!(inner =>
        name, root, lane_key, list_directory, get_metadata, exists, is_directory, create_file,
        create_directory, create_directory_all, delete, get_space_info, supports_streaming,
        supports_export, operations_are_local, max_concurrent_ops, scan_for_copy, scan_for_copy_batch,
        scan_for_conflicts, open_read_stream, supports_unknown_length_writes, write_from_stream,
        write_is_single_shot, apply_metadata, metadata_caps, folder_metadata,
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
        let to_app = to;
        let to_std = to.as_std_legacy();
        let to: &Path = &to_std;
        if to != self.held_onto {
            return self.inner.rename(from, to_app, force);
        }
        Box::pin(async move {
            let from_std = from.as_std_legacy();
            let from: &Path = &from_std;
            let held = read_all(&*self.inner, from).await;
            *self.held_temp.lock_ignore_poison() = Some((from.to_path_buf(), held));
            self.reached.notify_one();
            std::future::pending().await
        })
    }
}

/// A source whose read of `fails` waits until the swap is held, then refuses:
/// the sibling's failure that starts the concurrent driver's drain.
struct FailsOnceTheSwapIsHeld {
    inner: Arc<InMemoryVolume>,
    fails: PathBuf,
    reached: Arc<Notify>,
}

impl Volume for FailsOnceTheSwapIsHeld {
    forward_volume_methods!(inner =>
        name, root, lane_key, list_directory, get_metadata, exists, is_directory, get_space_info,
        supports_streaming, supports_export, operations_are_local, max_concurrent_ops, scan_for_copy,
        scan_for_copy_batch, metadata_caps,
    );
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        let path_app = path;
        let path_std = path.as_std_legacy();
        let path: &Path = &path_std;
        if path != self.fails {
            return self.inner.open_read_stream(path_app);
        }
        Box::pin(async move {
            self.reached.notified().await;
            Err(refusal("simulated read refusal"))
        })
    }
}

/// The destination volume id the dropped-swap cell records its landing under.
const LEDGER_VOLUME_ID: &str = "safe-replace-grid-nas";

/// ❗ A top-level concurrent safe-replace task sits between its move aside and
/// its landing when a sibling fails, and the landing never answers, so the
/// failure drain's deadline drops it. The user's original is whole at its
/// aside, and the persisted ledger holds that aside (a `FileAside` at the new
/// file's size) for the volume's next sweep: the temp is a spare copy any
/// cleanup may take, since the source still holds the new bytes. The failure
/// still names the sibling, and the next launch puts the original back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_concurrent_task_dropped_mid_swap_keeps_the_original_for_the_sweep_to_put_back() {
    let _drain = super::copy::wedge_test_support::CancelDrainGuard::set(Duration::from_millis(150));
    let dir = TestDir::new("safe-replace-grid-dropped-swap");
    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let ledger = Ledger::recording_in(&data_dir);
    let engine = TestOperations::new();
    let reached = Arc::new(Notify::new());
    let source_inner = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    for (name, bytes) in [("/a.txt", SIBLING_A), ("/notes.txt", NEW), ("/z.txt", SIBLING_Z)] {
        source_inner
            .create_file(AppPath::from_normalized(name).expect("a canonical path"), bytes)
            .await
            .unwrap();
    }
    let source = Arc::new(FailsOnceTheSwapIsHeld {
        inner: Arc::clone(&source_inner),
        fails: PathBuf::from("/z.txt"),
        reached: Arc::clone(&reached),
    });
    let dest_inner = Arc::new(InMemoryVolume::new("Dest").with_space_info(10_000_000, 10_000_000));
    dest_inner.create_directory(app_path!("/dest")).await.unwrap();
    dest_inner.create_file(app_path!("/dest/notes.txt"), OLD).await.unwrap();
    let dest = Arc::new(HoldsTheSwapRename {
        inner: Arc::clone(&dest_inner),
        held_onto: PathBuf::from("/dest/notes.txt"),
        reached,
        held_temp: StdMutex::new(None),
    });
    let state = Arc::new(
        engine
            .new_operation_state(Duration::from_millis(50))
            .with_journal_volumes("safe-replace-grid-source".into(), LEDGER_VOLUME_ID.into())
            .with_in_flight_ledger(ledger.clone()),
    );

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        super::copy::copy_volumes_with_progress(
            Arc::new(CollectorEventSink::new()),
            "op-safe-replace-dropped-mid-swap",
            &state,
            Arc::clone(&source) as Arc<dyn Volume>,
            &[
                PathBuf::from("/a.txt"),
                PathBuf::from("/notes.txt"),
                PathBuf::from("/z.txt"),
            ],
            Arc::clone(&dest) as Arc<dyn Volume>,
            Path::new("/dest"),
            &VolumeCopyConfig {
                conflict_resolution: ConflictResolution::Overwrite,
                ..VolumeCopyConfig::default()
            },
        ),
    )
    .await
    .expect("the driver drops the held task instead of waiting on it");

    let (_, held) = dest
        .held_temp
        .lock_ignore_poison()
        .take()
        .expect("the swap reached its held landing");
    assert_eq!(
        held.as_deref(),
        Some(NEW),
        "the temp held the whole new file at the hold"
    );
    match result {
        Err(failure) => match failure.error {
            WriteOperationError::PermissionDenied { path, .. } => {
                assert_eq!(path, "/z.txt", "the failure names the sibling");
            }
            other => panic!("the sibling's refusal ends the op, got {other:?}"),
        },
        Ok(()) => panic!("a sibling's failure fails the op"),
    }
    assert_eq!(
        read_all(&*dest_inner, Path::new("/dest/notes.txt")).await,
        None,
        "the swap's move aside went through"
    );
    let asides: Vec<PathBuf> = dest_inner
        .list_directory(app_path!("/dest"), None)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| PathBuf::from(entry.path))
        .filter(|path| path.to_string_lossy().contains(".cmdr-temp-"))
        .collect();
    let [aside] = asides.as_slice() else {
        panic!("❗ exactly one aside holds the original: {asides:?}");
    };
    assert_eq!(read_all(&*dest_inner, aside).await.as_deref(), Some(OLD));
    let aside_recorded = Ledger::replay_log(&data_dir).iter().any(|record| {
        matches!(record, Record::Tracked(TrackedItem {
            kind: ItemKind::FileAside { destination, expected_size },
            path,
            ..
        }) if path == aside && destination == Path::new("/dest/notes.txt") && *expected_size == NEW.len() as u64)
    });
    assert!(
        aside_recorded,
        "the persisted ledger holds the aside, at the new file's size"
    );

    ledger.die();
    let next_app = TestOperations::new();
    next_app
        .volumes()
        .register(LEDGER_VOLUME_ID, Arc::clone(&dest_inner) as Arc<dyn Volume>);
    let tally = Ledger::for_test().launch_in(&data_dir, &next_app).wait();
    assert_eq!(
        read_all(&*dest_inner, Path::new("/dest/notes.txt")).await.as_deref(),
        Some(OLD),
        "the next launch puts the original back at its name: {tally:?}"
    );
    assert_eq!(read_all(&*dest_inner, aside).await, None, "and the aside is gone");
    assert_eq!(
        read_all(&*source_inner, Path::new("/notes.txt")).await.as_deref(),
        Some(NEW),
        "the source still holds the new bytes"
    );
    drop(engine);
}
