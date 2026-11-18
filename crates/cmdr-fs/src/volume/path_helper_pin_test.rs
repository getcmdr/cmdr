//! The string and `std::path` helpers app paths go through today, pinned on the edges
//! their own suites leave open: a trailing `/`, a `.` or `..` name, a doubled `/`, a
//! relative tail, and a scheme root. Each cell is today's answer on macOS and Linux,
//! including the ones that look wrong (each says so): converting a helper to the typed
//! app path must keep these answers, or flip one on purpose in its own commit.
//!
//! Unix only: on Windows these helpers run `std::path`'s Windows rules, the bug #1225
//! removes, so their Windows answers are the type's targets, not today's.
#![cfg(unix)]

use std::path::{Path, PathBuf};

use super::app_paths::{path_under, rebase};
use super::remote_paths::RemoteRoot;
use super::{ChildName, InMemoryVolume, path_volume_id, root_anchored};
use crate::entry::FileEntry;
use crate::twin_path;

fn anchor(root: &str, path: &str) -> PathBuf {
    root_anchored(Path::new(root), Path::new(path))
}

// ── `root_anchored` ──────────────────────────────────────────────────────

/// A trailing `/` survives anchoring: a path already under the root comes back as
/// given, and a relative one keeps its slash under the root. The OS call that follows
/// gets the slash too.
#[test]
fn root_anchored_keeps_a_trailing_slash_as_given() {
    assert_eq!(
        anchor("/Volumes/testshare", "/Volumes/testshare/photos/").as_os_str(),
        "/Volumes/testshare/photos/"
    );
    assert_eq!(
        anchor("/Volumes/testshare", "photos/").as_os_str(),
        "/Volumes/testshare/photos/"
    );
    // The root spelled with a slash is still `/`'s alias for the root.
    assert_eq!(anchor("/Volumes/testshare/", "/").as_os_str(), "/Volumes/testshare/");
}

/// ❗ Today's answer, pinned as such: a `..` is never resolved, so a path that names the
/// root and then climbs passes the component match and comes back untouched, off the
/// volume once the OS resolves it. The typed app path refuses a `..` at the boundary.
#[test]
fn root_anchored_passes_a_climbing_path_through_unresolved() {
    assert_eq!(
        anchor("/Volumes/testshare", "/Volumes/testshare/../etc").as_os_str(),
        "/Volumes/testshare/../etc"
    );
    assert_eq!(
        anchor("/Volumes/testshare", "../etc").as_os_str(),
        "/Volumes/testshare/../etc"
    );
    assert_eq!(
        anchor("/Volumes/testshare", "./x").as_os_str(),
        "/Volumes/testshare/./x"
    );
}

/// `Path`'s lenient equality decides the root aliases and the "already under" test:
/// `//` and `/./` read as `/`, and a root given with a trailing `/` still contains its
/// children.
#[test]
fn root_anchored_matches_the_root_component_wise() {
    assert_eq!(anchor("/Volumes/testshare", "//").as_os_str(), "/Volumes/testshare");
    assert_eq!(anchor("/Volumes/testshare", "/./").as_os_str(), "/Volumes/testshare");
    assert_eq!(
        anchor("/Volumes/testshare/", "/Volumes/testshare/x").as_os_str(),
        "/Volumes/testshare/x"
    );
    assert_eq!(
        anchor("/Volumes/testshare", "/Volumes//testshare/x").as_os_str(),
        "/Volumes//testshare/x",
        "a doubled slash still counts as under the root, and keeps its bytes"
    );
}

// ── `RemoteRoot` ─────────────────────────────────────────────────────────

const PREFIX: &str = "sftp://test_user_1@testnas.local:22";

fn rooted_at(remote_root: &str) -> RemoteRoot {
    RemoteRoot::new(PREFIX.to_string(), Path::new(remote_root))
}

/// The server path loses a trailing `/`, a doubled `/`, and `.` names: what the server
/// is sent is normalized whatever the app path carried.
#[test]
fn remote_path_drops_trailing_and_doubled_slashes_and_dots() {
    let root = rooted_at("/srv/data");
    let remote = |path: &str| root.to_remote_path(Path::new(path));
    assert_eq!(
        remote(&format!("{PREFIX}/srv/data/photos/")).as_deref(),
        Some("/srv/data/photos")
    );
    assert_eq!(
        remote(&format!("{PREFIX}/srv//data/./photos")).as_deref(),
        Some("/srv/data/photos")
    );
    assert_eq!(remote("photos/").as_deref(), Some("/srv/data/photos"));
    assert_eq!(remote("./photos/./2026").as_deref(), Some("/srv/data/photos/2026"));
    assert_eq!(remote("photos/../videos").as_deref(), Some("/srv/data/videos"));
}

/// A server answer with a trailing `/`, a doubled `/`, or a `.` comes back as one app
/// spelling, the prefix plus the normalized server path.
#[test]
fn app_path_of_a_server_answer_is_normalized() {
    let root = rooted_at("/srv/data");
    let app = |remote: &str| root.to_app_path(remote);
    assert_eq!(
        app("/srv/data/photos/").as_deref(),
        Some(Path::new(&format!("{PREFIX}/srv/data/photos")))
    );
    assert_eq!(
        app("/srv//data/./photos").as_deref(),
        Some(Path::new(&format!("{PREFIX}/srv/data/photos")))
    );
    assert_eq!(
        app("srv/data/photos").as_deref(),
        Some(Path::new(&format!("{PREFIX}/srv/data/photos")))
    );
    assert_eq!(
        app("/srv/data").as_deref(),
        Some(Path::new(&format!("{PREFIX}/srv/data")))
    );
}

/// A root given with a trailing `/` or `..` is normalized once, at construction, and the
/// app root is the prefix plus that spelling.
#[test]
fn remote_root_normalizes_its_own_root_once() {
    for given in ["/srv/data/", "/srv/./data", "/srv/x/../data", "srv/data"] {
        let root = rooted_at(given);
        assert_eq!(root.remote_root(), Path::new("/srv/data"), "{given:?}");
        assert_eq!(root.app_root().as_os_str(), &*format!("{PREFIX}/srv/data"), "{given:?}");
    }
}

// ── `app_paths` ──────────────────────────────────────────────────────────

/// ❗ Today's answer, pinned as such: `path_under` is lexical and never resolves `..`, so
/// a climbing path counts as under its root and the tail carries the `..` on. A doubled
/// `/` after the root is trimmed off the tail.
#[test]
fn path_under_passes_dot_dot_and_trims_a_doubled_slash() {
    assert_eq!(
        path_under("/Volumes/testshare/../etc", "/Volumes/testshare"),
        Some("../etc")
    );
    assert_eq!(
        path_under("/Volumes/testshare//docs", "/Volumes/testshare"),
        Some("docs")
    );
    assert_eq!(
        path_under("/Volumes/testshare/./docs", "/Volumes/testshare"),
        Some("./docs")
    );
    assert_eq!(
        path_under("/Volumes/testshare/docs//", "/Volumes/testshare"),
        Some("docs")
    );
    assert_eq!(
        rebase(
            "/Volumes/testshare/../etc",
            "/Volumes/testshare",
            "/Volumes/testshare-1"
        )
        .as_deref(),
        Some("/Volumes/testshare-1/../etc")
    );
}

/// A doubled `/` INSIDE the root's spelling is a different string, so it isn't under.
#[test]
fn path_under_compares_the_root_bytewise_up_to_a_trailing_slash() {
    assert_eq!(path_under("/Volumes//testshare/docs", "/Volumes/testshare"), None);
    assert_eq!(path_under("sftp://u@h:22/srv/x", "sftp://u@h:22/srv/"), Some("x"));
    assert_eq!(
        rebase("sftp://u@h:22/srv/x", "sftp://u@h:22/srv/", "sftp://u@h:22/").as_deref(),
        Some("sftp://u@h:22/x")
    );
}

// ── `twin_path` ──────────────────────────────────────────────────────────

/// The folder of a twin row steps over the pick on a scheme path too, and a pick right
/// under the storage root steps back to the storage.
#[test]
fn folder_of_steps_over_a_pick_under_a_scheme_root() {
    let twin = format!("mtp://pixel/65537/{}/IMG_1.JPG", twin_path::pick_segment(4242));
    assert_eq!(
        twin_path::folder_of(Path::new(&twin)),
        Some(Path::new("mtp://pixel/65537"))
    );
    assert_eq!(twin_path::shown(&twin), "mtp://pixel/65537/IMG_1.JPG");
}

/// ❗ Today's answer, pinned as such: `folder_of` is `Path::parent`, so it climbs above
/// a scheme volume's root (`mtp://pixel/65537` to `mtp://pixel`, then `mtp:`). The
/// typed app path's `parent` stops at the root.
#[test]
fn folder_of_climbs_above_a_scheme_root() {
    assert_eq!(
        twin_path::folder_of(Path::new("mtp://pixel/65537")),
        Some(Path::new("mtp://pixel"))
    );
    assert_eq!(twin_path::folder_of(Path::new("mtp://pixel")), Some(Path::new("mtp:")));
    assert_eq!(twin_path::folder_of(Path::new("/DCIM/")), Some(Path::new("/")));
}

// ── `ChildName` ──────────────────────────────────────────────────────────

/// What counts as one plain name today: never empty, `.`, `..`, a `/`, or a NUL; a `\`,
/// a `:`, and a drive-shaped `C:` are ordinary name characters.
#[test]
fn child_name_takes_backslash_and_colon_and_refuses_dots_and_slashes() {
    for refused in ["", ".", "..", "a/b", "/a", "a/", "a\0b"] {
        assert!(ChildName::new(refused).is_err(), "{refused:?}");
    }
    for plain in ["a\\b", "a:b", "C:", "...", ".hidden", " a "] {
        assert!(ChildName::new(plain).is_ok(), "{plain:?}");
    }
}

/// `under` is `Path::join`: a folder's trailing `/` doesn't double, a scheme folder takes
/// the name after a `/`, and a `C:`-shaped name stays a name.
#[test]
fn child_name_under_joins_one_name_below_the_folder() {
    let under = |dir: &str, name: &str| ChildName::new(name).expect("plain").under(Path::new(dir));
    assert_eq!(under("/x/", "a").as_os_str(), "/x/a");
    assert_eq!(under("/x", "a").as_os_str(), "/x/a");
    assert_eq!(under("/", "a").as_os_str(), "/a");
    assert_eq!(under("mtp://pixel/65537", "DCIM").as_os_str(), "mtp://pixel/65537/DCIM");
    assert_eq!(
        under("sftp://test_user_1@nas:22/", "srv").as_os_str(),
        "sftp://test_user_1@nas:22/srv"
    );
    assert_eq!(under("/x", "C:").as_os_str(), "/x/C:");
    assert_eq!(under("/x/.", "a").as_os_str(), "/x/./a", "a `.` in the folder stays");
}

// ── `volume::ids` ────────────────────────────────────────────────────────

/// ❗ A mount path goes into the ID verbatim, so a trailing `/` makes a different ID.
/// Every caller hands the kernel's spelling, which has none, so normalizing a mount path
/// before this call would MOVE every saved ID: keep the bytes.
#[test]
fn a_mount_paths_trailing_slash_makes_a_different_id() {
    assert_ne!(
        path_volume_id("/Volumes/testshare"),
        path_volume_id("/Volumes/testshare/")
    );
    assert_ne!(
        path_volume_id("/Volumes/testshare"),
        path_volume_id("/Volumes//testshare")
    );
    assert_eq!(path_volume_id("/"), super::DEFAULT_VOLUME_ID);
    assert_ne!(
        path_volume_id("//"),
        super::DEFAULT_VOLUME_ID,
        "only `/` itself is the root's ID"
    );
}

// ── `InMemoryVolume` ─────────────────────────────────────────────────────

fn dir(path: &str) -> FileEntry {
    FileEntry::new(String::new(), path.to_string(), true, false)
}

fn link(path: &str) -> FileEntry {
    FileEntry::new(String::new(), path.to_string(), true, true)
}

/// `canonicalize` reads `..` after a link PHYSICALLY, the way `realpath` does: up from
/// the link's target, not from the folder holding the link. A relative input is read
/// from `/`.
#[test]
fn canonicalize_takes_dot_dot_up_from_a_links_target() {
    let volume = InMemoryVolume::with_entries(
        "Mem",
        vec![dir("/real"), dir("/real/inner"), dir("/other"), link("/other/l")],
    )
    .with_link_target(Path::new("/other/l"), Path::new("/real/inner"));
    let resolved = |path: &str| volume.canonicalize(Path::new(path));
    assert_eq!(resolved("/other/l/.."), Some(PathBuf::from("/real")));
    assert_eq!(resolved("/other/l/../inner"), Some(PathBuf::from("/real/inner")));
    assert_eq!(resolved("other/l"), Some(PathBuf::from("/real/inner")));
    assert_eq!(resolved("//other//l/"), Some(PathBuf::from("/real/inner")));
}

/// ❗ Today's answer, pinned as such: `canonicalize` walks from `/`, so a scheme-rooted
/// volume's paths never resolve, even ones it holds.
#[test]
fn canonicalize_never_resolves_a_scheme_path() {
    let volume = InMemoryVolume::with_entries("Mem", vec![dir("mtp://pixel/65537/DCIM")]);
    assert_eq!(volume.canonicalize(Path::new("mtp://pixel/65537/DCIM")), None);
}
