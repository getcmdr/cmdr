//! Keys aren't paths: `a/b`, `a//b`, `a/./b`, and `a/../b` are four objects
//! in one bucket, but an app path has one spelling for all of them (`…/a/b`).
//! ❗ Only `a/b` may list, and an operation on `…/a/b` may reach only `a/b`:
//! the others' extra segment is empty, `.`, or `..`, a name no path can
//! address, so the listing leaves it out.
//!
//! Over `fake_s3.rs`, which seeds such keys in one line (real servers refuse
//! some of them on the way in).

use std::time::Duration;

use cmdr_fs::os_error::OsErrorClass;
use cmdr_fs::volume::{Volume, VolumeError};

use super::fake_s3::FakeS3;
use super::test_support::at;

/// One object per spelling of `a/b`, each a different size so a stat tells
/// them apart, plus `a/c//d`, which only an empty name reaches.
async fn seeded() -> FakeS3 {
    let s3 = FakeS3::start(Duration::ZERO).await;
    s3.seed("a/b", 1);
    s3.seed("a//b", 2);
    s3.seed("a/./b", 3);
    s3.seed("a/../b", 4);
    s3.seed("a/c//d", 5);
    s3.seed("/x", 6);
    s3.seed("./y", 7);
    s3.seed("../z", 8);
    s3
}

fn rows(listed: &[cmdr_fs::entry::FileEntry]) -> Vec<(String, bool, Option<u64>)> {
    let mut rows: Vec<_> = listed
        .iter()
        .map(|entry| (entry.name.clone(), entry.is_directory, entry.size))
        .collect();
    rows.sort();
    rows
}

#[tokio::test(flavor = "multi_thread")]
async fn keys_that_spell_one_path_list_as_the_plain_key_alone() {
    let s3 = seeded().await;
    let volume = s3.volume();

    let top = volume
        .list_directory(&at(&volume, ""), None)
        .await
        .expect("the bucket lists");
    assert_eq!(rows(&top), vec![("a".to_string(), true, None)], "{top:?}");

    let a = volume.list_directory(&at(&volume, "a"), None).await.expect("a lists");
    assert_eq!(
        rows(&a),
        vec![("b".to_string(), false, Some(1)), ("c".to_string(), true, None)],
        "{a:?}"
    );
    let b = a.iter().find(|entry| entry.name == "b").expect("the b row");
    assert_eq!(b.path, at(&volume, "a/b").as_str());

    let c = volume
        .list_directory(&at(&volume, "a/c"), None)
        .await
        .expect("a/c lists");
    assert!(c.is_empty(), "`a/c//d`'s empty name stays out: {c:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_operation_on_the_shared_path_reaches_only_the_plain_key() {
    let s3 = seeded().await;
    let volume = s3.volume();
    let path = at(&volume, "a/b");

    let stat = volume.get_metadata(&path).await.expect("a/b stats");
    assert_eq!(stat.size, Some(1), "the plain key's own size");

    volume.delete(&path).await.expect("a/b deletes");
    assert!(s3.object("a/b").is_none(), "the plain key went");
    for other in ["a//b", "a/./b", "a/../b"] {
        assert!(s3.object(other).is_some(), "{other} stays");
    }
    assert!(
        matches!(volume.get_metadata(&path).await, Err(VolumeError::NotFound(_))),
        "nothing else answers for a/b now"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_holding_only_an_unlisted_key_refuses_its_delete() {
    let s3 = seeded().await;
    let volume = s3.volume();

    let deleted = volume.delete_directory(&at(&volume, "a/c"), None).await;
    assert!(
        matches!(&deleted, Err(error) if error.os_class() == Some(OsErrorClass::NotEmpty)),
        "`a/c//d` still sits under it: {deleted:?}"
    );
    assert!(s3.object("a/c//d").is_some());
}
