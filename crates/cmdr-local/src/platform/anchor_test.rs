//! Where a caller's path lands under a Windows volume root, as a table on every OS: the
//! inputs are canonical app paths, which read the same on every host.

use super::anchor_under;
use cmdr_fs::app_path::{AppPath, AppPathBuf, NotAnAppPath, PathFlavor};

// The relative and `..` inputs only a transition-bridge value spells are Windows cells
// (`local_windows_test.rs`): their reading is the host's.

fn windows(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Windows).unwrap_or_else(|why| panic!("{text:?}: {why}"))
}

fn posix(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Posix).unwrap_or_else(|why| panic!("{text:?}: {why}"))
}

fn anchored(root: &AppPath, path: &AppPath) -> Result<String, NotAnAppPath> {
    anchor_under(root, path).map(AppPathBuf::into_string)
}

#[test]
fn a_drive_or_unc_path_names_its_own_place() {
    let root = windows("C:/data");
    assert_eq!(anchored(&root, &windows("C:/data/x")), Ok("C:/data/x".to_string()));
    assert_eq!(anchored(&root, &windows("D:/x")), Ok("D:/x".to_string()));
    assert_eq!(
        anchored(&root, &windows("//testnas.local/share/x")),
        Ok("//testnas.local/share/x".to_string())
    );
}

#[test]
fn a_posix_path_and_the_roots_aliases_hang_under_the_root() {
    let root = windows("C:/data");
    assert_eq!(anchored(&root, &posix("/")), Ok("C:/data".to_string()));
    assert_eq!(anchored(&root, &posix("/a/b.txt")), Ok("C:/data/a/b.txt".to_string()));
    let drive_root = windows("C:/");
    assert_eq!(anchored(&drive_root, &posix("/a")), Ok("C:/a".to_string()));
}

/// A scheme path isn't anchored: it stays itself, and the crossing refuses it as not local.
#[test]
fn a_scheme_path_is_kept_for_the_crossing_to_refuse() {
    let root = windows("C:/data");
    let path = AppPathBuf::parse("sftp://u@h:22/x", PathFlavor::Windows).expect("parses");
    assert_eq!(anchored(&root, &path), Ok("sftp://u@h:22/x".to_string()));
}
