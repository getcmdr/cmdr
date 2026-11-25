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

fn at(dest: &str, source: &str, chosen: Option<&str>) -> TopLevel {
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

/// A chosen name is the person's pick, read as ONE name by the app path's rules on every
/// host: a POSIX folder holds `a\b.txt` whole (on Windows `std::path` split it), and a
/// name that's no plain name is asked about, ❌ never swapped for the source's own name,
/// ❌ never replacing the base.
#[test]
fn a_chosen_name_lands_as_one_name_or_is_asked_about() {
    for one_name in ["b.txt", "a\\b.txt", "foo\\..", "..\\bar", "a:b", "x.", "x ", "C:\\x"] {
        assert_eq!(
            at("/dst", "/src/a.txt", Some(one_name)),
            TopLevel::At(
                parse("/dst")
                    .join(AppName::from_app_spelling(one_name).expect("one name"))
                    .expect("joins")
            ),
            "{one_name:?}"
        );
    }
    for not_a_name in ["", ".", "..", "/etc/x", "a/b"] {
        assert!(
            matches!(at("/dst", "/src/a.txt", Some(not_a_name)), TopLevel::Refused(_)),
            "{not_a_name:?}"
        );
    }
    // A drive folder can't hold a `\`: asked about, per item.
    assert!(matches!(
        at("C:/dst", "/src/a.txt", Some("a\\b.txt")),
        TopLevel::Refused(_)
    ));
    assert_eq!(at("/dst", "/src/a.txt", Some("b.txt")), landed("/dst/b.txt"));
}
