//! The long-name step over a scripted `GetLongPathNameW`, so it runs on every OS.

use std::io;
use std::path::{Path, PathBuf};

use super::long_spelling;

/// `GetLongPathNameW` over a fixed tree: `/c/Users/RUNNER~1` is `runneradmin`, and only the
/// folders listed exist (a missing path is refused, as the call refuses one).
fn expand(path: &Path) -> io::Result<PathBuf> {
    let home = Path::new("/c/Users/RUNNER~1");
    let existing = [
        Path::new("/c").to_path_buf(),
        Path::new("/c/Users").to_path_buf(),
        home.to_path_buf(),
        home.join("AppData"),
        home.join("AppData").join("Local"),
    ];
    if !existing.iter().any(|folder| folder == path) {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    Ok(match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("/c/Users/runneradmin"),
        Ok(rest) => Path::new("/c/Users/runneradmin").join(rest),
        Err(_) => path.to_path_buf(),
    })
}

#[test]
fn an_existing_short_folder_is_spelled_long() {
    let typed = Path::new("/c/Users/RUNNER~1").join("AppData").join("Local");
    assert_eq!(
        long_spelling(&typed, expand),
        Some(Path::new("/c/Users/runneradmin").join("AppData").join("Local"))
    );
}

#[test]
fn a_missing_tail_keeps_its_typed_spelling_under_the_long_prefix() {
    let typed = Path::new("/c/Users/RUNNER~1").join("AppData").join("gone~1").join("deeper");
    assert_eq!(
        long_spelling(&typed, expand),
        Some(Path::new("/c/Users/runneradmin").join("AppData").join("gone~1").join("deeper"))
    );
}

#[test]
fn a_path_with_no_tilde_is_never_asked() {
    let never = |_: &Path| -> io::Result<PathBuf> { panic!("no short name can be there") };
    assert_eq!(long_spelling(&Path::new("/c/Users").join("someone"), never), None);
}

#[test]
fn a_tilde_that_names_no_short_folder_changes_nothing() {
    let typed = Path::new("/c/Users").join("a~b");
    assert_eq!(long_spelling(&typed, expand), None, "the long spelling is the same text");
}

#[test]
fn nothing_that_exists_leaves_it_as_typed() {
    let nothing = |_: &Path| -> io::Result<PathBuf> { Err(io::Error::from(io::ErrorKind::NotFound)) };
    assert_eq!(long_spelling(&Path::new("/x").join("LONGNA~1"), nothing), None);
}
