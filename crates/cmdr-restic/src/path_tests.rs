use super::{SnapshotRef, VIRTUAL_ENTRY_NAME, VirtualPath, child_text, classify, restic_route, short_id_of};
use crate::test_fixtures::{app, path_in as at};
use cmdr_fs::app_path::AppPathBuf;
use std::path::{Path, PathBuf};

const VIEW: &str = "/Volumes/testshare/backups/test-restic/Browse snapshots";

fn inner(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn view() -> AppPathBuf {
    app(VIEW)
}

/// A path spelled as only a transition-bridge value spells one (relative, a
/// `..`, a trailing `/`), the shapes the `std::path` forms still hand in.
fn bridge_value(text: &str) -> AppPathBuf {
    crate::hub::bridged(Path::new(text)).into_owned()
}

#[test]
fn the_route_is_the_folder_holding_the_first_browse_snapshots_component() {
    assert_eq!(
        restic_route(Path::new(
            "/Volumes/testshare/backups/test-restic/Browse snapshots/latest/Users"
        )),
        Some(PathBuf::from("/Volumes/testshare/backups/test-restic"))
    );
    assert_eq!(
        restic_route(Path::new(VIEW)),
        Some(PathBuf::from("/Volumes/testshare/backups/test-restic"))
    );
    // A backed-up repo inside a snapshot still routes at the FIRST boundary: the
    // outer repo serves everything below it.
    assert_eq!(
        restic_route(Path::new("/r/Browse snapshots/latest/backup/Browse snapshots/x")),
        Some(PathBuf::from("/r"))
    );
}

#[test]
fn a_path_without_the_component_never_routes() {
    assert_eq!(
        restic_route(Path::new("/Volumes/testshare/backups/test-restic/snapshots")),
        None
    );
    // A near miss is a different name, not a route.
    assert_eq!(restic_route(Path::new("/r/browse snapshots/latest")), None);
    assert_eq!(restic_route(Path::new("/r/Browse snapshots.txt")), None);
    assert_eq!(restic_route(Path::new("/")), None);
}

#[test]
fn the_view_root_classifies_as_the_snapshot_list() {
    assert_eq!(classify(&view(), &view()), Some(VirtualPath::Root));
    assert_eq!(classify(&view(), &bridge_value("")), Some(VirtualPath::Root));
}

#[test]
fn latest_and_its_contents_classify_as_latest() {
    assert_eq!(
        classify(&view(), &at(&view(), "latest")),
        Some(VirtualPath::Snapshot(SnapshotRef::Latest, Vec::new()))
    );
    assert_eq!(
        classify(&view(), &at(&view(), "latest/Users/test_user_3")),
        Some(VirtualPath::Snapshot(
            SnapshotRef::Latest,
            inner(&["Users", "test_user_3"])
        ))
    );
}

#[test]
fn a_dated_folder_classifies_by_its_short_id_alone() {
    let path = at(&view(), "2026-10-12 21.00.03 (a1b2c3d4)/Users");
    assert_eq!(
        classify(&view(), &path),
        Some(VirtualPath::Snapshot(
            SnapshotRef::ShortId("a1b2c3d4".to_string()),
            inner(&["Users"])
        ))
    );
}

/// ❗ The time prefix is display only. A favorite saved before a time zone change
/// (or across DST) still names the same snapshot by its id.
#[test]
fn a_stale_time_prefix_still_classifies_by_the_id() {
    let stale = at(&view(), "2026-10-12 19.00.03 (a1b2c3d4)");
    assert_eq!(
        classify(&view(), &stale),
        Some(VirtualPath::Snapshot(
            SnapshotRef::ShortId("a1b2c3d4".to_string()),
            Vec::new()
        ))
    );
}

#[test]
fn a_relative_path_is_read_against_the_view_root() {
    assert_eq!(
        classify(&view(), &bridge_value("latest/a.txt")),
        Some(VirtualPath::Snapshot(SnapshotRef::Latest, inner(&["a.txt"])))
    );
}

/// A name a Linux backup stores with `\` or `:` is one name on every OS: the
/// app path splits on `/` alone.
#[test]
fn a_name_holding_a_backslash_or_a_colon_is_one_name() {
    assert_eq!(
        classify(&view(), &at(&view(), "latest/a\\b/12:00 notes")),
        Some(VirtualPath::Snapshot(
            SnapshotRef::Latest,
            inner(&["a\\b", "12:00 notes"])
        ))
    );
}

#[test]
fn names_that_name_no_snapshot_classify_as_nothing() {
    for name in [
        "snapshots",
        "2026-10-12 21.00.03",
        "2026-10-12 21.00.03 ()",
        "2026-10-12 21.00.03 (a1b2c3)",
        "2026-10-12 21.00.03 (a1b2c3dz)",
        "2026-10-12 21.00.03 (A1B2C3D4)",
        "Latest",
    ] {
        assert_eq!(
            classify(&view(), &at(&view(), name)),
            None,
            "{name:?} names no snapshot"
        );
    }
    assert_eq!(classify(&view(), &app("/elsewhere/latest")), None);
    assert_eq!(classify(&view(), &bridge_value(&format!("{VIEW}/latest/../x"))), None);
}

#[test]
fn the_short_id_is_the_trailing_hex_in_parentheses() {
    assert_eq!(short_id_of("2026-10-12 21.00.03 (a1b2c3d4)"), Some("a1b2c3d4"));
    assert_eq!(short_id_of("2026-10-12 21.00.03 (a1b2c3d4e5)"), Some("a1b2c3d4e5"));
    assert_eq!(short_id_of("(a1b2c3d4)"), Some("a1b2c3d4"));
    assert_eq!(short_id_of("2026-10-12 21.00.03 (a1b2c3d4) "), None);
    assert_eq!(short_id_of("a1b2c3d4"), None);
}

#[test]
fn the_component_name_is_stable() {
    // A path component, so it stays English in every locale (history, favorites,
    // and the agent all address paths).
    assert_eq!(VIRTUAL_ENTRY_NAME, "Browse snapshots");
}

/// A row's text under a canonical folder is the app path's join; under a
/// bridge folder, and for a name a hostile repo stores, it's the text
/// `Path::join` made on macOS and Linux.
#[test]
fn a_row_joins_its_folder_as_the_path_join_did() {
    assert_eq!(child_text(&view(), "latest"), format!("{VIEW}/latest"));
    assert_eq!(child_text(&app("/"), "a"), "/a");
    assert_eq!(child_text(&bridge_value("/r/"), "a"), "/r/a");
    assert_eq!(child_text(&bridge_value("smb:/h/s/r"), "a"), "smb:/h/s/r/a");
    assert_eq!(child_text(&view(), "a/b"), format!("{VIEW}/a/b"));
    assert_eq!(child_text(&view(), ".."), format!("{VIEW}/.."));
    assert_eq!(child_text(&view(), "/etc"), "/etc");
}
