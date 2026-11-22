//! New folder, new file, and rename on a byte-exact volume that holds a name in
//! another Unicode spelling, or that asks for composed new names.
//!
//! The transfer engines' twin is `transfer/volume/look_alike_tests.rs`; the rule
//! both follow is `look_alike.rs`.

use cmdr_fs::app_path;
use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};
use std::sync::Arc;

use super::create::{create_directory_core_on, create_file_core_on};
use super::mutation_error::MutationError;
use crate::testing::TestOperations;

/// Canonical runtime text as the app path a start takes.
fn canonical(text: &str) -> AppPathBuf {
    AppPath::from_normalized(text).expect("canonical test text").to_owned()
}
use cmdr_fs::volume::InMemoryVolume;
use cmdr_fs::volume::Volume;
use cmdr_operation_log::types::Initiator;

const CAFE_NFC: &str = "caf\u{e9}.txt";
const CAFE_NFD: &str = "cafe\u{301}.txt";
const FOTOK_NFC: &str = "fot\u{f3}k";
const FOTOK_NFD: &str = "foto\u{301}k";

/// An in-memory share registered on `engine`: byte-exact, composing new names,
/// holding `/dir`.
async fn share(engine: &TestOperations, label: &str) -> (String, Arc<InMemoryVolume>) {
    let id = format!("look-alike-{label}");
    let volume = Arc::new(InMemoryVolume::new(&id).with_composed_new_names());
    volume.create_directory(app_path!("/dir")).await.unwrap();
    engine.volumes().register(&id, Arc::clone(&volume) as Arc<dyn Volume>);
    (id, volume)
}

/// `/dir/<name>`, the fixture folder's entry.
fn in_dir(name: &str) -> AppPathBuf {
    app_path!("/dir")
        .join(AppName::from_app_spelling(name).expect("one name"))
        .expect("joins")
}

async fn names_in(volume: &InMemoryVolume, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = volume
        .list_directory(AppPath::from_normalized(dir).expect("a canonical path"), None)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_new_folder_and_a_new_file_go_out_composed() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "new").await;

    create_directory_core_on(engine.host(), Some(id.clone()), "/dir", FOTOK_NFD)
        .await
        .unwrap();
    create_file_core_on(engine.host(), Some(id), "/dir", CAFE_NFD)
        .await
        .unwrap();

    let mut expected = vec![CAFE_NFC.to_string(), FOTOK_NFC.to_string()];
    expected.sort();
    assert_eq!(names_in(&volume, "/dir").await, expected);
}

#[tokio::test]
async fn a_new_folder_or_file_is_refused_beside_its_look_alike() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "taken").await;
    volume.create_file(&in_dir(CAFE_NFD), b"x").await.unwrap();
    volume.create_directory(&in_dir(FOTOK_NFD)).await.unwrap();

    let file = create_file_core_on(engine.host(), Some(id.clone()), "/dir", CAFE_NFC).await;
    let folder = create_directory_core_on(engine.host(), Some(id), "/dir", FOTOK_NFC).await;

    assert!(matches!(file, Err(MutationError::AlreadyExists { .. })), "{file:?}");
    assert!(matches!(folder, Err(MutationError::AlreadyExists { .. })), "{folder:?}");
    let mut expected = vec![CAFE_NFD.to_string(), FOTOK_NFD.to_string()];
    expected.sort();
    assert_eq!(names_in(&volume, "/dir").await, expected, "no twin appeared");
}

#[tokio::test]
async fn a_rename_target_goes_out_composed() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "rename-new").await;
    volume.create_file(app_path!("/dir/a.txt"), b"x").await.unwrap();

    engine
        .rename_managed(
            app_path!("/dir/a.txt").to_owned(),
            canonical(&format!("/dir/{CAFE_NFD}")),
            false,
            id,
            Initiator::User,
        )
        .await
        .unwrap();

    assert_eq!(names_in(&volume, "/dir").await, vec![CAFE_NFC.to_string()]);
}

#[tokio::test]
async fn a_rename_onto_a_look_alike_is_refused_unless_forced_and_then_replaces_it() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "rename-taken").await;
    volume.create_file(app_path!("/dir/a.txt"), b"NEW").await.unwrap();
    volume.create_file(&in_dir(CAFE_NFD), b"OLD").await.unwrap();

    let refused = engine
        .rename_managed(
            app_path!("/dir/a.txt").to_owned(),
            canonical(&format!("/dir/{CAFE_NFC}")),
            false,
            id.clone(),
            Initiator::User,
        )
        .await;
    assert!(
        matches!(refused, Err(MutationError::AlreadyExists { .. })),
        "{refused:?}"
    );
    let mut both = vec!["a.txt".to_string(), CAFE_NFD.to_string()];
    both.sort();
    assert_eq!(names_in(&volume, "/dir").await, both);

    engine
        .rename_managed(
            app_path!("/dir/a.txt").to_owned(),
            canonical(&format!("/dir/{CAFE_NFC}")),
            true,
            id,
            Initiator::User,
        )
        .await
        .unwrap();
    assert_eq!(
        names_in(&volume, "/dir").await,
        vec![CAFE_NFD.to_string()],
        "a forced rename replaces the entry that's there, in its own spelling"
    );
}

/// Renaming a decomposed name to its own composed spelling is a respell, not a
/// clash with itself: the one way to fix a name Finder can't open.
#[tokio::test]
async fn respelling_a_name_is_not_a_clash_with_itself() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "respell").await;
    volume.create_file(&in_dir(CAFE_NFD), b"x").await.unwrap();

    let validity = engine
        .check_rename_validity_impl("/dir".into(), CAFE_NFD.into(), CAFE_NFC.into(), id.clone())
        .await
        .priced(|_| false);
    assert!(!validity.has_conflict, "the only look-alike is the file itself");

    engine
        .rename_managed(
            canonical(&format!("/dir/{CAFE_NFD}")),
            canonical(&format!("/dir/{CAFE_NFC}")),
            false,
            id,
            Initiator::User,
        )
        .await
        .unwrap();
    assert_eq!(names_in(&volume, "/dir").await, vec![CAFE_NFC.to_string()]);
}

/// The rename editor's live check names the look-alike, so the user sees the
/// clash before pressing Enter.
#[tokio::test]
async fn the_live_rename_check_reports_a_look_alike() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "validity").await;
    volume.create_file(app_path!("/dir/a.txt"), b"x").await.unwrap();
    volume.create_file(&in_dir(CAFE_NFD), b"x").await.unwrap();

    let validity = engine
        .check_rename_validity_impl("/dir".into(), "a.txt".into(), CAFE_NFC.into(), id)
        .await
        .priced(|_| false);

    assert!(validity.has_conflict);
    assert_eq!(validity.conflict.map(|c| c.name).as_deref(), Some(CAFE_NFD));
}

/// The mirror case: the typed name composes onto a name the folder holds
/// exactly. The rename itself refuses it, so the live check has to say so too,
/// or the editor promises a name that Enter then refuses.
#[tokio::test]
async fn the_live_rename_check_reports_the_name_a_typed_one_composes_onto() {
    let engine = TestOperations::new();
    let (id, volume) = share(&engine, "validity-composed").await;
    volume.create_file(app_path!("/dir/a.txt"), b"x").await.unwrap();
    volume.create_file(&in_dir(CAFE_NFC), b"x").await.unwrap();

    let validity = engine
        .check_rename_validity_impl("/dir".into(), "a.txt".into(), CAFE_NFD.into(), id)
        .await
        .priced(|_| false);

    assert!(validity.has_conflict);
    assert_eq!(validity.conflict.map(|c| c.name).as_deref(), Some(CAFE_NFC));
}
