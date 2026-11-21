//! The command edge's door: what a wire path becomes, and what it refuses.

use super::*;

/// The app path a volume call gets for `text`, as text.
fn typed(text: &str) -> String {
    on_volume(text).expect("an app path").as_str().to_owned()
}

/// A path a listing row carries is canonical already, so it crosses byte for byte, NFD,
/// dots, spaces, and an archive's inner path alike: to a volume, and to an engine start.
#[test]
fn a_path_built_from_a_listing_row_is_unchanged() {
    for row in [
        "/",
        "/Users/test_user_1/Documents/a b.txt",
        "/x/.hidden",
        "/x/a.",
        "/x/cafe\u{301}.txt",
        "/x/archive.zip/inner/y.txt",
        "mtp://mtp-0-1/65537/DCIM/x.jpg",
        "sftp://u@testnas.local:22/srv/x",
        "s3://AKIA@host:443/b/a.txt",
    ] {
        assert_eq!(typed(row), row, "{row}");
        assert_eq!(checked(row), Ok(PathBuf::from(row)), "{row}");
    }
}

/// A trailing `/` goes, so `x/link/` names the link, ❌ never its target. A root keeps its
/// own `/`, which is no trailing one.
#[test]
fn a_trailing_slash_is_dropped() {
    assert_eq!(typed("/x/link/"), "/x/link");
    assert_eq!(typed("/x/link//"), "/x/link");
    assert_eq!(typed("/x/link/."), "/x/link");
    assert_eq!(typed("/"), "/");
    assert_eq!(typed("mtp://mtp-0-1/65537/DCIM/"), "mtp://mtp-0-1/65537/DCIM");
}

/// A `..` name is the volume's to answer: refused, wherever it sits, by every door.
#[test]
fn a_dot_dot_name_is_refused() {
    for text in [
        "/x/link/../y",
        "/Volumes/share/../etc",
        "/x/..",
        "x/../y",
        "../y",
        "sftp://h/srv/../etc",
    ] {
        let refusal = WirePathRefused {
            path: text.to_owned(),
            why: NotAnAppPath::DotName,
        };
        assert_eq!(checked(text), Err(refusal.clone()), "{text}");
        assert_eq!(on_volume(text), Err(refusal), "{text}");
        assert_eq!(read_on_volume(text), Err(NotAnAppPath::DotName), "{text}");
    }
}

/// `..` only as a whole name: `..x` and `x..` are ordinary names.
#[test]
fn a_name_holding_dots_is_no_dot_dot() {
    assert_eq!(typed("/x/..y/z.."), "/x/..y/z..");
}

/// Text that's no app path (no root): an engine start keeps today's answer, its text as
/// sent; a volume call has no place to ask about, so it's refused.
#[test]
fn text_with_no_root_is_kept_for_a_start_and_refused_for_a_volume() {
    assert_eq!(checked("relative/dir/"), Ok(PathBuf::from("relative/dir/")));
    assert_eq!(checked(""), Ok(PathBuf::new()));
    assert!(matches!(
        on_volume("relative/dir"),
        Err(WirePathRefused {
            why: NotAnAppPath::Relative,
            ..
        })
    ));
}

/// While escapes aren't live, an escape-range code point in wire text is literal, so the
/// path names the file whose name holds that code point, byte for byte.
#[cfg(unix)] // The native crossing of a POSIX path.
#[test]
fn an_escape_range_code_point_crosses_byte_for_byte() {
    let text = "/x/caf\u{10FEE9}.txt";
    assert!(is_escape('\u{10FEE9}'));
    assert_eq!(checked(text), Ok(PathBuf::from(text)));
    let native = on_volume(text).expect("an app path").to_native().expect("a local path");
    assert_eq!(native.to_str(), Some(text));
}

/// A refusal of the whole set: one `..` refuses every source, so nothing starts.
#[test]
fn a_set_with_one_dot_dot_is_refused_whole() {
    assert!(checked_all(["/a/b", "/a/../c"]).is_err());
    assert_eq!(
        checked_all(["/a/b/", "/a/c"]),
        Ok(vec![PathBuf::from("/a/b/"), PathBuf::from("/a/c")])
    );
}

fn posix_root(text: &str) -> &AppPath {
    AppPath::from_normalized_as(text, PathFlavor::Posix).expect("a canonical root")
}

/// The transfer dialog's box on macOS and Linux: the boot volume's `/` holds every
/// absolute path as is, and a share's root takes a volume-relative `/photos` under it.
#[test]
fn a_destination_anchors_on_its_volume_root() {
    let on = |root: &str, text: &str| anchored_text(posix_root(root), text);
    assert_eq!(on("/", "/Users/test_user_1/dst"), "/Users/test_user_1/dst");
    assert_eq!(on("/", "/Users/test_user_1/dst/"), "/Users/test_user_1/dst");
    assert_eq!(on("/Volumes/share", "/photos"), "/Volumes/share/photos");
    assert_eq!(on("/Volumes/share", "/Volumes/share/photos"), "/Volumes/share/photos");
    assert_eq!(on("/Volumes/share", ""), "/Volumes/share");
}

/// A person typed the box, so a `..` there is no sender bug: it has no typed anchor (the
/// probe can't tell) until the typed-path resolver answers it. Nor has another volume's path.
#[test]
fn a_typed_dot_dot_in_a_destination_has_no_typed_anchor() {
    assert_eq!(anchored(posix_root("/"), "/x/link/../dst"), None);
    assert_eq!(anchored(posix_root("/Volumes/share"), "/a/../b"), None);
    assert_eq!(anchored(posix_root("/Volumes/share"), "sftp://h/srv/x"), None);
}

/// The same-drive move on Windows: the Move dialog hands a volume-relative `/Users/…`, and
/// the boot volume's root is `C:/`, so the destination lands on that drive. Windows rules,
/// run on every host (the anchor is pure).
#[test]
fn a_volume_relative_destination_lands_on_a_drive_root() {
    let root = AppPath::from_normalized_as("C:/", PathFlavor::Windows).expect("a canonical drive root");
    assert_eq!(anchored_text(root, "/Users/Admin/w35/dst"), "C:/Users/Admin/w35/dst");
    // A drive path parses as one only under the Windows flavor, which is the host's there.
    #[cfg(windows)]
    assert_eq!(anchored_text(root, "C:/Users/Admin/w35/dst/"), "C:/Users/Admin/w35/dst");
}

fn anchored_text(root: &AppPath, text: &str) -> String {
    anchored(root, text).expect("anchors").as_str().to_owned()
}
