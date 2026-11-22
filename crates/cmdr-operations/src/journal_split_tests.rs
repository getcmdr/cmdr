//! Pins for the journal's row split (`journal::journal_split`): a row's ends are typed
//! now, and every row a shipped macOS or Linux build wrote must keep its bytes, since
//! the store's reader (`cmdr-operation-log`'s `escape_mark.rs`) reads them unchanged.

use std::path::Path;

use super::{bridge_split, journal_split};

/// What the store keeps of a split: the folder's names (it interns by `/`, empty
/// names dropped, `store/dirs.rs`), whether it hangs off `/`, and the leaf name.
fn as_interned((dir, name): (String, String)) -> (bool, Vec<String>, String) {
    let names = dir.split('/').filter(|n| !n.is_empty()).map(str::to_owned).collect();
    (dir.starts_with('/'), names, name)
}

/// Every shape a macOS or Linux build journaled stores exactly what it always did:
/// a plain path's text is its app text, and anything the native door refuses (an
/// escape-range code point, `..`, relative text) or skips (a remote volume's
/// `scheme://`) keeps the old split. On Windows none of these is a native path, so
/// each keeps the old split there too.
#[test]
fn every_posix_row_stores_what_it_always_did() {
    let corpus = [
        "/Users/test_user_1/Documents/report.pdf",
        "/a.txt",
        "/",
        "/folder with spaces/naïve café.txt",
        "/nfd/cafe\u{301}.txt",
        "/x/caf\u{10FEE9}.txt",
        "/trailing/slash/",
        "//leading/double",
        "/inner//double/a.txt",
        "/a/../b.txt",
        "/a/./b.txt",
        "relative/a.txt",
        "sftp://test_user_1@testnas.local:22/share/a.txt",
        "sftp://testnas.local",
        "mtp://12345678/65537/DCIM/a.jpg",
        "/name with a colon: 12:00.txt",
    ];
    for text in corpus {
        let path = Path::new(text);
        assert_eq!(
            as_interned(journal_split(path)),
            as_interned(bridge_split(path)),
            "{text:?}"
        );
    }
    // Byte for byte, not only as interned, for the plain shapes every real row has.
    for text in ["/Users/test_user_1/a.txt", "/a.txt", "/", "/x/caf\u{10FEE9}.txt"] {
        assert_eq!(
            journal_split(Path::new(text)),
            bridge_split(Path::new(text)),
            "{text:?}"
        );
    }
}

/// A Linux name that isn't UTF-8 keeps today's lossy row until 10f escapes it.
#[cfg(unix)] // a non-UTF-8 name: Windows `OsStr`s are UTF-16
#[test]
fn a_name_that_is_not_utf8_keeps_its_lossy_row() {
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"/x/caf\xe9/d\xe9.txt"));
    assert_eq!(journal_split(path), bridge_split(path));
    assert_eq!(
        journal_split(path),
        ("/x/caf\u{FFFD}".to_owned(), "d\u{FFFD}.txt".to_owned())
    );
}

/// On Windows a native end stores its APP text, `/` between names, so the store
/// interns its drive as the one root name it reads back (#1351: `C:\Users\x` was one
/// name under `/`, read back as `/C:\Users\x`). Elsewhere the same text is relative
/// and keeps the old split.
#[test]
fn a_windows_native_end_stores_its_app_text() {
    let cases = [
        (
            r"C:\Users\test_user_1\src\report.pdf",
            "C:/Users/test_user_1/src",
            "report.pdf",
        ),
        (
            r"C:\Users\test_user_1\src/report.pdf",
            "C:/Users/test_user_1/src",
            "report.pdf",
        ),
        ("C:/x\\sub\\a.txt", "C:/x/sub", "a.txt"),
        (r"\\?\D:\dst\a.txt", "D:/dst", "a.txt"),
        (r"\\testnas.local\share\d\a.txt", "//testnas.local/share/d", "a.txt"),
        (r"C:\a.txt", "C:/", "a.txt"),
    ];
    for (native, dir, name) in cases {
        let path = Path::new(native);
        let expected = if cfg!(windows) {
            (dir.to_owned(), name.to_owned())
        } else {
            bridge_split(path)
        };
        assert_eq!(journal_split(path), expected, "{native:?}");
    }
}

/// On Windows a volume engine's POSIX path was joined by `std::path` with `\`, which no
/// name there holds (`ChildName` refuses one), so it stores `/`-separated: a POSIX
/// server's undo would read `/photos\sub` as one folder name. Elsewhere `\` stays a
/// name character.
#[test]
fn a_volume_path_joined_with_backslashes_on_windows_stores_its_folders() {
    for (text, dir, name) in [
        (r"/photos\sub\b.txt", "/photos/sub", "b.txt"),
        (r"/photos/sub\b.txt", "/photos/sub", "b.txt"),
        (
            r"sftp://testnas.local/share\d\a.txt",
            "sftp://testnas.local/share/d",
            "a.txt",
        ),
    ] {
        let path = Path::new(text);
        let expected = if cfg!(windows) {
            (dir.to_owned(), name.to_owned())
        } else {
            bridge_split(path)
        };
        assert_eq!(journal_split(path), expected, "{text:?}");
    }
}
