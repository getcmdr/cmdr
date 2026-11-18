//! The server-free half of following a DFS link: which listings claim a watch
//! once a call followed a link. The calls themselves need smb2's DFS fixtures
//! (`dfs_links_integration_test.rs`).

use super::*;
use crate::volume::test_support::*;
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::Path;

#[test]
fn a_followed_path_covers_itself_and_what_is_below_it_by_whole_components() {
    let paths = LinkPaths::default();
    paths.note("data/sub");
    assert!(paths.covers("data/sub"));
    assert!(paths.covers("data/sub/deeper/file.txt"));
    assert!(!paths.covers("data"), "the link's parent is the share's own");
    assert!(!paths.covers("data/sub2"), "a sibling sharing a name prefix");
    assert!(!paths.covers(""));
}

#[test]
fn a_followed_path_above_a_remembered_one_replaces_it() {
    let paths = LinkPaths::default();
    paths.note("data/a/b");
    paths.note("data/c");
    paths.note("data/a/b/c");
    paths.note("data");
    assert_eq!(*paths.0.lock().unwrap(), vec!["data".to_string()]);
}

#[test]
fn a_listing_behind_a_followed_link_claims_no_watch() {
    let vol = make_test_volume_direct();
    let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
    *vol.inner.watcher_cancel.lock().unwrap() = Some(tx);
    vol.inner.link_paths.note("data");

    assert_eq!(vol.listing_watch_coverage(app_path!("/")), WatchCoverage::EveryWriter);
    assert_eq!(
        vol.listing_watch_coverage(&AppPath::legacy_unchecked(Path::new("data"))),
        WatchCoverage::None
    );
    assert_eq!(
        vol.listing_watch_coverage(&AppPath::legacy_unchecked(Path::new("data/sub"))),
        WatchCoverage::None
    );
    assert_eq!(
        vol.listing_watch_coverage(&AppPath::legacy_unchecked(Path::new("database"))),
        WatchCoverage::EveryWriter
    );
}
