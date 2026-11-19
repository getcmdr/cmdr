//! The short-name gate as a table on every OS: the expander is a stand-in that records being
//! asked, so no cell touches a disk.

use std::cell::Cell;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use super::long_root;

/// A native root as the OS would hand it over (Windows spelling, read as data on every host).
fn native(text: &str) -> PathBuf {
    OsString::from(text).into()
}

/// `long_root` of `root` with a stand-in expander: what it answered, and whether it asked.
fn rooted(root: &str, answer: io::Result<&str>) -> (PathBuf, bool) {
    let asked = Cell::new(false);
    let answer = answer.map(native);
    let long = long_root(native(root), |given| {
        asked.set(true);
        assert_eq!(given, native(root), "the expander is asked about the root itself");
        answer
    });
    (long, asked.get())
}

/// A drive root, a share root, a profile folder: no `~`, so nothing reads the disk (a share
/// that's gone offline can't stall a constructor), and the root keeps its own text.
#[test]
fn a_root_with_no_short_name_shape_is_never_expanded() {
    for root in [
        r"C:\",
        r"C:\Users\test_user_1",
        r"\\testnas.local\share",
        r"\\?\C:\x",
        "C:/Users/test_user_1",
    ] {
        assert_eq!(rooted(root, Ok("unused")), (native(root), false), "{root}");
    }
}

/// `%TEMP%` under a long account name carries a short name: that one is spelled long.
#[test]
fn a_root_holding_a_short_name_is_spelled_long() {
    let short = r"C:\Users\LONGNA~1\AppData\Local\Temp\.tmpXYZ";
    let long = r"C:\Users\longname_test_user\AppData\Local\Temp\.tmpXYZ";
    assert_eq!(rooted(short, Ok(long)), (native(long), true));
}

/// A root the expander can't read (gone, refused) keeps its text, as before the gate.
#[test]
fn a_root_the_expander_refuses_keeps_its_text() {
    let short = r"C:\Users\LONGNA~1\gone";
    let refused = Err(io::Error::from(io::ErrorKind::NotFound));
    assert_eq!(rooted(short, refused), (native(short), true));
}
