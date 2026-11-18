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
            .create_directory(&AppPath::legacy_unchecked(Path::new(dir)))
            .await
            .unwrap();
    }
    for file in ["/album/ok.txt", "/album/dup.", "/album/dir./x.txt"] {
        source
            .create_file(&AppPath::legacy_unchecked(Path::new(file)), b"incoming")
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
        dest.create_directory(&AppPath::legacy_unchecked(Path::new(dir)))
            .await
            .unwrap();
    }
    for file in ["/inbox/album/dup", "/inbox/album/dir/x.txt", "/inbox/top"] {
        dest.create_file(&AppPath::legacy_unchecked(Path::new(file)), SIBLING)
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
            &sources,
            dst,
            inbox,
            &config,
        )
        .await
        .map(|_| ()),
        Driver::CrossVolumeMove => move_volumes_with_progress(
            Arc::clone(&events) as _,
            "op",
            &state,
            src,
            &sources,
            dst,
            inbox,
            &config,
        )
        .await
        .map(|_| ()),
    };
    result.unwrap_or_else(|f| panic!("{driver:?}: a skipped name must not fail the transfer: {:?}", f.error));

    for sibling in ["/inbox/album/dup", "/inbox/album/dir/x.txt", "/inbox/top"] {
        let mut stream = dest
            .open_read_stream(&AppPath::legacy_unchecked(Path::new(sibling)))
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
            !dest.exists(&AppPath::legacy_unchecked(Path::new(refused))).await,
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
                source.exists(&AppPath::legacy_unchecked(Path::new(kept))).await,
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
