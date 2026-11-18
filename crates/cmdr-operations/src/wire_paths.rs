//! The one guard every mutating start runs its caller's paths through: a trailing
//! `/` (or `/.`) is dropped, so the path names the entry its last name says.
//!
//! POSIX resolves `x/link/` and `x/link/.` THROUGH a symlink, so a delete of either
//! walked and emptied the link's target, and a move or rename took the target
//! along. Rows never carry one (they're built by `join`), but a raw IPC call or a
//! backend caller can, and the engine must be safe whatever it's sent. Lexical
//! only, ❌ never a `..` fold (the OS reads `link/..` as the target's parent, and
//! a lexical fold would retarget the op). The typed app path's `from_wire` takes
//! this over (#1225, `docs/specs/app-path-plan.md` § "Normalization").

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// `path` with its trailing `/` and `/.` runs dropped. Unchanged when nothing
/// trails, or when dropping would leave a root: `/`, `mtp://`, or a scheme
/// authority such as `sftp://host/`, which have no last name to name.
#[must_use]
pub(crate) fn without_trailing_separators(path: &Path) -> Cow<'_, Path> {
    // Read-only bytes, to find what trails; the rebuild below stays in `OsStr`, so
    // a name that isn't UTF-8 survives byte for byte, on every OS.
    let text = path.as_os_str().as_encoded_bytes();
    let mut end = text.len();
    // Each step drops one `/`, or the `.` of a trailing `/.`.
    while (end > 1 && text[end - 1] == b'/') || (end > 2 && text[end - 1] == b'.' && text[end - 2] == b'/') {
        end -= 1;
    }
    if end == text.len() || is_a_root(&text[..end]) {
        return Cow::Borrowed(path);
    }
    // `file_name` skips the trailing `/` and `.` the OS would resolve through, and
    // `parent` is the original text before that name, so the join is the kept text.
    // `None` is a trailing `..`, which already names what the OS resolves.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Cow::Owned(parent.join(name)),
        _ => Cow::Borrowed(path),
    }
}

/// [`without_trailing_separators`] over a start's owned paths.
#[must_use]
pub(crate) fn named_as_given(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().map(owned_without_trailing_separators).collect()
}

/// [`without_trailing_separators`] for one owned path.
#[must_use]
pub(crate) fn owned_without_trailing_separators(path: PathBuf) -> PathBuf {
    match without_trailing_separators(&path) {
        Cow::Borrowed(_) => path,
        Cow::Owned(trimmed) => trimmed,
    }
}

/// [`without_trailing_separators`] for a destination the start takes as text.
#[must_use]
pub(crate) fn text_without_trailing_separators(path: String) -> String {
    match without_trailing_separators(Path::new(&path)) {
        Cow::Borrowed(_) => path,
        Cow::Owned(trimmed) => trimmed.to_string_lossy().into_owned(),
    }
}

/// Whether what's left after the trim is a root with no name of its own: all
/// slashes, a bare `scheme:`, or `scheme://authority` with nothing below it.
fn is_a_root(kept: &[u8]) -> bool {
    if kept.iter().all(|&b| b == b'/') || kept.ends_with(b":") {
        return true;
    }
    match kept.windows(3).position(|w| w == b"://") {
        Some(at) => !kept[at + 3..].contains(&b'/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trimmed(text: &str) -> String {
        without_trailing_separators(Path::new(text))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_trailing_slash_or_dot_is_dropped() {
        for (given, named) in [
            ("/x/link/", "/x/link"),
            ("/x/link//", "/x/link"),
            ("/x/link/.", "/x/link"),
            ("/x/link/./", "/x/link"),
            ("/x/link/././/", "/x/link"),
            ("x/link/", "x/link"),
            ("mtp://mtp-0-1/65537/DCIM/", "mtp://mtp-0-1/65537/DCIM"),
            ("smb://nas/share/reports/", "smb://nas/share/reports"),
        ] {
            assert_eq!(trimmed(given), named, "{given}");
        }
    }

    #[test]
    fn a_path_with_nothing_trailing_is_borrowed_as_is() {
        for given in ["/x/link", "/x/.hidden", "/x/a.", "/x/..", "/x/link/../y", "x"] {
            assert!(
                matches!(without_trailing_separators(Path::new(given)), Cow::Borrowed(_)),
                "{given}"
            );
        }
    }

    /// `..` is the OS's to resolve through a link, ❌ never folded here.
    #[test]
    fn a_trailing_dot_dot_stays() {
        assert_eq!(trimmed("/x/link/.."), "/x/link/..");
        // A `/` after a `..` changes nothing: both resolve through the link.
        assert_eq!(trimmed("/x/link/../"), "/x/link/../");
    }

    #[test]
    fn a_root_keeps_its_slash() {
        for given in ["/", "//", "/.", "mtp://", "adb://", "sftp://h/", "smb://nas/", "C:/"] {
            assert_eq!(trimmed(given), given, "{given}");
        }
    }

    /// The rebuild stays in `OsStr` (`parent` + `file_name`), so no name passes
    /// through text: an NFD name keeps its form, as a non-UTF-8 one keeps its bytes.
    #[test]
    fn a_name_keeps_its_exact_bytes() {
        let nfd = "/x/cafe\u{301}/";
        assert_eq!(
            without_trailing_separators(Path::new(nfd))
                .as_os_str()
                .as_encoded_bytes(),
            "/x/cafe\u{301}".as_bytes()
        );
    }
}
