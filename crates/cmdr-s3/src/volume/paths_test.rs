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

/// ❗ A row's text is `RemoteRoot::to_app_path`'s, byte for byte the spelling a row always
/// carried: `<prefix><server path, normalized>`, for a name the typed path can hold and for
/// one it can't yet alike. Pinned as literal strings, the text a pane, a saved tab, and a
/// favorite hold.
#[test]
fn a_rows_text_is_the_normalized_server_path_under_the_prefix() {
    let volume = make_test_volume(Some("photos"));
    for (remote, row) in [
        ("/photos/a.jpg", "/photos/a.jpg"),
        ("/photos/2025//a.jpg", "/photos/2025/a.jpg"),
        ("/photos/./2025/a.jpg", "/photos/2025/a.jpg"),
        ("/photos/2025/x/../a.jpg", "/photos/2025/a.jpg"),
        ("/photos/2025//a\u{10FE80}.txt", "/photos/2025/a\u{10FE80}.txt"),
        ("/photos/./b\u{10E800}", "/photos/b\u{10E800}"),
    ] {
        assert_eq!(
            volume.row_path(remote).as_deref(),
            Some(format!("{PREFIX}{row}").as_str()),
            "{remote:?}"
        );
    }
    assert_eq!(volume.row_path("/photos/../backups/a.tar"), None, "off this place");
}

/// ❗ A place whose bucket or key id the app path can't hold (a NUL, an escape-range code
/// point) refuses the connect, typed by which half, before any request. Any text a
/// provider allows connects as before, non-ASCII included, with the root text it always had.
#[test]
fn a_place_the_app_path_cant_hold_refuses_the_connect() {
    use crate::{S3ConnectError, S3ConnectionParams, S3Provider};
    let place = |key: &str, bucket: Option<&str>| {
        let provider = S3Provider::Other {
            endpoint: url::Url::parse("http://127.0.0.1:1").expect("a valid test URL"),
            region: None,
            path_style: true,
        };
        super::Place::of(&S3ConnectionParams::new(provider, key, bucket).expect("valid params"))
    };
    for bucket in ["a\0b", "x\u{10FE80}", "x\u{10FEFF}", "x\u{10E800}", "x\u{10EFFF}"] {
        assert!(
            matches!(place("AKIATEST", Some(bucket)), Err(S3ConnectError::NoSuchBucket)),
            "{bucket:?}"
        );
    }
    for key in ["key\0", "key\u{10FE80}", "key\u{10EFFF}"] {
        assert!(
            matches!(place(key, Some("photos")), Err(S3ConnectError::KeysRejected)),
            "{key:?}"
        );
        assert!(matches!(place(key, None), Err(S3ConnectError::KeysRejected)), "{key:?}");
    }
    for (key, bucket) in [
        ("AKIATEST", "fotók-2026"),
        ("AKIATEST", "x\u{10FE7F}"),
        ("AKIATEST", "x\u{10F000}"),
        ("AKIATEST", "x\u{F8FF}"),
        ("clé-ünïcode", "photos"),
    ] {
        let place = place(key, Some(bucket)).expect("a place a provider allows connects");
        assert_eq!(place.app.as_str(), format!("s3://{key}@127.0.0.1:1/{bucket}"));
    }
    let account = place("AKIATEST", None).expect("the account root connects");
    assert_eq!(account.app.as_str(), "s3://AKIATEST@127.0.0.1:1/");
}

/// #1310: a key the bucket lists with an escape-range code point is reached as listed: the
/// row's text through the app's door (the listed-text one, which the wire and the bridge
/// spell alike), and what the listing patcher spells comes back the same value.
#[test]
fn a_listed_escape_range_key_is_reached_as_listed() {
    use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
    let volume = make_test_volume(Some("photos"));
    let listed = "/photos/x\u{10FEFF}/caf\u{10FEC3}\u{10FEA9}";
    let row = volume.row_path(listed).expect("a row");
    let sent = AppPathBuf::from_listed_text(&row, PathFlavor::Posix).expect("an app path");
    assert_eq!(volume.to_remote_path(&sent).expect("on the place"), listed);
    assert_eq!(volume.app_path_of(listed), Some(sent));
}
