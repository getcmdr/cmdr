//! The in-flight ledger's record lines, byte for byte, and how a record states and
//! matches a path. The ledger keeps native `PathBuf`s on purpose (the local engine's
//! own bookkeeping), so these pin that nothing about its format moves when the
//! `Volume` trait takes typed app paths. #1225.

use std::path::{Path, PathBuf};

use super::*;

fn item(kind: ItemKind, home: ItemHome, path: &str) -> Record {
    Record::Tracked(TrackedItem {
        kind,
        home,
        path: PathBuf::from(path),
    })
}

/// Each home's line, exactly as the log writes it, decoding back to itself.
#[test]
fn a_record_line_is_byte_identical_and_decodes_to_itself() {
    let cases = [
        (
            item(
                ItemKind::Temp,
                ItemHome::Local,
                "/Users/test_user_1/a b/c.txt.cmdr-tmp-1",
            ),
            r#"{"kind":"temp","home":"local","path":"/Users/test_user_1/a b/c.txt.cmdr-tmp-1"}"#,
        ),
        (
            item(
                ItemKind::FileAside {
                    destination: PathBuf::from("photos/cafe\u{0301}.jpg"),
                    expected_size: 7,
                },
                ItemHome::Mount {
                    volume_id: "usb-1".to_string(),
                },
                "photos/.cafe\u{0301}.jpg.cmdr-aside-1",
            ),
            "{\"kind\":\"file_aside\",\"destination\":\"photos/cafe\u{0301}.jpg\",\"expected_size\":7,\
             \"home\":{\"mount\":{\"volume_id\":\"usb-1\"}},\"path\":\"photos/.cafe\u{0301}.jpg.cmdr-aside-1\"}",
        ),
        (
            item(
                ItemKind::Temp,
                ItemHome::VolumeSpace {
                    volume_id: "smb-testnas".to_string(),
                },
                "/srv/a\\b/c:d.cmdr-tmp-2",
            ),
            r#"{"kind":"temp","home":{"volume_space":{"volume_id":"smb-testnas"}},"path":"/srv/a\\b/c:d.cmdr-tmp-2"}"#,
        ),
        (
            Record::Legacy(RecordedTemp::Local(PathBuf::from("/tmp/x.cmdr-tmp-3"))),
            r#""/tmp/x.cmdr-tmp-3""#,
        ),
    ];
    for (record, line) in cases {
        assert_eq!(record.encode().as_deref(), Some(line));
        let decoded = match &record {
            Record::Tracked(_) => Record::Tracked(serde_json::from_str(line).expect("decodes")),
            Record::Legacy(_) => Record::Legacy(serde_json::from_str(line).expect("decodes")),
        };
        assert_eq!(decoded, record);
    }
}

/// A mount-homed record states its path relative to the mount root through
/// `Path::strip_prefix`, which matches by components, so a trailing `/` (or a `.`)
/// on the absolute path vanishes from the record, and `names` matches either
/// spelling. A path off the root is kept whole. Today's answers; the typed path
/// normalizes the same way, so the record stays the same. #1225.
#[test]
fn a_mount_record_states_its_path_by_components() {
    let home = RecordHome::mount("usb-1", Path::new("/Volumes/USB"));
    assert_eq!(home.record_path(Path::new("/Volumes/USB/a/b")), PathBuf::from("a/b"));
    assert_eq!(home.record_path(Path::new("/Volumes/USB/a/b/")), PathBuf::from("a/b"));
    assert_eq!(home.record_path(Path::new("/Volumes/USB/./a/b")), PathBuf::from("a/b"));
    assert_eq!(
        home.record_path(Path::new("/Volumes/USB-1/a")),
        PathBuf::from("/Volumes/USB-1/a")
    );

    let record = item(
        ItemKind::Temp,
        ItemHome::Mount {
            volume_id: "usb-1".to_string(),
        },
        "a/b",
    );
    let root = Path::new("/Volumes/USB");
    assert!(record.names("usb-1", root, Path::new("/Volumes/USB/a/b")));
    assert!(record.names("usb-1", root, Path::new("/Volumes/USB/a/b/")));
    assert!(!record.names("usb-1", root, Path::new("/Volumes/USB/a/b/c")));
    assert!(!record.names("usb-2", root, Path::new("/Volumes/USB/a/b")));
}

/// A volume-space record keeps the path as given, and matches by `PathBuf`'s
/// component equality, so `/a/b/` names the `/a/b` record. Today's answer. #1225.
#[test]
fn a_volume_space_record_matches_by_components() {
    let record = item(
        ItemKind::Temp,
        ItemHome::VolumeSpace {
            volume_id: "smb-testnas".to_string(),
        },
        "/srv/a/b",
    );
    let unused_root = Path::new("/");
    assert!(record.names("smb-testnas", unused_root, Path::new("/srv/a/b")));
    assert!(record.names("smb-testnas", unused_root, Path::new("/srv/a/b/")));
    assert!(record.names("smb-testnas", unused_root, Path::new("/srv//a/./b")));
    assert!(!record.names("smb-testnas", unused_root, Path::new("/srv/a/../a/b")));
}
