//! Where a top-level source lands (`top_level`), and the last name a root keeps
//! (`own_name`), byte for byte what `std::path` gave on `main`.

use super::*;
use cmdr_fs::app_path;
use cmdr_fs::app_path::PathFlavor;

/// A test's path text as the app path a pane holds on every host: canonical POSIX or
/// scheme text, a drive or UNC path, or (a scheme root's trailing `/`) the wire's reading.
fn parse(text: &str) -> AppPathBuf {
    AppPath::from_normalized(text)
        .or_else(|_| AppPath::from_normalized_as(text, PathFlavor::Windows))
        .map(ToOwned::to_owned)
        .or_else(|_| AppPathBuf::from_wire(text))
        .unwrap_or_else(|why| panic!("{text}: {why}"))
}

fn at(dest: &str, source: &str, chosen: Option<&OsStr>) -> TopLevel {
    top_level(&parse(dest), &parse(source), chosen)
}

fn landed(text: &str) -> TopLevel {
    TopLevel::At(parse(text))
}

/// ❗ A root keeps the last name `Path::file_name` read for it on `main`, so copying a
/// server's root into `/dst` still lands in `/dst/<server>`, ❌ never mixed into `/dst`
/// (and the progress tree, `TreeLanding::under`, predicts the same place).
#[test]
fn a_source_that_is_a_root_lands_under_the_name_std_path_gave_it() {
    assert_eq!(at("/dst", "sftp://u@h:22/", None), landed("/dst/u@h:22"));
    assert_eq!(at("/dst", "smb://nas/share/", None), landed("/dst/share"));
    assert_eq!(at("/dst", "/", None), landed("/dst"), "`/` has no name on any host");
    if cfg!(windows) {
        assert_eq!(at("/dst", "C:/", None), landed("/dst"));
        assert_eq!(at("/dst", "//server/share", None), landed("/dst"));
    } else {
        assert_eq!(at("/dst", "C:/", None), landed("/dst/C:"));
        assert_eq!(at("/dst", "//server/share", None), landed("/dst/share"));
    }
    assert_eq!(own_name(app_path!("/a/b.txt")).as_deref(), Some("b.txt"));
}

/// A chosen name is the person's pick: one that isn't text is asked about, ❌ never
/// swapped for the source's own name.
#[cfg(unix)]
#[test]
fn a_chosen_name_that_isnt_text_is_refused_not_dropped() {
    use std::os::unix::ffi::OsStrExt;
    let chosen = OsStr::from_bytes(b"caf\xe9.txt");
    assert!(
        matches!(at("/dst", "/src/a.txt", Some(chosen)), TopLevel::Refused(_)),
        "{:?}",
        at("/dst", "/src/a.txt", Some(chosen))
    );
    assert_eq!(
        at("/dst", "/src/a.txt", Some(OsStr::new("b.txt"))),
        landed("/dst/b.txt")
    );
}
