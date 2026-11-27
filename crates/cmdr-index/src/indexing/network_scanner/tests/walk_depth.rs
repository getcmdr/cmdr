//! The depth backstop every network walk shares (`walk_descends`).
//!
//! Samba lists a folder link as a plain folder, so `loop -> ..` on a share looks
//! like any folder and the walk queues the same folder one level deeper, lap after
//! lap. The listings these walks get carry no node id to catch it by, so depth
//! is what ends it: `cmdr_fs::volume::descent::MAX_WALK_DEPTH` levels below the
//! volume's root.

use super::*;
use cmdr_fs::volume::descent::MAX_WALK_DEPTH;

fn folder_at_depth(depth: usize) -> AppPathBuf {
    let text = format!("/{}", vec!["loop"; depth].join("/"));
    AppPathBuf::parse(&text, cmdr_fs::app_path::PathFlavor::Posix).expect("an absolute app path")
}

#[test]
fn a_walk_stops_past_the_depth_backstop() {
    let vol = InMemoryVolume::with_entries("Test", vec![]);

    assert!(walk_descends(&vol, &folder_at_depth(MAX_WALK_DEPTH), "loop", false));
    assert!(
        !walk_descends(&vol, &folder_at_depth(MAX_WALK_DEPTH + 1), "loop", false),
        "a folder past the backstop is listed, never walked"
    );
}
