//! A name the destination refuses asks Skip / Skip all / Stop per item instead
//! of ending the transfer (cmdr-reports#83: a 1,000-file move onto an encrypted
//! Synology share stopped at the first long name, refused at the final rename).
//!
//! The share here refuses any name over 143 bytes at the landing rename, the
//! way eCryptfs does: the staged temp fits, the final name doesn't. Each cell
//! runs a driver over a mix of short names (which land) and long ones (which
//! the person skips), and checks what the data-safety contract asks of a skip:
//! nothing of it at the destination, its source still there after a MOVE, and
//! the skip counted.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::conflict_responder_test_support::ConflictResponderSink;
use super::faulty_volume::FaultyVolume;
use super::{copy_volumes_with_progress, move_volumes_with_progress};
use crate::state::WriteOperationState;
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, RefusedNameReason, VolumeCopyConfig, WriteOperationError};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::staging::MAX_SCRATCH_NAME_BYTES;
use cmdr_fs::volume::{InMemoryVolume, Volume};

/// A name the share refuses: 150-odd bytes, over its 143.
fn long_name(tag: &str) -> String {
    format!("{tag} – {}.flac", "a live recording with a very long title ".repeat(4))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Driver {
    /// One source folder: `copy.rs` takes the serial driver.
    SerialCopy,
    /// Three sources: the concurrent driver.
    ConcurrentCopy,
    /// `move_cross.rs`.
    CrossVolumeMove,
}

struct Seeded {
    source: Arc<InMemoryVolume>,
    paths: Vec<PathBuf>,
    /// Every source file, and whether it lands (its name, and every folder
    /// above it, fit the share).
    files: Vec<(PathBuf, bool)>,
    /// How many names the share refuses, each one prompt: every long filename
    /// outside a refused folder, plus the refused folder itself.
    refusals: usize,
}

/// `/album/{short,long}` per source, plus a folder deep in it whose own name is
/// too long (two files and a nested folder under it, all skipped with it),
/// and, for the multi-source drivers, a top-level long FILE and a top-level
/// short one.
async fn seeded(driver: Driver) -> Seeded {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    let mut paths = Vec::new();
    let mut files = Vec::new();
    let album = PathBuf::from("/album");
    source
        .create_directory(&AppPath::legacy_unchecked(&album))
        .await
        .unwrap();
    for (name, fits) in [
        ("01 intro.flac".to_string(), true),
        (long_name("02"), false),
        ("03 outro.flac".to_string(), true),
        (long_name("04"), false),
    ] {
        let path = album.join(&name);
        source
            .create_file(&AppPath::legacy_unchecked(&path), b"the user's bytes")
            .await
            .unwrap();
        files.push((path, fits));
    }
    let refused_folder = album.join(long_name("bonus disc").trim_end_matches(".flac"));
    source
        .create_directory(&AppPath::legacy_unchecked(&refused_folder))
        .await
        .unwrap();
    source
        .create_directory(&AppPath::legacy_unchecked(&refused_folder.join("live")))
        .await
        .unwrap();
    for file in ["a.flac", "b.flac", "live/c.flac"] {
        let path = refused_folder.join(file);
        source
            .create_file(&AppPath::legacy_unchecked(&path), b"inside a refused folder")
            .await
            .unwrap();
        files.push((path, false));
    }
    let mut refusals = 3;
    paths.push(album);
    if driver != Driver::SerialCopy {
        for (name, fits) in [(long_name("single"), false), ("short single.flac".to_string(), true)] {
            let path = PathBuf::from("/").join(&name);
            source
                .create_file(&AppPath::legacy_unchecked(&path), b"a top-level file")
                .await
                .unwrap();
            files.push((path.clone(), fits));
            paths.push(path);
        }
        refusals += 1;
    }
    Seeded {
        source,
        paths,
        files,
        refusals,
    }
}

async fn capped_dest() -> Arc<FaultyVolume<InMemoryVolume>> {
    let dest = Arc::new(InMemoryVolume::new("Share").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(app_path!("/inbox")).await.unwrap();
    FaultyVolume::wrapping(dest)
        .refusing_names_longer_than(MAX_SCRATCH_NAME_BYTES)
        .arc()
}

async fn run(
    driver: Driver,
    state: &Arc<WriteOperationState>,
    events: Arc<ConflictResponderSink>,
    seeded: &Seeded,
    dest: &Arc<FaultyVolume<InMemoryVolume>>,
) -> Result<(), WriteOperationError> {
    let source = Arc::clone(&seeded.source) as Arc<dyn Volume>;
    let dest = Arc::clone(dest) as Arc<dyn Volume>;
    let config = VolumeCopyConfig::default();
    let result = match driver {
        Driver::SerialCopy | Driver::ConcurrentCopy => {
            copy_volumes_with_progress(
                events,
                "op-refused",
                state,
                source,
                &crate::testing::app_paths_of(&seeded.paths),
                dest,
                &crate::testing::app_path_of(Path::new("/inbox")),
                &config,
            )
            .await
        }
        Driver::CrossVolumeMove => {
            move_volumes_with_progress(
                events,
                "op-refused",
                state,
                source,
                &crate::testing::app_paths_of(&seeded.paths),
                dest,
                &crate::testing::app_path_of(Path::new("/inbox")),
                &config,
            )
            .await
        }
    };
    result.map(|_| ()).map_err(|failure| failure.error)
}

fn at_dest(source: &Path) -> PathBuf {
    Path::new("/inbox").join(source.strip_prefix("/").unwrap())
}

/// Every long name is asked about, skipped, and kept out of the destination;
/// every short one lands; a move keeps exactly the skipped sources.
async fn assert_skips_each_refused_name(driver: Driver) {
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));
    let seeded = seeded(driver).await;
    let dest = capped_dest().await;

    let result = run(driver, &state, Arc::clone(&events), &seeded, &dest).await;

    result.unwrap_or_else(|e| panic!("{driver:?}: a skipped name must not fail the transfer, got {e:?}"));
    let refused: Vec<_> = seeded.files.iter().filter(|(_, fits)| !fits).collect();
    assert_eq!(
        dest.names_refused(),
        seeded.refusals,
        "{driver:?}: the share refused each long name once"
    );
    let prompts = events.inner.conflicts.lock_ignore_poison().clone();
    assert_eq!(
        prompts.len(),
        seeded.refusals,
        "{driver:?}: one prompt per refused name"
    );
    assert_eq!(
        prompts.iter().filter(|p| p.source_is_directory).count(),
        1,
        "{driver:?}: the refused folder is asked about once, as a folder: {prompts:?}"
    );
    // A file's landing proves its length is what the share refused (its temp
    // fit); a folder's refused `create_directory` proves only that the name
    // can't be stored.
    assert!(
        prompts.iter().all(|p| p.refused_name
            == Some(if p.source_is_directory {
                RefusedNameReason::NotAllowed
            } else {
                RefusedNameReason::TooLong
            })),
        "{driver:?}: each prompt says the name is too long: {prompts:?}"
    );
    for (source, fits) in &seeded.files {
        let landed = dest.exists(&AppPath::legacy_unchecked(&at_dest(source))).await;
        assert_eq!(landed, *fits, "{driver:?}: {} landed={landed}", source.display());
        let kept = seeded.source.exists(&AppPath::legacy_unchecked(source)).await;
        let moved = driver == Driver::CrossVolumeMove;
        // ❗ The data-safety half: a move never deletes the source of an item
        // it skipped, whose only copy is still there.
        assert_eq!(kept, !moved || !fits, "{driver:?}: {} kept={kept}", source.display());
    }
    // Nothing of a skipped item is left at the destination: no temp, no partial.
    let leftovers: Vec<_> = dest
        .inner()
        .list_directory(app_path!("/inbox/album"), None)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.name.contains(".cmdr-tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "{driver:?}: temps left behind: {leftovers:?}");
    let complete = events.inner.complete.lock_ignore_poison();
    assert_eq!(complete.len(), 1, "{driver:?}: the transfer completes");
    assert_eq!(
        complete[0].files_skipped,
        refused.len(),
        "{driver:?}: every file left out counts as skipped, a refused folder's whole subtree included"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_skips_each_refused_name() {
    assert_skips_each_refused_name(Driver::SerialCopy).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_skips_each_refused_name() {
    assert_skips_each_refused_name(Driver::ConcurrentCopy).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_skips_each_refused_name_and_keeps_its_source() {
    assert_skips_each_refused_name(Driver::CrossVolumeMove).await;
}

/// "Skip all" asks once, then skips every later refused name on its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skip_all_asks_once() {
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, true));
    let seeded = seeded(Driver::CrossVolumeMove).await;
    let dest = capped_dest().await;

    run(Driver::CrossVolumeMove, &state, Arc::clone(&events), &seeded, &dest)
        .await
        .unwrap();

    assert_eq!(events.inner.conflicts.lock_ignore_poison().len(), 1);
    for (source, fits) in &seeded.files {
        assert_eq!(
            seeded.source.exists(&AppPath::legacy_unchecked(source)).await,
            !fits,
            "{}",
            source.display()
        );
    }
}

/// An answer that can't store the name stops the transfer with the refusal it
/// was asked about, and a move keeps every source it didn't carry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn any_other_answer_stops_with_the_refusal() {
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Overwrite, false));
    let seeded = seeded(Driver::CrossVolumeMove).await;
    let dest = capped_dest().await;

    let error = run(Driver::CrossVolumeMove, &state, Arc::clone(&events), &seeded, &dest)
        .await
        .expect_err("a Stop ends the transfer");

    let prompts = events.inner.conflicts.lock_ignore_poison().clone();
    assert_eq!(prompts.len(), 1, "the first answer stops it: {prompts:?}");
    let asked_about = prompts[0].destination_path.clone();
    assert!(
        matches!(
            &error,
            WriteOperationError::NameTooLong { path } | WriteOperationError::InvalidName { path, .. }
                if *path == asked_about
        ),
        "got {error:?}"
    );
    for (source, fits) in &seeded.files {
        if !fits {
            assert!(
                seeded.source.exists(&AppPath::legacy_unchecked(source)).await,
                "{} must stay",
                source.display()
            );
        }
    }
}
