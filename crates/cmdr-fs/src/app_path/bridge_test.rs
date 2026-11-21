//! The transition bridge's arithmetic (right in release builds, `lenient.rs`) and the type
//! half of the escape gate (no conversion to `std::path`, held at compile time).

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use super::*;

fn posix(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Posix).unwrap_or_else(|e| panic!("{text}: {e}"))
}

#[cfg(windows)]
fn win(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Windows).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn name(text: &str) -> &AppName {
    AppName::from_app_spelling(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

// ---- Arithmetic on a transition-bridge value, right in release builds (review 2's N6) ----

fn bridge(text: &str) -> Cow<'_, AppPath> {
    AppPath::legacy_unchecked(Path::new(text))
}

/// Unnormalized POSIX bridge text answers what `Path` answers, and the typed tails and joins
/// come out canonical: `rebase` never builds `/new//b`.
#[test]
fn a_posix_bridge_value_answers_what_path_answers() {
    let sloppy = bridge("/a//b/");
    assert!(!sloppy.is_canonical());
    assert_eq!(sloppy.parent().map(AppPath::as_str), Some("/a"));
    assert_eq!(sloppy.file_name(), Some("b"));
    assert_eq!(sloppy.names().collect::<Vec<_>>(), ["a", "b"]);
    assert!(sloppy.starts_with(&posix("/a")));
    assert_eq!(
        sloppy
            .strip_prefix(&posix("/a"))
            .as_deref()
            .map(RelativeAppPath::as_str),
        Some("b")
    );
    let moved = sloppy.rebase(&posix("/a"), &posix("/new")).expect("under /a");
    assert_eq!(moved.as_str(), "/new/b");
    assert_eq!(
        sloppy.join(name("c")).map(AppPathBuf::into_string).as_deref(),
        Ok("/a/b/c")
    );
    assert_eq!(bridge("/a/./b").parent().map(AppPath::as_str), Some("/a"));
    assert_eq!(bridge("/a/").parent().map(AppPath::as_str), Some("/"));
    assert!(bridge("/").is_root() && bridge("//").is_root());
    // A sloppy root join is the path std opens.
    assert_eq!(bridge("//Users").parent().map(AppPath::as_str), Some("/"));
    // A `..` has no canonical twin: it sits under nothing, and a join is a typed refusal.
    let climbing = bridge("/a/x/../b");
    assert!(!climbing.starts_with(&posix("/a")));
    assert_eq!(climbing.join(name("c")), Err(NotAnAppPath::DotName));
    // Review 3's 1a: a value ending in `..` has no last name to replace. `Path` answers
    // `/a/b/../z` (which opens `/a/z`); the lenient parent `/a/b` would have built `/a/b/z`.
    let up = bridge("/a/b/..");
    assert_eq!(up.with_file_name(name("z")), Err(NotAnAppPath::DotName));
    assert_eq!((up.file_name(), up.extension(), up.file_stem()), (None, None, None));
    assert_eq!(up.join(name("z")), Err(NotAnAppPath::DotName));
    assert_eq!(bridge("rel/x").to_canonical().err(), Some(NotAnAppPath::Relative));
    // Review 2's L-a: a bridge ARGUMENT reads the same way, so the exact and the volume-root
    // matches agree on it.
    let path = posix("/Volumes/x/y");
    let sloppy_root = bridge("/Volumes/x/");
    assert_eq!(
        path.strip_prefix(&sloppy_root).as_deref().map(RelativeAppPath::as_str),
        Some("y")
    );
    assert_eq!(
        path.strip_volume_prefix(&sloppy_root)
            .as_deref()
            .map(RelativeAppPath::as_str),
        Some("y")
    );
}

/// Windows bridge text (`std`'s spelling, `C:\x\y`) read with Windows' rules, on every OS.
#[test]
fn a_windows_bridge_value_reads_as_windows_reads_it() {
    let windows = PathFlavor::Windows;
    let read = lenient::read_as(r"C:\x\y", windows);
    assert_eq!(read.kind(), PathKind::Drive);
    assert_eq!(read.parent_end(), Some(r"C:\x".len()));
    assert_eq!(read.file_name(), Some("y"));
    assert_eq!(read.names(r"C:\x\y"), ["x", "y"]);
    assert!(lenient::read_as(r"C:\", windows).is_root());
    assert_eq!(lenient::read_as(r"C:\x", windows).parent_end(), Some(r"C:\".len()));
    assert_eq!(lenient::read_as(r"\\srv\sh\x", windows).kind(), PathKind::Unc);
    for (text, canonical) in [
        (r"C:\x\y", "C:/x/y"),
        (r"c:\x\y\", "C:/x/y"),
        (r"\\srv\sh\x", "//srv/sh/x"),
        (r"\\?\C:\x\dup.", "C:/x/dup."),
        (r"\\?\UNC\srv\sh\x", "//srv/sh/x"),
        ("/d/a\\b", "/d/a\\b"),
    ] {
        assert_eq!(
            lenient::canonical_as(text, windows)
                .map(AppPathBuf::into_string)
                .as_deref(),
            Ok(canonical),
            "{text}"
        );
    }
    assert_eq!(
        lenient::canonical_as("C:x", windows).err(),
        Some(NotAnAppPath::DriveRelative)
    );
    assert_eq!(
        lenient::canonical_as(r"\\.\pipe\x", windows).err(),
        Some(NotAnAppPath::DevicePath)
    );
    assert_eq!(
        lenient::canonical_as(r"C:\x\..\y", windows).err(),
        Some(NotAnAppPath::DotName)
    );
}

/// On the Windows host itself, the reviewer's inputs through the type.
#[cfg(windows)]
#[test]
fn a_windows_bridge_value_answers_through_the_type() {
    let bridged = bridge(r"C:\x\y");
    assert!(bridged.starts_with(&win("C:/x")));
    assert_eq!(bridged.parent().map(AppPath::as_str), Some(r"C:\x"));
    assert_eq!(bridged.file_name(), Some("y"));
    assert_eq!(bridged.kind(), PathKind::Drive);
    assert!(bridged.same_root(&win("C:/")));
    assert_eq!(
        bridged
            .strip_prefix(&win("C:/"))
            .as_deref()
            .map(RelativeAppPath::as_str),
        Some("x/y")
    );
}

// ---- No conversion to `std::path`, held at compile time ----

/// Whether `$ty` implements `$trait`, answered at compile time (an inherent const that exists
/// only under the bound, shadowing a blanket trait's `false`).
macro_rules! implements {
    ($ty:ty: $($trait:tt)+) => {{
        #[allow(dead_code, reason = "unused where the bound holds: the inherent const answers")]
        trait Fallback {
            const IMPLS: bool = false;
        }
        impl<T: ?Sized> Fallback for T {}
        struct Probe<T: ?Sized>(std::marker::PhantomData<T>);
        #[allow(dead_code, reason = "unused where the bound fails: the fallback answers")]
        impl<T: ?Sized + $($trait)+> Probe<T> {
            const IMPLS: bool = true;
        }
        <Probe<$ty>>::IMPLS
    }};
}

/// The type half of the escape gate (docs/specs/app-path-plan.md § "Keeping it that way"):
/// no app-path type converts to a `std::path` value or to the text one is built from, so
/// `Path::new(&p)`, `PathBuf::from(p)`, `.into()`, and an `impl AsRef<Path>` parameter fed a
/// typed value don't compile. The only exits are the named ones (`to_native`, `as_std_legacy`,
/// `as_str`, `into_string`, `Display`).
#[test]
fn no_app_path_type_converts_to_std_path_or_its_text() {
    use std::ffi::{OsStr, OsString};
    macro_rules! none_of {
        ($($ty:ty),+) => {$(
            assert!(!implements!($ty: AsRef<Path>), "{} is AsRef<Path>", stringify!($ty));
            assert!(!implements!($ty: AsRef<OsStr>), "{} is AsRef<OsStr>", stringify!($ty));
            assert!(!implements!($ty: AsRef<str>), "{} is AsRef<str>", stringify!($ty));
            assert!(!implements!($ty: std::borrow::Borrow<str>), "{} is Borrow<str>", stringify!($ty));
            assert!(!implements!($ty: std::ops::Deref<Target = str>), "{} derefs to str", stringify!($ty));
            assert!(!implements!($ty: Into<PathBuf>), "{} is Into<PathBuf>", stringify!($ty));
            assert!(!implements!($ty: Into<OsString>), "{} is Into<OsString>", stringify!($ty));
            assert!(!implements!($ty: Into<String>), "{} is Into<String>", stringify!($ty));
        )+};
    }
    none_of!(
        AppPath,
        AppPathBuf,
        AppName,
        AppNameBuf,
        RelativeAppPath,
        RelativeAppPathBuf
    );
    // The probe itself answers right.
    assert!(implements!(PathBuf: AsRef<Path>));
    assert!(implements!(String: Into<PathBuf>));
}

// ---- A folder its listed names join under ----

/// A folder with a canonical twin joins each name as that canonical place (`/dot/.` and
/// `/top/` lose their spelling); one with none (relative, a `..`) joins as `Path::join`
/// joined, its text kept, so a walk or a patch under it still reaches the child.
#[test]
fn a_listed_folder_joins_typed_where_it_can_and_as_the_path_form_where_it_cannot() {
    let cases = [
        ("/top", "/top/x"),
        ("/top/", "/top/x"),
        ("/dot/.", "/dot/x"),
        ("sftp://u@h:22", "sftp://u@h:22/x"),
        ("sftp://u@h:22/srv", "sftp://u@h:22/srv/x"),
        ("rel/dir", "rel/dir/x"),
        ("", "x"),
        ("/a/../b", "/a/../b/x"),
    ];
    for (folder, child) in cases {
        let dir = bridge(folder);
        let listed = ListedFolder::new(&dir);
        assert_eq!(
            listed.join(name("x")).map(AppPathBuf::into_string).as_deref(),
            Ok(child),
            "{folder:?}"
        );
    }
}

/// The bridge join keeps a name's escapes round-tripping: an escape-range code point a
/// backend listed as text comes back out as that same text.
#[test]
#[cfg(unix)]
fn a_listed_folder_with_no_canonical_twin_keeps_an_escaped_name() {
    let dir = bridge("rel/dir");
    let typed = AppNameBuf::from_typed("caf\u{10FEE9}", Namer::Bytes).expect("a name");
    let joined = ListedFolder::new(&dir).join(&typed).expect("joins");
    assert_eq!(joined.as_std_legacy(), Path::new("rel/dir/caf\u{10FEE9}"));
}

/// The third caller of the rule: a twin row under a folder with no canonical twin unpicks as
/// the `Path` form does, ❌ never keeping its pick.
#[test]
fn a_twin_row_under_a_bridge_folder_unpicks_as_the_path_form_does() {
    use crate::twin_path::{own_pick_app, pick_segment, unpicked_app};
    for folder in ["rel/DCIM", "/a/../DCIM"] {
        let text = format!("{folder}/{}/IMG_1.JPG", pick_segment(5));
        let twin = bridge(&text);
        assert_eq!(own_pick_app(&twin), Some(5), "{folder}");
        assert_eq!(unpicked_app(&twin).as_str(), format!("{folder}/IMG_1.JPG"), "{folder}");
    }
}

/// A listed row's name joins under a canonical folder to exactly what the bridge made of the
/// `Path` join, a literal escape-range code point included (escapes aren't live, so the row
/// holds it as text), and refuses what `ChildName` refused.
#[test]
#[cfg(unix)]
fn a_listed_name_joins_as_the_bridge_spelled_the_path_join() {
    let names = [
        "x",
        "caf\u{e9}.txt",
        "caf\u{10FEE9}",
        "\u{10E800}a",
        "a b",
        "12:00 notes",
    ];
    for folder in ["/", "/top", "sftp://u@h:22/", "sftp://u@h:22/srv"] {
        for listed in names {
            let native = AppPath::legacy_unchecked(&Path::new(folder).join(listed)).into_owned();
            let name = AppName::from_listed_text(listed).expect("a plain name");
            let joined = ListedFolder::new(&bridge(folder)).join(&name).expect("joins");
            assert_eq!(joined.as_str(), native.as_str(), "{folder:?} {listed:?}");
        }
    }
    assert!(matches!(AppName::from_listed_text("x"), Ok(Cow::Borrowed(_))));
    for refused in ["", ".", "..", "a/b", "a\0b"] {
        assert!(AppName::from_listed_text(refused).is_err(), "{refused:?}");
    }
}
