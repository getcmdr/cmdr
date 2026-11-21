//! `list_as_stored` against a byte-exact volume that knows a foreign spelling.

use std::path::{Path, PathBuf};

use super::*;
use crate::testing::SpelledVolume;
use cmdr_fs::volume::InMemoryVolume;

/// `fotók` composed (how the server stores it) and decomposed (how the macOS
/// kernel mount spelled it for the pane).
const STORED: &str = "/fot\u{f3}k";
const FOREIGN: &str = "/foto\u{301}k";

async fn album() -> InMemoryVolume {
    let volume = InMemoryVolume::new("Share");
    volume
        .create_directory(crate::testing::app(STORED))
        .await
        .expect("seed the album");
    volume
        .create_file(&crate::testing::app_child(STORED, "photo.jpg"), b"jpeg")
        .await
        .expect("seed a photo");
    volume
}

#[tokio::test]
async fn a_path_that_lists_as_given_never_asks_for_another_spelling() {
    let volume = SpelledVolume::new(album().await);

    let listed = list_as_stored(&volume, Path::new(STORED), None, None)
        .await
        .expect("the stored spelling lists");

    assert_eq!(listed.path, Path::new(STORED));
    assert_eq!(listed.stored_spelling_of(Path::new(STORED)), None);
    assert_eq!(volume.resolves(), 0, "the happy path costs nothing extra");
}

/// The pane asked for the kernel mount's spelling: the listing comes back under
/// the server's, and so does every child, so the next click inside is exact.
#[tokio::test]
async fn a_foreign_spelling_lists_the_stored_directory_under_its_stored_path() {
    let volume = SpelledVolume::new(album().await).resolving(FOREIGN, Ok(Some(STORED)));

    let listed = list_as_stored(&volume, Path::new(FOREIGN), None, None)
        .await
        .expect("the foreign spelling resolves and lists");

    assert_eq!(listed.path, Path::new(STORED));
    assert_eq!(
        listed.stored_spelling_of(Path::new(FOREIGN)).as_deref(),
        Some(Path::new(STORED))
    );
    let child = &listed.entries.first().expect("the photo is listed").path;
    assert!(
        child.starts_with(STORED),
        "{child} must carry the stored spelling of its directory"
    );
}

/// No other spelling found: the pane gets the listing's own `NotFound`, which is
/// what drives its "this folder is gone" walk-up.
#[tokio::test]
async fn a_miss_with_no_other_spelling_keeps_its_not_found() {
    let volume = SpelledVolume::new(album().await);

    let result = list_as_stored(&volume, Path::new("/gone"), None, None).await;

    assert!(
        matches!(result, Err(VolumeError::NotFound(_))),
        "got {:?}",
        result.err()
    );
    assert_eq!(volume.resolves(), 1);
}

/// Two look-alikes: the refusal reaches the pane as itself, never a guess.
#[tokio::test]
async fn a_look_alike_refusal_reaches_the_pane() {
    let volume =
        SpelledVolume::new(album().await).resolving(FOREIGN, Err(VolumeError::AmbiguousName(FOREIGN.to_string())));

    let result = list_as_stored(&volume, Path::new(FOREIGN), None, None).await;

    assert!(
        matches!(result, Err(VolumeError::AmbiguousName(_))),
        "got {:?}",
        result.err()
    );
}

/// A pane open in an accented folder when the share moves from the kernel mount
/// to a direct connection holds the kernel's spelling of the folder and of every
/// entry. The respell re-reads it through the new backend: the listing moves to
/// the stored spelling, so the watcher's reports (in server bytes) find it, and
/// its entries are replaced by ones the new backend can open.
#[tokio::test]
async fn a_backend_swap_respells_an_open_listing_and_its_entries() {
    use crate::testing::{TestEngine, TestListing};

    let volume_id = format!("respell-{}", uuid::Uuid::new_v4());
    let volume = SpelledVolume::new(album().await).resolving(FOREIGN, Ok(Some(STORED)));
    let engine = TestEngine::builder()
        .volume(&volume_id, std::sync::Arc::new(volume))
        .build();
    let kernel_entry = FileEntry::new("photo.jpg".to_string(), format!("{FOREIGN}/photo.jpg"), false, false);
    let listing = TestListing::new()
        .volume(&volume_id)
        .path(FOREIGN)
        .entries(vec![kernel_entry])
        .insert(&engine, "respell");

    engine
        .inner
        .respell_listing(volume_id.clone(), listing.id().to_string(), FOREIGN.into())
        .await;
    let found_by_server_bytes = engine
        .inner
        .find_listings_for_path_on_volume(Some(&volume_id), Path::new(STORED));

    listing.with_listing(|cached| {
        assert_eq!(cached.path.as_path(), Path::new(STORED));
        let paths: Vec<&str> = cached.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![format!("{STORED}/photo.jpg")],
            "entries carry the stored bytes"
        );
    });
    assert_eq!(
        found_by_server_bytes.len(),
        1,
        "a change reported in the server's bytes must reach the open pane"
    );
}

/// Only a miss is a spelling question: any other refusal is what it is.
#[tokio::test]
async fn a_refusal_other_than_a_miss_is_not_resolved() {
    let volume = SpelledVolume::new(album().await)
        .resolving(FOREIGN, Ok(Some(STORED)))
        .listings_refused_with(VolumeError::PermissionDenied {
            path: FOREIGN.to_string(),
            raw_os_error: None,
        });

    let result = list_as_stored(&volume, Path::new(FOREIGN), None, None).await;

    assert!(
        matches!(result, Err(VolumeError::PermissionDenied { .. })),
        "got {:?}",
        result.err()
    );
    assert_eq!(volume.resolves(), 0, "a refusal is not a spelling question");
}

/// A file dragged in from Finder, or pasted after a copy there: the kernel
/// mount spelled it decomposed, the server stores it composed. The source lands
/// on the server's own spelling before any operation sees it.
#[tokio::test]
async fn a_leaf_from_outside_cmdr_takes_the_stored_spelling() {
    let foreign_leaf = format!("{FOREIGN}/re\u{301}sz.jpg");
    let stored_leaf = format!("{STORED}/r\u{e9}sz.jpg");
    let volume = SpelledVolume::new(album().await).resolving(&foreign_leaf, Ok(Some(&stored_leaf)));

    let paths = stored_spellings(&volume, vec![PathBuf::from(&foreign_leaf)]).await;

    assert_eq!(paths, vec![PathBuf::from(stored_leaf)]);
}

/// An all-ASCII path can't differ in Unicode form, and the kernel mount keeps
/// case, so it's never a round trip. A path whose spelling is already the
/// stored one keeps it.
#[tokio::test]
async fn an_exact_or_ascii_leaf_stays_as_given() {
    let volume = SpelledVolume::new(album().await);
    let exact = format!("{STORED}/photo.jpg");

    let paths = stored_spellings(&volume, vec![PathBuf::from("/plain/photo.jpg"), PathBuf::from(&exact)]).await;

    assert_eq!(paths, vec![PathBuf::from("/plain/photo.jpg"), PathBuf::from(exact)]);
    assert_eq!(volume.resolves(), 1, "only the accented path asks the volume");
}

/// Two look-alikes and no exact match: the path stays as given, so whatever
/// the operation does next means those exact bytes (on a byte-exact share, a
/// plain "couldn't find"), and never lands on a twin it guessed.
#[tokio::test]
async fn a_look_alike_leaf_is_never_guessed() {
    let foreign_leaf = format!("{FOREIGN}/photo.jpg");
    let volume = SpelledVolume::new(album().await)
        .resolving(&foreign_leaf, Err(VolumeError::AmbiguousName(foreign_leaf.clone())));

    let paths = stored_spellings(&volume, vec![PathBuf::from(&foreign_leaf)]).await;

    assert_eq!(paths, vec![PathBuf::from(foreign_leaf)]);
}
