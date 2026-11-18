//! The app path's contract: the shared case table (both flavors, every OS), the native
//! boundary's typed refusals, and the properties that make it safe to put everywhere.
//! Property tests live in `properties_test.rs`.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use super::*;
use crate::volume::path_grammar;

#[path = "properties_test.rs"]
mod properties_test;

fn posix(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Posix).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn win(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Windows).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn name(text: &str) -> &AppName {
    AppName::new(text).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

// ---- The shared case table ----

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

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Case {
    path: String,
    flavor: Flavor,
    kind: Option<String>,
    normalized: Option<String>,
    basename: Option<String>,
    parent: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct EscapeCase {
    path: String,
    flavor: Flavor,
    normalized: Option<String>,
    refusal: Option<String>,
    parent: Option<String>,
    basename: Option<String>,
    join_name: Option<String>,
    joined: Option<String>,
    shown: Option<String>,
    shown_name: Option<String>,
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
    escapes: Vec<EscapeCase>,
    escaped_user_input: Vec<UserInputCase>,
}

fn table() -> Table {
    serde_json::from_str(include_str!("../volume/path_grammar_cases.json")).expect("the shared cases parse")
}

/// Every grammar row, read by the typed path: no app path stays no app path, a `..` is
/// refused (the grammar's text helpers resolve it lexically; the type never does), and
/// everything else parses to the grammar's normalized spelling, with the same last name and
/// (below a local root) the same parent.
#[test]
fn every_grammar_row_holds_for_the_typed_path() {
    for case in table().cases {
        let flavor = PathFlavor::from(case.flavor);
        let what = format!("{:?} {:?}", case.flavor, case.path);
        let typed = AppPathBuf::parse(&case.path, flavor);
        let Some(normalized) = case.normalized else {
            assert!(typed.is_err(), "{what} is no app path, typed {typed:?}");
            continue;
        };
        if case.path.split('/').any(|name| name == "..") {
            assert_eq!(typed, Err(NotAnAppPath::DotName), "{what}");
            continue;
        }
        let typed = typed.unwrap_or_else(|e| panic!("{what}: {e}"));
        if case.kind.as_deref() == Some("scheme") {
            assert!(
                typed.as_str() == normalized || typed.as_str() == format!("{normalized}/"),
                "{what}"
            );
            continue;
        }
        assert_eq!(typed.as_str(), normalized, "{what}");
        assert_eq!(
            AppPath::from_normalized(&normalized).map(AppPath::as_str),
            Ok(normalized.as_str()),
            "{what}"
        );
        assert_eq!(
            typed.file_name().unwrap_or(""),
            case.basename.as_deref().unwrap_or_default(),
            "basename of {what}"
        );
        assert_eq!(
            typed.parent().map(AppPath::as_str),
            case.parent.as_deref(),
            "parent of {what}"
        );
        assert_eq!(
            typed.shown(),
            path_grammar::shown_path(&normalized, flavor),
            "shown {what}"
        );
    }
}

#[test]
fn every_escape_row_holds() {
    for case in table().escapes {
        let flavor = PathFlavor::from(case.flavor);
        let what = format!("{:?} {:?}", case.flavor, case.path);
        let parsed = AppPathBuf::parse(&case.path, flavor);
        match (&case.normalized, &case.refusal) {
            (Some(normalized), None) => {
                assert_eq!(parsed.as_ref().map(|p| p.as_str()), Ok(normalized.as_str()), "{what}");
            }
            (None, Some(refusal)) => {
                assert_eq!(refusal, "mixedEscape", "{what}: a refusal the test knows");
                assert_eq!(parsed, Err(NotAnAppPath::MixedEscape), "{what}");
                assert_eq!(
                    AppPath::from_normalized(&case.path),
                    Err(NotAnAppPath::MixedEscape),
                    "{what}"
                );
                continue;
            }
            _ => panic!("{what}: a row has a normalized form or a refusal"),
        }
        let path = parsed.expect("checked above");
        let canonical = case.normalized.as_deref() == Some(case.path.as_str());
        assert_eq!(AppPath::from_normalized(&case.path).is_ok(), canonical, "{what}");
        if let Some(parent) = &case.parent {
            assert_eq!(
                path.parent().map(AppPath::as_str),
                Some(parent.as_str()),
                "parent of {what}"
            );
            assert_eq!(
                path_grammar::parent_in_volume(&case.path, flavor).as_ref(),
                Some(parent),
                "{what}"
            );
        }
        if let Some(basename) = &case.basename {
            assert_eq!(path.file_name(), Some(basename.as_str()), "basename of {what}");
            assert_eq!(
                path_grammar::basename_in_volume(&case.path, flavor).as_ref(),
                Some(basename),
                "{what}"
            );
        }
        if let (Some(join_name), Some(joined)) = (&case.join_name, &case.joined) {
            assert_eq!(
                path.join(name(join_name)).map(AppPathBuf::into_string),
                Ok(joined.clone()),
                "{what}"
            );
        }
        if let Some(shown) = &case.shown {
            assert_eq!(path.shown(), shown.as_str(), "shown {what}");
        }
        if let Some(shown_name) = &case.shown_name {
            assert_eq!(
                path.shown_name().as_deref(),
                Some(shown_name.as_str()),
                "shown name of {what}"
            );
        }
    }
}

#[test]
fn every_escaped_user_input_row_holds() {
    for case in table().escaped_user_input {
        let typed = AppPathBuf::from_user_input(&case.text, PathFlavor::from(case.flavor));
        assert_eq!(typed.map(AppPathBuf::into_string), Ok(case.wire.clone()), "{case:?}");
    }
}

// ---- Arithmetic ----

#[test]
fn parent_join_and_file_name_work_on_every_root() {
    for (text, flavor, parent, file) in [
        ("/a/b", PathFlavor::Posix, Some("/a"), Some("b")),
        ("/a", PathFlavor::Posix, Some("/"), Some("a")),
        ("/", PathFlavor::Posix, None, None),
        ("C:/x", PathFlavor::Windows, Some("C:/"), Some("x")),
        ("C:/", PathFlavor::Windows, None, None),
        ("//srv/share/x/y", PathFlavor::Windows, Some("//srv/share/x"), Some("y")),
        ("//srv/share/x", PathFlavor::Windows, Some("//srv/share/"), Some("x")),
        ("//srv/share/", PathFlavor::Windows, None, None),
        (
            "sftp://u@h:22/srv/x",
            PathFlavor::Posix,
            Some("sftp://u@h:22/srv"),
            Some("x"),
        ),
        (
            "sftp://u@h:22/srv",
            PathFlavor::Windows,
            Some("sftp://u@h:22/"),
            Some("srv"),
        ),
        ("sftp://u@h:22/", PathFlavor::Posix, None, None),
        (
            "nfs://h/%2Fexport/x",
            PathFlavor::Posix,
            Some("nfs://h/%2Fexport"),
            Some("x"),
        ),
        ("scheme:///x", PathFlavor::Posix, Some("scheme:///"), Some("x")),
        ("/x/a.zip/inner", PathFlavor::Posix, Some("/x/a.zip"), Some("inner")),
    ] {
        let path = AppPathBuf::parse(text, flavor).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(path.as_str(), text);
        assert_eq!(path.parent().map(AppPath::as_str), parent, "parent of {text}");
        assert_eq!(path.file_name(), file, "file name of {text}");
        assert_eq!(path.is_root(), parent.is_none(), "{text}");
        let child = path.join(name("n")).expect("a plain name joins");
        assert_eq!(child.parent(), Some(path.as_app_path()), "{text}");
        assert_eq!(child.file_name(), Some("n"));
    }
}

#[test]
fn a_pick_segment_is_a_name_and_the_parent_answers_the_pick() {
    let row = posix("/DCIM/\u{0}4242/IMG_1.JPG");
    assert_eq!(row.parent().map(AppPath::as_str), Some("/DCIM/\u{0}4242"));
    assert_eq!(row.names().collect::<Vec<_>>(), ["DCIM", "\u{0}4242", "IMG_1.JPG"]);
    assert_eq!(row.shown(), "/DCIM/IMG_1.JPG");
    for bad in ["/a\u{0}b", "/\u{0}", "/\u{0}x1"] {
        assert_eq!(
            AppPathBuf::parse(bad, PathFlavor::Posix),
            Err(NotAnAppPath::Nul),
            "{bad:?}"
        );
    }
    assert_eq!(row.to_native_posix(), Err(NativeRefusal::ForbiddenCharacter));
}

#[test]
fn backslash_and_colon_are_name_characters_on_posix_and_in_scheme_paths() {
    let path = posix("/a\\b/c:d");
    assert_eq!(path.names().collect::<Vec<_>>(), ["a\\b", "c:d"]);
    assert_eq!(path.file_name(), Some("c:d"));
    assert!(AppPathBuf::parse("sftp://h/a\\b", PathFlavor::Windows).is_ok());
    assert_eq!(win("C:/x").join(name("a\\b")), Err(NotAnAppPath::Backslash));
    assert_eq!(win("//s/sh/").join(name("a\\b")), Err(NotAnAppPath::Backslash));
}

#[test]
fn starts_with_and_strip_prefix_match_whole_names() {
    let base = posix("/Volumes/share");
    assert!(posix("/Volumes/share/x").starts_with(&base));
    assert!(posix("/Volumes/share").starts_with(&base));
    assert!(!posix("/Volumes/share-1/x").starts_with(&base));
    assert!(posix("/x").starts_with(&posix("/")));
    assert_eq!(
        posix("/Volumes/share/a/b")
            .strip_prefix(&base)
            .map(RelativeAppPath::as_str),
        Some("a/b")
    );
    assert_eq!(
        posix("/Volumes/share").strip_prefix(&base).map(RelativeAppPath::as_str),
        Some("")
    );
    assert_eq!(
        posix("/a/b").strip_prefix(&posix("/")).map(RelativeAppPath::as_str),
        Some("a/b")
    );
    let sftp = posix("sftp://u@h:22/srv/data");
    assert!(!posix("sftp://u@h:2222/srv/data/x").starts_with(&sftp));
    assert!(!posix("sftp://u@h:22/srv/data-1").starts_with(&sftp));
    assert!(!win("D:/Users").starts_with(&win("C:/")));
    assert!(!win("//nas/media-1/x").starts_with(&win("//nas/media/")));
}

#[test]
fn rebase_moves_a_path_to_a_new_root() {
    let moved = posix("/Volumes/share/a/b").rebase(&posix("/Volumes/share"), &posix("/Volumes/share-1"));
    assert_eq!(
        moved.map(AppPathBuf::into_string).as_deref(),
        Some("/Volumes/share-1/a/b")
    );
    assert_eq!(
        posix("/Volumes/other/a").rebase(&posix("/Volumes/share"), &posix("/x")),
        None
    );
    let rel = RelativeAppPath::new("a/b").expect("a tail");
    assert_eq!(
        posix("/").join_relative(rel).map(AppPathBuf::into_string).as_deref(),
        Ok("/a/b")
    );
    for bad in ["/a", "a/", "a//b", "a/./b"] {
        assert_eq!(
            RelativeAppPath::new(bad).err(),
            Some(NotAnAppPath::NotCanonical),
            "{bad:?}"
        );
    }
    assert_eq!(RelativeAppPath::new("a/../b").err(), Some(NotAnAppPath::DotName));
}

#[test]
fn a_unc_root_matches_without_case_and_equality_stays_exact() {
    let a = win("//NAS/Media/x");
    let b = win("//nas/media/y");
    assert!(a.same_root(&b));
    assert_ne!(win("//NAS/Media/x"), win("//nas/media/x"));
    assert!(!win("C:/x").same_root(&win("D:/x")));
    assert!(!posix("/A/x").same_root(&win("C:/x")));
}

#[test]
fn a_name_is_one_plain_component() {
    for good in ["photo.jpg", "a:b", "back\\slash", "...", ".hidden", "\u{0}42"] {
        assert!(AppName::new(good).is_ok(), "{good:?}");
    }
    for bad in ["", ".", "..", "a/b", "/x", "a\u{0}b", "\u{0}"] {
        assert!(AppName::new(bad).is_err(), "{bad:?}");
    }
    // Backend-spelled escapes must be canonical: the bytes of `é` spell `é`.
    assert!(AppName::new("caf\u{10FEC3}\u{10FEA9}").is_err());
    assert!(AppName::new("caf\u{10FEE9}").is_ok());
}

#[test]
fn a_typed_name_holding_an_escape_code_point_names_that_literal_character() {
    let typed = AppNameBuf::from_typed("a\u{10FE80}", Namer::Bytes).expect("a name");
    assert_eq!(typed.as_str(), "a\u{10FEF4}\u{10FE8F}\u{10FEBA}\u{10FE80}");
    let dir = posix("/d");
    let native = dir
        .join(&typed)
        .expect("joins")
        .to_native_posix()
        .expect("native")
        .into_owned();
    assert_eq!(native, "/d/a\u{10FE80}".as_bytes());
    let typed = AppNameBuf::from_typed("a\u{10FE80}", Namer::Units).expect("a name");
    let units = win("C:/d")
        .join(&typed)
        .expect("joins")
        .to_native_windows()
        .expect("native");
    assert_eq!(units, wide("\\\\?\\C:\\d\\a\u{10FE80}"));
}

#[test]
fn a_child_name_joins_onto_an_app_path_beside_the_std_join() {
    let dir = posix("/dest");
    let joined = crate::volume::ChildName::new("photo.jpg")
        .expect("plain")
        .under_app(&dir);
    assert_eq!(joined.map(AppPathBuf::into_string).as_deref(), Ok("/dest/photo.jpg"));
    let refused = crate::volume::ChildName::new("a\\b")
        .expect("plain")
        .under_app(&win("C:/x"));
    assert!(refused.is_err());
}

// ---- Normalization ----

#[test]
fn parse_normalizes_and_refuses_what_it_cant_respell_safely() {
    assert_eq!(posix("/a//b/./c/").as_str(), "/a/b/c");
    assert_eq!(win("c:").as_str(), "C:/");
    assert_eq!(win("C://x").as_str(), "C:/x");
    assert_eq!(win("//srv/share").as_str(), "//srv/share/");
    assert_eq!(posix("sftp://h:22").as_str(), "sftp://h:22/");
    assert_eq!(posix("sftp://h:22//a//b/").as_str(), "sftp://h:22/a/b");
    for (text, flavor, refusal) in [
        ("", PathFlavor::Posix, NotAnAppPath::Empty),
        ("a/b", PathFlavor::Posix, NotAnAppPath::Relative),
        ("C:/x", PathFlavor::Posix, NotAnAppPath::Relative),
        ("/a/../b", PathFlavor::Posix, NotAnAppPath::DotName),
        ("/x", PathFlavor::Windows, NotAnAppPath::WrongFlavor),
        ("C:x", PathFlavor::Windows, NotAnAppPath::DriveRelative),
        ("c://x", PathFlavor::Posix, NotAnAppPath::Relative),
        ("C:\\x", PathFlavor::Windows, NotAnAppPath::Backslash),
        ("//srv", PathFlavor::Windows, NotAnAppPath::MissingShare),
        ("//?/C:/x", PathFlavor::Windows, NotAnAppPath::DevicePath),
        ("sftp://h/a/../b", PathFlavor::Posix, NotAnAppPath::DotName),
    ] {
        assert_eq!(AppPathBuf::parse(text, flavor), Err(refusal), "{text:?} {flavor:?}");
    }
}

#[test]
fn from_normalized_takes_only_canonical_text() {
    for good in [
        "/",
        "/a/b",
        "C:/",
        "C:/x",
        "//s/sh/",
        "//s/sh/x",
        "sftp://h/",
        "sftp://h/x",
        "/a\\b",
    ] {
        assert_eq!(
            AppPath::from_normalized(good).map(AppPath::as_str),
            Ok(good),
            "{good:?}"
        );
    }
    for bad in [
        "/a/",
        "//s/sh",
        "C:",
        "c:/x",
        "/a//b",
        "/a/./b",
        "sftp://h",
        "sftp://h/x/",
        "C:/a\\b",
    ] {
        assert!(AppPath::from_normalized(bad).is_err(), "{bad:?}");
    }
    assert_eq!(AppPath::from_normalized("/a/../b"), Err(NotAnAppPath::DotName));
}

#[test]
fn nfc_and_nfd_are_two_values() {
    assert_ne!(posix("/caf\u{e9}"), posix("/cafe\u{301}"));
    assert_ne!(posix("/caf\u{e9}").as_str(), posix("/cafe\u{301}").as_str());
}

#[test]
fn the_macro_takes_a_canonical_literal() {
    assert_eq!(crate::app_path!("/a/b").as_str(), "/a/b");
    let refused = std::panic::catch_unwind(|| crate::app_path!("/a/"));
    assert!(refused.is_err());
}

// ---- Comparison ----

#[test]
fn a_sorted_set_iterates_in_pathbufs_order() {
    let texts = [
        "/a/b",
        "/a-b",
        "/a.b",
        "/a b",
        "/a",
        "/a/b/c",
        "/\u{e9}",
        "/z",
        "sftp://h/x",
    ];
    let typed: Vec<String> = texts
        .iter()
        .map(|t| posix(t))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(AppPathBuf::into_string)
        .collect();
    let std: Vec<String> = texts
        .iter()
        .map(PathBuf::from)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(typed, std);
    assert_eq!(
        typed,
        [
            "/a",
            "/a/b",
            "/a/b/c",
            "/a b",
            "/a-b",
            "/a.b",
            "/z",
            "/\u{e9}",
            "sftp://h/x"
        ]
    );
}

#[test]
fn a_hashed_set_dedups_bridge_values_as_pathbuf_did() {
    let mut set = HashSet::new();
    set.insert(AppPath::legacy_unchecked(Path::new("/a")).into_owned());
    assert!(!set.insert(AppPath::legacy_unchecked(Path::new("/a/")).into_owned()));
    assert!(!set.insert(AppPath::legacy_unchecked(Path::new("/a/./")).into_owned()));
    assert!(set.insert(AppPath::legacy_unchecked(Path::new("/a/..")).into_owned()));
}

/// The two places the component order departs from POSIX `Path::components`, on purpose: a
/// UNC root and a scheme root are ONE token, so two canonical values naming different places
/// never compare equal.
#[test]
fn a_unc_or_scheme_root_is_one_component() {
    assert_ne!(win("//srv/share/x"), posix("/srv/share/x"));
    assert_ne!(posix("sftp:///x"), posix("sftp://x/"));
    assert_ne!(
        posix("sftp://h/x"),
        AppPath::legacy_unchecked(Path::new("sftp:/h/x")).into_owned()
    );
}

// ---- The native boundary ----

#[test]
fn to_native_posix_is_the_text_with_its_escapes_decoded() {
    assert_eq!(posix("/a\\b").to_native_posix().as_deref(), Ok(&b"/a\\b"[..]));
    assert_eq!(
        posix("/d/caf\u{10FEE9}").to_native_posix().as_deref(),
        Ok(&b"/d/caf\xe9"[..])
    );
    assert_eq!(
        posix("/a\u{10E800}").to_native_posix(),
        Err(NativeRefusal::ForeignEscape)
    );
    assert_eq!(win("C:/x").to_native_posix(), Err(NativeRefusal::WrongFlavor));
    assert_eq!(posix("sftp://h/x").to_native_posix(), Err(NativeRefusal::NotLocal));
}

#[test]
fn to_native_windows_is_the_verbatim_form() {
    for (app, native) in [
        ("C:/Users/test_user_1", r"\\?\C:\Users\test_user_1"),
        ("C:/", r"\\?\C:\"),
        ("//nas/media/films/a.mkv", r"\\?\UNC\nas\media\films\a.mkv"),
        ("//nas/media/", r"\\?\UNC\nas\media\"),
        ("C:/x/CON", r"\\?\C:\x\CON"),
        ("C:/x/a.", r"\\?\C:\x\a."),
        ("//[::1]/D$/x", r"\\?\UNC\[::1]\D$\x"),
        ("C:/x/a\u{10E800}b", "\\\\?\\C:\\x\\a\u{FFFD}b"),
    ] {
        let units = win(app).to_native_windows().unwrap_or_else(|e| panic!("{app}: {e}"));
        if app.contains('\u{10E800}') {
            let mut expected = wide(r"\\?\C:\x\a");
            expected.push(0xD800);
            expected.push(u16::from(b'b'));
            assert_eq!(units, expected, "{app}");
        } else {
            assert_eq!(units, wide(native), "{app}");
        }
    }
}

#[test]
fn to_native_windows_refuses_typed() {
    let long = format!("C:/{}", vec!["x".repeat(200); 165].join("/"));
    for (app, refusal) in [
        ("C:/x/a:b", NativeRefusal::ForbiddenCharacter),
        ("C:/x/a*b", NativeRefusal::ForbiddenCharacter),
        ("C:/x/a\u{1}b", NativeRefusal::ForbiddenCharacter),
        ("//host:445/share/x", NativeRefusal::NoPortOnUnc),
        ("//host@445/share/x", NativeRefusal::NoPortOnUnc),
        ("C:/x/caf\u{10FEE9}", NativeRefusal::ForeignEscape),
        ("sftp://h/x", NativeRefusal::NotLocal),
        (long.as_str(), NativeRefusal::TooLong),
    ] {
        let path = AppPathBuf::parse(app, PathFlavor::Windows).unwrap_or_else(|e| panic!("{app}: {e}"));
        assert_eq!(path.to_native_windows(), Err(refusal), "{app}");
    }
    assert_eq!(posix("/x").to_native_windows(), Err(NativeRefusal::WrongFlavor));
    let nul = AppPath::legacy_unchecked(Path::new("C:/x/a\u{0}b"));
    assert!(nul.to_native_windows().is_err());
}

#[test]
fn to_native_windows_plain_refuses_every_path_whose_plain_form_names_another_file() {
    for (app, reason) in [
        ("C:/x/a.", NotPlainSafe::TrailingDotOrSpace),
        ("C:/x/a ", NotPlainSafe::TrailingDotOrSpace),
        ("C:/x/con.txt", NotPlainSafe::ReservedName),
        ("C:/x/lpt1 .x", NotPlainSafe::ReservedName),
        ("C:/x/CONIN$", NotPlainSafe::ReservedName),
        ("C:/x/COM\u{b9}", NotPlainSafe::ReservedName),
    ] {
        assert_eq!(
            win(app).to_native_windows_plain(),
            Err(NativeRefusal::NotPlainSafe(reason)),
            "{app}"
        );
    }
    let at_limit = format!("C:/{}", "x".repeat(256));
    assert_eq!(win(&at_limit).to_native_windows_plain().map(|u| u.len()), Ok(259));
    let past = format!("C:/{}", "x".repeat(257));
    assert_eq!(
        win(&past).to_native_windows_plain(),
        Err(NativeRefusal::NotPlainSafe(NotPlainSafe::TooLong))
    );
    assert_eq!(win("C:/x/a.b").to_native_windows_plain(), Ok(wide(r"C:\x\a.b")));
    assert_eq!(win("C:/x/console").to_native_windows_plain(), Ok(wide(r"C:\x\console")));
    assert_eq!(
        win("//nas/media/x").to_native_windows_plain(),
        Ok(wide(r"\\nas\media\x"))
    );
}

#[test]
fn from_native_windows_takes_every_spelling_to_one_app_path() {
    for native in [
        r"\\?\C:\Users\test_user_1",
        r"C:\Users\test_user_1",
        "C:/Users/test_user_1",
        r"c:\Users\test_user_1\",
    ] {
        let path = AppPathBuf::from_native_windows(&wide(native), PathFlavor::Windows)
            .unwrap_or_else(|e| panic!("{native}: {e}"));
        assert_eq!(path.as_str(), "C:/Users/test_user_1", "{native}");
    }
    for native in [r"\\?\UNC\nas\media\x", r"\\nas\media\x", "//nas/media/x"] {
        let path = AppPathBuf::from_native_windows(&wide(native), PathFlavor::Windows)
            .unwrap_or_else(|e| panic!("{native}: {e}"));
        assert_eq!(path.as_str(), "//nas/media/x", "{native}");
    }
    // A plain path's `..` is Win32's lexical one.
    let plain = AppPathBuf::from_native_windows(&wide(r"C:\a\..\b"), PathFlavor::Windows);
    assert_eq!(plain.map(AppPathBuf::into_string).as_deref(), Ok("C:/b"));
}

#[test]
fn from_native_windows_refuses_typed() {
    for (native, refusal) in [
        ("C:foo", NotAnAppPath::DriveRelative),
        ("C:", NotAnAppPath::BareDrive),
        (r"\\?\D:", NotAnAppPath::BareDrive),
        ("foo", NotAnAppPath::Relative),
        (r"\\.\PhysicalDrive0", NotAnAppPath::DevicePath),
        (
            r"\\?\Volume{0b1c2d3e-0000-0000-0000-100000000000}\x",
            NotAnAppPath::DevicePath,
        ),
        (r"\\?\GLOBALROOT\x", NotAnAppPath::DevicePath),
        (r"\\?\C:\x\..\y", NotAnAppPath::VerbatimName),
        (r"\\?\C:\x\.\y", NotAnAppPath::VerbatimName),
        (r"\\?\C:\x\\y", NotAnAppPath::VerbatimName),
        (r"\\?\UNC\?\x\y", NotAnAppPath::DevicePath),
        (r"\x", NotAnAppPath::WrongFlavor),
        ("", NotAnAppPath::Empty),
    ] {
        assert_eq!(
            AppPathBuf::from_native_windows(&wide(native), PathFlavor::Windows),
            Err(refusal),
            "{native}"
        );
    }
}

#[test]
fn an_unpaired_surrogate_round_trips_through_its_unit_escape() {
    let mut native = wide(r"\\?\D:\x\a");
    native.push(0xD800);
    native.push(u16::from(b'b'));
    let path = AppPathBuf::from_native_windows(&native, PathFlavor::Windows).expect("a path");
    assert_eq!(path.as_str(), "D:/x/a\u{10E800}b");
    assert_eq!(path.shown(), "D:\\x\\a\u{FFFD}b");
    assert_eq!(path.to_native_windows(), Ok(native));
    // `a<D800>b` and `a<D801>b` are two files on NTFS, and two app paths.
    let mut other = wide(r"\\?\D:\x\a");
    other.push(0xD801);
    other.push(u16::from(b'b'));
    assert_ne!(
        AppPathBuf::from_native_windows(&other, PathFlavor::Windows).expect("a path"),
        path
    );
}

#[test]
fn two_names_differing_only_in_invalid_bytes_get_different_spellings() {
    let e9 = AppPathBuf::from_native_posix(b"/d/caf\xe9.txt", PathFlavor::Posix).expect("a path");
    let e8 = AppPathBuf::from_native_posix(b"/d/caf\xe8.txt", PathFlavor::Posix).expect("a path");
    let lossy = posix("/d/caf\u{FFFD}.txt");
    assert_ne!(e9, e8);
    assert_ne!(e9, lossy);
    assert_eq!(e9.shown(), lossy.as_str());
    assert_eq!(e9.to_native_posix().as_deref(), Ok(&b"/d/caf\xe9.txt"[..]));
}

#[test]
fn a_literal_escape_code_point_in_a_native_name_round_trips() {
    let native = "/d/\u{10FE80}\u{10E800}".as_bytes();
    let path = AppPathBuf::from_native_posix(native, PathFlavor::Posix).expect("a path");
    assert_eq!(path.to_native_posix().as_deref(), Ok(native));
    let units = wide("\\\\?\\C:\\\u{10FE80}\u{10E800}");
    let path = AppPathBuf::from_native_windows(&units, PathFlavor::Windows).expect("a path");
    assert_eq!(path.to_native_windows(), Ok(units));
}

#[test]
fn from_wire_parses_with_the_host_flavor() {
    assert_eq!(
        AppPathBuf::from_wire("/x").is_ok(),
        PathFlavor::HOST == PathFlavor::Posix
    );
    assert_eq!(
        AppPathBuf::from_wire("C:/x").is_ok(),
        PathFlavor::HOST == PathFlavor::Windows
    );
    assert!(AppPathBuf::from_wire("sftp://h/x").is_ok());
}

#[test]
fn the_host_flavor_and_namer_follow_the_build_target() {
    let windows = std::path::MAIN_SEPARATOR == '\\';
    assert_eq!(PathFlavor::HOST == PathFlavor::Windows, windows);
    assert_eq!(Namer::HOST == Namer::Units, windows);
}

#[test]
fn the_host_crossing_matches_its_pure_arm() {
    let dir = crate::testing::TestDir::new("host_crossing");
    let path = AppPathBuf::from_native(&dir, PathFlavor::HOST).expect("a scratch directory is absolute");
    let back = path.to_native().expect("it crosses back");
    assert_eq!(AppPathBuf::from_native(&back, PathFlavor::HOST), Ok(path.clone()));
    assert_eq!(path.to_native_plain().is_ok(), path.to_native().is_ok());
}

#[test]
fn a_test_dir_comes_with_its_app_path() {
    let dir = crate::testing::AppTestDir::new("app_path_dir");
    let app = dir.app_path();
    assert_eq!(
        app.kind() == PathKind::Drive || app.kind() == PathKind::Unc,
        PathFlavor::HOST == PathFlavor::Windows
    );
    let native = app.to_native().expect("it crosses to the OS");
    assert!(native.is_dir(), "{native:?} is the scratch directory");
    std::fs::write(
        app.join(name("a.txt")).expect("joins").to_native().expect("crosses"),
        b"x",
    )
    .expect("writes");
    assert!(dir.native().join("a.txt").is_file());
    let nested = crate::testing::AppTestDir::new_in("app_path_nested", dir.native());
    assert!(nested.app_path().starts_with(app));
}

// ---- Wire ----

#[test]
fn the_wire_is_a_plain_string() {
    let path = posix("/Users/test_user_1/caf\u{e9} \"x\".txt");
    let json = serde_json::to_string(&path).expect("serializes");
    assert_eq!(json, serde_json::to_string(path.as_str()).expect("serializes"));
    let back: AppPathBuf = serde_json::from_str(&json).expect("deserializes on this host");
    if PathFlavor::HOST == PathFlavor::Posix {
        assert_eq!(back, path);
    }
    assert!(serde_json::from_str::<AppPathBuf>("\"relative\"").is_err());
}

#[test]
fn display_is_the_wire_and_debug_names_the_type() {
    let path = posix("/a/b");
    assert_eq!(path.to_string(), "/a/b");
    assert_eq!(format!("{path:?}"), "AppPath(\"/a/b\")");
}
