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

use cmdr_fs::app_path::{AppPath, AppPathBuf};

/// `path` with its trailing `/` and `/.` runs dropped. Unchanged when nothing
/// trails, or when dropping would leave a root: `/`, `mtp://`, or a scheme
/// authority such as `sftp://host/`, which have no last name to name.
#[must_use]
pub(crate) fn without_trailing_separators(path: &Path) -> Cow<'_, Path> {
    // Read-only bytes, to find what trails; the rebuild below is the kept text, or
    // for a name that isn't UTF-8 stays in `OsStr`, so it survives byte for byte.
    let text = path.as_os_str().as_encoded_bytes();
    let mut end = text.len();
    // Each step drops one `/`, or the `.` of a trailing `/.`.
    while (end > 1 && text[end - 1] == b'/') || (end > 2 && text[end - 1] == b'.' && text[end - 2] == b'/') {
        end -= 1;
    }
    if end == text.len() || is_a_root(&text[..end]) {
        return Cow::Borrowed(path);
    }
    // The kept text, cut out as text: a `Path::join` of its parent and name would put a
    // `\` between a POSIX volume's folder and the name on Windows (`/x/link/` came back
    // `/x\link`, a name no volume holds). A trailing `..` keeps what it names, as below.
    if let Some(kept) = path.to_str().map(|text| &text[..end]) {
        return match kept.rsplit(['/', '\\']).next() {
            Some("..") => Cow::Borrowed(path),
            _ => Cow::Owned(PathBuf::from(kept)),
        };
    }
    // Not valid text (a Linux name that isn't UTF-8): the join, in `OsStr`, byte for byte.
    // `file_name` skips the trailing `/` and `.` the OS would resolve through, and
    // `parent` is the original text before that name, so the join is the kept text.
    // `None` is a trailing `..`, which already names what the OS resolves.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Cow::Owned(parent.join(name)),
        _ => Cow::Borrowed(path),
    }
}

/// [`without_trailing_separators`] for one owned path.
#[must_use]
pub(crate) fn owned_without_trailing_separators(path: PathBuf) -> PathBuf {
    match without_trailing_separators(&path) {
        Cow::Borrowed(_) => path,
        Cow::Owned(trimmed) => trimmed,
    }
}

/// A start's typed path as the engine's `std::path` bodies take it: through the bridge,
/// byte for byte, then trimmed as [`without_trailing_separators`] trims. The public starts
/// take app paths (`AppPathBuf`, read at the caller's edge) and ask their route gates
/// (`OperationVolumes`) with them; this is where each one hands its paths on to the bodies
/// behind it, which still do `std::path` arithmetic. ❗ The bridge, ❌ never `to_native`:
/// these paths are a remote volume's as often as the local disk's, and the split between
/// the two comes after. Goes as those bodies take app paths (#1225).
#[must_use]
pub(crate) fn engine_path(path: &AppPath) -> PathBuf {
    owned_without_trailing_separators(path.as_std_legacy().into_owned())
}

/// A `std::path` value the engine holds, as its host's route questions take it
/// (`OperationVolumes::resolve` and friends): through the bridge, byte for byte. The way
/// back from [`engine_path`] for the values a body made itself (a ledger row, a create's
/// parent, a destination it anchored), so every such crossing is this one call. Goes with
/// [`engine_path`] (#1225).
#[must_use]
pub(crate) fn seam_path(path: &Path) -> Cow<'_, AppPath> {
    AppPath::legacy_unchecked(path)
}

/// [`engine_path`] over a start's sources.
#[must_use]
pub(crate) fn engine_paths(paths: &[AppPathBuf]) -> Vec<PathBuf> {
    paths.iter().map(|path| engine_path(path)).collect()
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

    /// The kept text is cut out as text, so a POSIX volume's path keeps its `/` on every
    /// host: a Windows `Path::join` of its parent and name made `/x\link`, one name no
    /// volume holds (a delete of `/x/link/` on a share asked for the wrong file).
    #[test]
    fn the_kept_text_keeps_its_own_separators_on_every_host() {
        assert_eq!(trimmed("/x/link/"), "/x/link");
        assert_eq!(trimmed("sftp://h/srv/a b/"), "sftp://h/srv/a b");
        assert_eq!(trimmed("/x/cafe\u{301}/."), "/x/cafe\u{301}");
    }

    /// A start's typed path reaches the body as the bytes its caller's text held: a trailing
    /// `/` dropped, an NFD name in its form, and a literal escape-range code point (the
    /// command edge reads one through `from_literal_store` while escapes aren't live) as
    /// its own UTF-8, ❌ never an escape's. The `/x` cases are a POSIX volume's paths (a
    /// share's, the in-memory volume's), so they read POSIX on every host: `from_literal_store`
    /// reads only this host's flavor and refuses them on Windows. `from_external_text` in POSIX
    /// flavor spells them the same way (every escape-range code point literal).
    #[test]
    fn a_typed_start_path_crosses_byte_for_byte() {
        use cmdr_fs::app_path::{Namer, PathFlavor};
        for (text, body) in [
            ("/x/link", "/x/link"),
            ("/x/cafe\u{301}.txt", "/x/cafe\u{301}.txt"),
            ("/x/caf\u{10FEE9}.txt", "/x/caf\u{10FEE9}.txt"),
            ("smb://nas/share/a.txt", "smb://nas/share/a.txt"),
        ] {
            let typed =
                AppPathBuf::from_external_text(text, PathFlavor::Posix, Namer::HOST).expect("literal store text");
            assert_eq!(
                engine_path(&typed).as_os_str().as_encoded_bytes(),
                body.as_bytes(),
                "{text}"
            );
        }
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
