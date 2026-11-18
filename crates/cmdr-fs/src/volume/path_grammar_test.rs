//! The app-path grammar, driven by the case table the frontend's suite reads too
//! (`path_grammar_cases.json`, against `apps/desktop/src/lib/path/grammar.ts`), so the two
//! languages can't drift apart. A new case goes into that file, ❌ never into one suite.

use super::*;

#[derive(serde::Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
enum Flavor {
    Posix,
    Windows,
}

impl From<Flavor> for PathFlavor {
    fn from(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Posix => PathFlavor::Posix,
            Flavor::Windows => PathFlavor::Windows,
        }
    }
}

#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Posix,
    Drive,
    Unc,
    Scheme,
}

impl From<PathKind> for Kind {
    fn from(kind: PathKind) -> Self {
        match kind {
            PathKind::Posix => Kind::Posix,
            PathKind::Drive => Kind::Drive,
            PathKind::Unc => Kind::Unc,
            PathKind::Scheme => Kind::Scheme,
        }
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Case {
    path: String,
    flavor: Flavor,
    kind: Option<Kind>,
    root: Option<String>,
    normalized: Option<String>,
    is_root: Option<bool>,
    parent: Option<String>,
    basename: Option<String>,
    segments: Option<Vec<String>>,
    shown: String,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct UserInputCase {
    text: String,
    flavor: Flavor,
    wire: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    cases: Vec<Case>,
    user_input: Vec<UserInputCase>,
}

fn table() -> Table {
    serde_json::from_str(include_str!("path_grammar_cases.json")).expect("the shared cases parse")
}

#[test]
fn every_shared_case_holds() {
    for case in table().cases {
        let flavor = PathFlavor::from(case.flavor);
        let what = format!("{:?} {:?}", case.flavor, case.path);
        assert_eq!(
            path_kind(&case.path, flavor).map(Kind::from),
            case.kind,
            "kind of {what}"
        );
        assert_eq!(root_of(&case.path, flavor), case.root, "root of {what}");
        assert_eq!(normalize_path(&case.path, flavor), case.normalized, "normalized {what}");
        assert_eq!(names_of(&case.path, flavor), case.segments, "segments of {what}");
        if let Some(is_root) = case.is_root {
            assert_eq!(
                names_of(&case.path, flavor).is_some_and(|names| names.is_empty()),
                is_root,
                "root-ness of {what}"
            );
        }
        assert_eq!(parent_in_volume(&case.path, flavor), case.parent, "parent of {what}");
        assert_eq!(
            basename_in_volume(&case.path, flavor),
            case.basename,
            "basename of {what}"
        );
        assert_eq!(shown_path(&case.path, flavor), case.shown, "shown {what}");
    }
}

#[test]
fn every_shared_user_input_case_holds() {
    for case in table().user_input {
        let flavor = PathFlavor::from(case.flavor);
        assert_eq!(
            from_user_input(&case.text, flavor),
            case.wire,
            "{:?} {:?}",
            case.flavor,
            case.text
        );
    }
}

#[test]
fn the_verbatim_form_is_the_long_path_one() {
    assert_eq!(
        verbatim_lexically("C:/Users/test_user_1").as_deref(),
        Some(r"\\?\C:\Users\test_user_1")
    );
    assert_eq!(verbatim_lexically("C:/").as_deref(), Some(r"\\?\C:\"));
    assert_eq!(
        verbatim_lexically("//nas/media/films/a.mkv").as_deref(),
        Some(r"\\?\UNC\nas\media\films\a.mkv")
    );
    assert_eq!(
        verbatim_lexically("//nas/media").as_deref(),
        Some(r"\\?\UNC\nas\media\")
    );
    assert_eq!(verbatim_lexically("/Users/x"), None);
}

#[test]
fn native_to_app_takes_every_windows_spelling_to_one_app_path() {
    for native in [
        r"\\?\C:\Users\test_user_1",
        r"C:\Users\test_user_1",
        "C:/Users/test_user_1",
        r"c:\Users\test_user_1\",
    ] {
        assert_eq!(
            native_to_app(native, PathFlavor::Windows).as_deref(),
            Some("C:/Users/test_user_1"),
            "{native}"
        );
    }
    for native in [r"\\?\UNC\nas\media\x", r"\\nas\media\x", "//nas/media/x"] {
        assert_eq!(
            native_to_app(native, PathFlavor::Windows).as_deref(),
            Some("//nas/media/x"),
            "{native}"
        );
    }
}

#[test]
fn app_spelling_turns_a_native_windows_path_and_keeps_what_it_cant_read() {
    assert_eq!(
        app_spelling(r"C:\Users\test_user_1\Desktop", PathFlavor::Windows),
        "C:/Users/test_user_1/Desktop"
    );
    assert_eq!(
        app_spelling(r"\\.\PhysicalDrive0", PathFlavor::Windows),
        r"\\.\PhysicalDrive0"
    );
    assert_eq!(
        app_spelling("/Users/test_user_1", PathFlavor::Posix),
        "/Users/test_user_1"
    );
    assert_eq!(app_spelling(r"/tmp/a\b", PathFlavor::Posix), r"/tmp/a\b");
}

#[test]
fn native_to_app_refuses_what_names_no_folder() {
    for native in [
        r"\\.\PhysicalDrive0",
        r"\\?\Volume{0b1c2d3e-0000-0000-0000-100000000000}\",
        "Users",
        r"\x",
        "",
    ] {
        assert_eq!(native_to_app(native, PathFlavor::Windows), None, "{native}");
    }
}

#[test]
fn a_lexical_join_never_leaves_the_volume() {
    let w = PathFlavor::Windows;
    assert_eq!(join_lexically("C:/", "../../Windows", w).as_deref(), Some("C:/Windows"));
    assert_eq!(
        join_lexically("//nas/media/x", "../..", w).as_deref(),
        Some("//nas/media/")
    );
    assert_eq!(
        join_lexically("/Users", "test_user_1/./notes.txt", PathFlavor::Posix).as_deref(),
        Some("/Users/test_user_1/notes.txt")
    );
    assert_eq!(parent_in_volume("/Users", PathFlavor::Posix).as_deref(), Some("/"));
    assert_eq!(parent_in_volume("C:/", w), None);
}

#[test]
fn the_host_flavor_follows_the_build_target() {
    // The standard library's own per-target answer, so the test names no OS itself.
    let expected = if std::path::MAIN_SEPARATOR == '\\' {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    };
    assert_eq!(PathFlavor::HOST, expected);
}
