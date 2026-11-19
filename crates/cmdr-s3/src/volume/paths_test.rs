//! What a path names on a bucket place and on the account root.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::{Volume, VolumeError};

use super::super::test_support::{PREFIX, make_test_volume};
use super::{Target, target_of};

/// `text` as the app path it already is in canonical form.
fn typed(text: &str) -> &AppPath {
    AppPath::from_normalized(text).unwrap_or_else(|why| panic!("{text:?}: {why}"))
}

#[test]
fn a_server_side_path_splits_into_bucket_and_key() {
    assert_eq!(target_of("/"), Target::Account);
    assert_eq!(target_of("/photos"), Target::Bucket("photos"));
    assert_eq!(
        target_of("/photos/2025/a b+c.jpg"),
        Target::Key {
            bucket: "photos",
            key: "2025/a b+c.jpg"
        }
    );
}

#[test]
fn a_bucket_place_is_rooted_at_its_bucket() {
    let volume = make_test_volume(Some("photos"));
    // The text the root always had, compared as text: `Path ==` would let a
    // trailing `/` come or go unseen.
    assert_eq!(volume.root().as_str(), format!("{PREFIX}/photos"));
    assert_eq!(volume.to_remote_path(app_path!("/")).expect("the root"), "/photos");
    assert_eq!(volume.to_remote_path(volume.root()).expect("the root"), "/photos");
    assert_eq!(
        volume
            .to_remote_path(typed(&format!("{PREFIX}/photos/2025/a.jpg")))
            .expect("a path in the bucket"),
        "/photos/2025/a.jpg"
    );
}

#[test]
fn a_bucket_place_refuses_another_buckets_path() {
    // ❗ The same account's other bucket is another place; a pane must never
    // reach it through this one.
    let volume = make_test_volume(Some("photos"));
    let refused = volume.to_remote_path(typed(&format!("{PREFIX}/backups/a.tar")));
    assert!(matches!(refused, Err(VolumeError::NotFound(_))), "got {refused:?}");
}

#[test]
fn the_account_root_reaches_every_bucket() {
    let volume = make_test_volume(None);
    assert_eq!(volume.root().as_str(), format!("{PREFIX}/"));
    assert_eq!(
        volume
            .to_remote_path(typed(&format!("{PREFIX}/backups/a.tar")))
            .expect("any bucket"),
        "/backups/a.tar"
    );
}

#[test]
fn a_bare_server_path_is_refused() {
    let volume = make_test_volume(None);
    assert!(volume.to_remote_path(app_path!("/photos/a.jpg")).is_err());
}

#[test]
fn a_row_keeps_the_text_the_server_path_always_made() {
    let volume = make_test_volume(Some("photos"));
    assert_eq!(
        volume.row_path("/photos/2025/a b+c.jpg").as_deref(),
        Some(format!("{PREFIX}/photos/2025/a b+c.jpg").as_str())
    );
    assert_eq!(volume.row_path("/backups/a.tar"), None, "off this place");
    // A name the typed path can't hold yet keeps today's spelling, never a
    // dropped row.
    let escape_range = "/photos/a\u{10FE80}.txt";
    assert_eq!(
        volume.row_path(escape_range).as_deref(),
        Some(format!("{PREFIX}{escape_range}").as_str())
    );
}
