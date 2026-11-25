//! A name the destination's rules REFUSE is refused before any write, on every
//! cross-volume driver, ❌ never left to the landing: Linux's vfat strips a
//! trailing period with no error, so `dup.` lands on a sibling `dup`, and under
//! "Overwrite all" replaces it (`crate::name_rules::DestinationNames`). The
//! destination here is an in-memory volume with a Linux host's FAT rules preset,
//! holding the siblings a stripped name would land on.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::validation::{NameRules, NameTier};
use cmdr_fs::volume::{InMemoryVolume, Volume};

use super::super::conflict_responder_test_support::ConflictResponderSink;
use super::{copy_volumes_with_progress, move_volumes_with_progress};
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, RefusedNameReason, VolumeCopyConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Driver {
    SerialCopy,
    ConcurrentCopy,
    CrossVolumeMove,
}

fn fat_on_linux() -> NameRules {
    NameRules {
        windows_characters: NameTier::Refuse,
        reserved_names: NameTier::Warn,
        ..NameRules::windows_format()
    }
}

const SIBLING: &[u8] = b"the user's own bytes";

async fn run_case(driver: Driver) {
    let source = Arc::new(InMemoryVolume::new("Source").with_space_info(10_000_000, 10_000_000));
    for dir in ["/album", "/album/dir."] {
        source
            .create_directory(AppPath::from_normalized(dir).expect("a canonical path"))
            .await
            .unwrap();
    }
    for file in ["/album/ok.txt", "/album/dup.", "/album/dir./x.txt"] {
        source
            .create_file(AppPath::from_normalized(file).expect("a canonical path"), b"incoming")
            .await
            .unwrap();
    }
    let mut sources = vec![PathBuf::from("/album")];
    if driver != Driver::SerialCopy {
        source.create_file(app_path!("/top."), b"incoming").await.unwrap();
        sources.push(PathBuf::from("/top."));
    }

    // What a stripped name would land on: siblings the user already has.
    let dest = Arc::new(InMemoryVolume::new("Stick").with_space_info(10_000_000, 10_000_000));
    for dir in ["/inbox", "/inbox/album", "/inbox/album/dir"] {
        dest.create_directory(AppPath::from_normalized(dir).expect("a canonical path"))
            .await
            .unwrap();
    }
    for file in ["/inbox/album/dup", "/inbox/album/dir/x.txt", "/inbox/top"] {
        dest.create_file(AppPath::from_normalized(file).expect("a canonical path"), SIBLING)
            .await
            .unwrap();
    }

    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    state.destination_names.resolve(fat_on_linux);
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));
    let config = VolumeCopyConfig {
        conflict_resolution: ConflictResolution::Overwrite,
        ..VolumeCopyConfig::default()
    };
    let (src, dst) = (
        Arc::clone(&source) as Arc<dyn Volume>,
        Arc::clone(&dest) as Arc<dyn Volume>,
    );
    let inbox = Path::new("/inbox");
    let result = match driver {
        Driver::SerialCopy | Driver::ConcurrentCopy => copy_volumes_with_progress(
            Arc::clone(&events) as _,
            "op",
            &state,
            src,
            &crate::testing::app_paths_of(&sources),
            dst,
            &crate::testing::app_path_of(inbox),
            &config,
        )
        .await
        .map(|_| ()),
        Driver::CrossVolumeMove => move_volumes_with_progress(
            Arc::clone(&events) as _,
            "op",
            &state,
            src,
            &crate::testing::app_paths_of(&sources),
            dst,
            &crate::testing::app_path_of(inbox),
            &config,
        )
        .await
        .map(|_| ()),
    };
    result.unwrap_or_else(|f| panic!("{driver:?}: a skipped name must not fail the transfer: {:?}", f.error));

    for sibling in ["/inbox/album/dup", "/inbox/album/dir/x.txt", "/inbox/top"] {
        let mut stream = dest
            .open_read_stream(AppPath::from_normalized(sibling).expect("a canonical path"))
            .await
            .expect("the sibling opens");
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next_chunk().await {
            bytes.extend_from_slice(&chunk.expect("a chunk"));
        }
        assert_eq!(bytes, SIBLING, "{driver:?}: {sibling} was written over");
    }
    for refused in ["/inbox/album/dup.", "/inbox/album/dir.", "/inbox/top."] {
        assert!(
            !dest
                .exists(AppPath::from_normalized(refused).expect("a canonical path"))
                .await,
            "{driver:?}: {refused} landed"
        );
    }
    assert!(
        dest.exists(app_path!("/inbox/album/ok.txt")).await,
        "{driver:?}: ok.txt"
    );

    let prompts = events.inner.conflicts.lock_ignore_poison().clone();
    let expected = if driver == Driver::SerialCopy { 2 } else { 3 };
    assert_eq!(
        prompts.len(),
        expected,
        "{driver:?}: one prompt per refused name: {prompts:?}"
    );
    assert!(
        prompts
            .iter()
            .all(|p| p.refused_name == Some(RefusedNameReason::NotAllowed)),
        "{driver:?}: {prompts:?}"
    );
    assert_eq!(
        prompts.iter().filter(|p| p.source_is_directory).count(),
        1,
        "{driver:?}"
    );
    if driver == Driver::CrossVolumeMove {
        for kept in ["/album/dup.", "/album/dir./x.txt", "/top."] {
            assert!(
                source
                    .exists(AppPath::from_normalized(kept).expect("a canonical path"))
                    .await,
                "a move keeps a skipped source: {kept}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_refuses_a_name_the_destination_refuses() {
    run_case(Driver::SerialCopy).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_refuses_a_name_the_destination_refuses() {
    run_case(Driver::ConcurrentCopy).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_refuses_a_name_the_destination_refuses_and_keeps_its_source() {
    run_case(Driver::CrossVolumeMove).await;
}

/// ❗ A remote name holding `:` or `\` (`12:00 notes`, `back\slash.txt`, a folder `a:b`) is
/// one name on every host: the walk joins it typed and the leaf bodies take the app path,
/// so it lands whole on a remote destination, and a destination whose rules refuse those
/// characters (a local NTFS disk, here preset as FAT's) asks about it per item, ❌ never
/// ending the transfer as a hostile listing does, and ❌ never landing it as a subfolder or
/// a stream. The source is a POSIX server's tree, the destination another remote.
///
/// ❗ The `\` names run on macOS and Linux only: `InMemoryVolume` keys its store by
/// `PathBuf`, so on Windows it reads `a\b.txt` as `b.txt` in a folder `a` and can't hold
/// both (the plan's 10f list, `in_memory.rs`); the engine's own handling is pinned on
/// every host by the typed temp and journal cells. The `:` names run everywhere.
async fn run_remote_names_case(driver: Driver, refusing: bool) {
    let at = |text: &'static str| AppPath::from_normalized(text).unwrap_or_else(|why| panic!("{text}: {why}"));
    let source = Arc::new(InMemoryVolume::new("Server").with_space_info(10_000_000, 10_000_000));
    for dir in ["/album", "/album/a:b", "/album/a"] {
        source.create_directory(at(dir)).await.unwrap();
    }
    let backslash_names: &[(&str, &str)] = &[
        ("/inbox/album/back\\slash.txt", "/album/back\\slash.txt"),
        // `a\b.txt` beside a real folder `a` holding `b.txt`: two files, ❌ never one.
        ("/inbox/album/a\\b.txt", "/album/a\\b.txt"),
    ];
    for file in [
        "/album/ok.txt",
        "/album/12:00 notes.txt",
        "/album/a:b/x.txt",
        "/album/a/b.txt",
    ]
    .into_iter()
    .chain(backslash_names.iter().map(|(_, from)| *from))
    {
        source.create_file(at(file), file.as_bytes()).await.unwrap();
    }
    let sources = vec![PathBuf::from("/album")];
    let dest = Arc::new(InMemoryVolume::new("Other server").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(app_path!("/inbox")).await.unwrap();

    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    if refusing {
        state.destination_names.resolve(fat_on_linux);
    }
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));
    let config = VolumeCopyConfig::default();
    let (src, dst) = (
        Arc::clone(&source) as Arc<dyn Volume>,
        Arc::clone(&dest) as Arc<dyn Volume>,
    );
    let inbox = Path::new("/inbox");
    let result = match driver {
        Driver::SerialCopy | Driver::ConcurrentCopy => copy_volumes_with_progress(
            Arc::clone(&events) as _,
            "op",
            &state,
            src,
            &crate::testing::app_paths_of(&sources),
            dst,
            &crate::testing::app_path_of(inbox),
            &config,
        )
        .await
        .map(|_| ()),
        Driver::CrossVolumeMove => move_volumes_with_progress(
            Arc::clone(&events) as _,
            "op",
            &state,
            src,
            &crate::testing::app_paths_of(&sources),
            dst,
            &crate::testing::app_path_of(inbox),
            &config,
        )
        .await
        .map(|_| ()),
    };
    result.unwrap_or_else(|f| panic!("{driver:?}: a remote name must not end the transfer: {:?}", f.error));

    let read = |path: &'static str| {
        let dest = Arc::clone(&dest);
        async move {
            let mut stream = dest.open_read_stream(at(path)).await.ok()?;
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next_chunk().await {
                bytes.extend_from_slice(&chunk.ok()?);
            }
            Some(bytes)
        }
    };
    for (landed, from) in [
        ("/inbox/album/ok.txt", "/album/ok.txt"),
        ("/inbox/album/a/b.txt", "/album/a/b.txt"),
    ] {
        assert_eq!(
            read(landed).await.as_deref(),
            Some(from.as_bytes()),
            "{driver:?}: {landed}"
        );
    }
    let named: Vec<(&'static str, &'static str)> = [
        ("/inbox/album/12:00 notes.txt", "/album/12:00 notes.txt"),
        ("/inbox/album/a:b/x.txt", "/album/a:b/x.txt"),
    ]
    .into_iter()
    .chain(backslash_names.iter().copied())
    .collect();
    let prompts = events.inner.conflicts.lock_ignore_poison().clone();
    if refusing {
        for &(refused, _) in &named {
            assert!(!dest.exists(at(refused)).await, "{driver:?}: {refused} landed");
        }
        assert!(
            !dest.exists(app_path!("/inbox/album/a:b")).await,
            "{driver:?}: a:b landed"
        );
        // One per refused file, plus the folder `a:b`.
        assert_eq!(
            prompts.len(),
            named.len(),
            "{driver:?}: one prompt per refused name: {prompts:?}"
        );
        assert!(
            prompts
                .iter()
                .all(|p| p.refused_name == Some(RefusedNameReason::NotAllowed)),
            "{driver:?}: {prompts:?}"
        );
        if driver == Driver::CrossVolumeMove {
            for &(_, kept) in &named {
                assert!(source.exists(at(kept)).await, "a move keeps a skipped source: {kept}");
            }
        }
    } else {
        for &(landed, from) in &named {
            assert_eq!(
                read(landed).await.as_deref(),
                Some(from.as_bytes()),
                "{driver:?}: {landed}"
            );
        }
        assert!(prompts.is_empty(), "{driver:?}: nothing to ask about: {prompts:?}");
        if driver == Driver::CrossVolumeMove {
            assert!(
                !source.exists(app_path!("/album")).await,
                "a move takes its source along"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_lands_a_remote_name_holding_a_colon_or_backslash_whole() {
    run_remote_names_case(Driver::SerialCopy, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_lands_a_remote_name_holding_a_colon_or_backslash_whole() {
    run_remote_names_case(Driver::ConcurrentCopy, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_lands_a_remote_name_holding_a_colon_or_backslash_whole() {
    run_remote_names_case(Driver::CrossVolumeMove, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_copy_asks_about_a_colon_or_backslash_name_the_destination_refuses() {
    run_remote_names_case(Driver::SerialCopy, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_copy_asks_about_a_colon_or_backslash_name_the_destination_refuses() {
    run_remote_names_case(Driver::ConcurrentCopy, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cross_volume_move_asks_about_a_colon_or_backslash_name_the_destination_refuses_and_keeps_its_source() {
    run_remote_names_case(Driver::CrossVolumeMove, true).await;
}

/// ❗ A drive folder takes no name holding `\` (a POSIX server's `a\b.txt` copied onto NTFS):
/// the walk asks about THAT item and carries on with its siblings, ❌ never failing the
/// level, which would blame the folder and, on a Skip, drop every sibling with it. The
/// destination is an in-memory volume spelled as a drive, so every host runs it, with no
/// name rules resolved: the typed join's own refusal is what asks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_backslash_name_a_drive_folder_cant_hold_is_asked_about_per_item() {
    use cmdr_fs::app_path::PathFlavor;
    let source = Arc::new(InMemoryVolume::new("Server").with_space_info(10_000_000, 10_000_000));
    source.create_directory(app_path!("/album")).await.unwrap();
    for file in ["/album/ok.txt", "/album/zz.txt"] {
        source
            .create_file(AppPath::from_normalized(file).expect("a canonical path"), b"incoming")
            .await
            .unwrap();
    }
    // Stored under a plain path, listed as a POSIX server lists `a\b.txt` (the store
    // can't hold the name itself on Windows).
    source.set_reported_name(Path::new("/album/zz.txt"), "a\\b.txt");
    let drive = |text: &'static str| AppPath::from_normalized_as(text, PathFlavor::Windows).expect("a drive path");
    let dest = Arc::new(InMemoryVolume::new("Disk").with_space_info(10_000_000, 10_000_000));
    dest.create_directory(drive("C:/inbox")).await.unwrap();

    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(ConflictResponderSink::new(&state, ConflictResolution::Skip, false));
    let result = copy_volumes_with_progress(
        Arc::clone(&events) as _,
        "op",
        &state,
        Arc::clone(&source) as Arc<dyn Volume>,
        &crate::testing::app_paths_of(&[PathBuf::from("/album")]),
        Arc::clone(&dest) as Arc<dyn Volume>,
        &crate::testing::app_path_of(Path::new("C:/inbox")),
        &VolumeCopyConfig::default(),
    )
    .await;
    result.unwrap_or_else(|f| panic!("one unusable name must not end the copy: {:?}", f.error));
    assert!(dest.exists(drive("C:/inbox/album/ok.txt")).await, "its sibling landed");
    let prompts = events.inner.conflicts.lock_ignore_poison().clone();
    assert_eq!(prompts.len(), 1, "one prompt, for the refused name: {prompts:?}");
    assert!(!prompts[0].source_is_directory, "{prompts:?}");
}
