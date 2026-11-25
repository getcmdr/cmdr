//! A same-volume move's top-level hints (`preflight.rs::top_level_move_hints`) read each
//! source from its own folder's listing, split by the app path: a POSIX volume's
//! `/src/sub\a.txt` is the one name `sub\a.txt` in `/src` on every host. On Windows `Path`
//! read it as `a.txt` in `/src/sub`, so the move listed `/src/sub` (failing the move when
//! it wasn't there) or took `/src/sub/a.txt`'s hint, another entry's kind.

use super::preflight::top_level_move_hints;
use super::rename_merge_test_support::make_state;
use crate::testing::TestOperations;
use crate::types::VolumeCopyConfig;
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::test]
async fn a_backslash_source_takes_its_own_entrys_hint() {
    let engine = TestOperations::new();
    let state = make_state(&engine);
    let memory = Arc::new(InMemoryVolume::new("Testnas"));
    memory.create_directory(app_path!("/src")).await.unwrap();
    memory.create_file(app_path!("/src/sub\\a.txt"), b"abc").await.unwrap();
    // The decoy `Path`'s Windows split pointed at: a FOLDER `a.txt` in `/src/sub`.
    memory.create_directory(app_path!("/src/sub")).await.unwrap();
    memory.create_directory(app_path!("/src/sub/a.txt")).await.unwrap();
    let volume: Arc<dyn Volume> = memory;
    let source = PathBuf::from("/src/sub\\a.txt");

    let hints = top_level_move_hints(
        &state,
        state.scope().stores().previews(),
        &volume,
        std::slice::from_ref(&source),
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the hints");

    let hint = hints.source_hints.get(&source).expect("a hint for the source");
    assert!(!hint.is_directory, "the source is the file, not the decoy folder");
    assert_eq!(hint.size, 3);
}

/// No `/src/sub` at all: `Path`'s Windows split listed it and failed the move.
#[tokio::test]
async fn hostile_backslash_and_colon_sources_list_their_own_folder() {
    let engine = TestOperations::new();
    let state = make_state(&engine);
    let memory = Arc::new(InMemoryVolume::new("Testnas"));
    memory.create_directory(app_path!("/src")).await.unwrap();
    let names = ["sub\\a.txt", "foo\\..", "..\\bar", "a:b", "trail.", "trail ", "C:\\x"];
    let mut sources = Vec::new();
    for name in names {
        let source = format!("/src/{name}");
        let app = AppPath::from_normalized(&source).expect("canonical");
        memory.create_file(app, b"x").await.unwrap();
        sources.push(PathBuf::from(source));
    }
    let volume: Arc<dyn Volume> = memory;

    let hints = top_level_move_hints(
        &state,
        state.scope().stores().previews(),
        &volume,
        &sources,
        &VolumeCopyConfig::default(),
    )
    .await
    .expect("the hints");

    for source in &sources {
        let hint = hints.source_hints.get(source);
        assert!(
            hint.is_some_and(|hint| !hint.is_directory && hint.size == 1),
            "{source:?}: {hint:?}"
        );
    }
}
