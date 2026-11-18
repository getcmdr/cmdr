//! The one place the Windows local volume crosses between app paths (`C:/Users/test_user_1`,
//! `//server/share/x`; plan D1) and the paths Win32 opens (`\\?\C:\Users\test_user_1`).
//!
//! Every call into the OS goes through [`native`], so a path past `MAX_PATH` (260 UTF-16 units)
//! reaches the filesystem whole: the `\\?\` form skips Win32's length limit and its own
//! normalization (verified on Windows 10 22H2, NTFS, the long-path cells in
//! `local_windows_test.rs`, 2026-11-18).

use std::io;
use std::path::{Path, PathBuf};

use cmdr_fs::volume::path_grammar::{AppPath, PathFlavor};

/// The `\\?\` path Win32 opens for the app path `path`.
///
/// # Errors
///
/// `InvalidInput` for anything that isn't an absolute Windows app path: a relative path, a
/// bare `/x`, or one holding a `\`. ❌ Never guess one: a path that loses its drive would
/// resolve against the process's current drive, the wrong disk.
pub(crate) fn native(path: &Path) -> io::Result<PathBuf> {
    app_path(path).map(|app| PathBuf::from(app.to_native())).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an absolute Windows app path (C:/… or //server/share/…)",
        )
    })
}

/// The app path for the entry `name` in the folder at the app path `dir`.
pub(crate) fn child_app_path(dir: &Path, name: &str) -> PathBuf {
    match app_path(dir) {
        Some(app) => PathBuf::from(app.join(name).to_string()),
        // A folder that was listed parsed already; keep the shape for one that somehow didn't.
        None => PathBuf::from(format!("{}/{name}", dir.to_string_lossy().trim_end_matches('/'))),
    }
}

/// A caller's path resolved to an absolute app path under the volume root `root`: the root
/// itself for `""`, `.`, and `/`; an absolute app path as it is (it names its own volume, D1);
/// anything else anchored under `root`, name by name.
///
/// Reads the relative part by `Path::components`, which on Windows splits on `\` as well as
/// `/`, so a relative path a caller built with `Path::join` (`level\a.txt`, #1225) lands on the
/// same app path as `level/a.txt`. NTFS can't hold a `\` in a name, so no name is split wrong.
/// ❌ Not `cmdr_fs::volume::root_anchored`: its `Path::join` writes a `\` into the app path.
pub(crate) fn resolve_under(root: &Path, path: &Path) -> PathBuf {
    use std::path::Component;

    if path.as_os_str().is_empty() || path == Path::new(".") || path == Path::new("/") {
        return root.to_path_buf();
    }
    if let Some(absolute) = app_path(path) {
        return PathBuf::from(absolute.to_string());
    }
    let Some(root_app) = app_path(root) else {
        // A root that isn't an app path can't anchor anything; `native` refuses the result.
        return root.join(path);
    };
    let names: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_string()),
            Component::Prefix(_) | Component::RootDir | Component::CurDir => None,
        })
        .collect();
    PathBuf::from(root_app.join(&names.join("/")).to_string())
}

fn app_path(path: &Path) -> Option<AppPath> {
    AppPath::parse(path.to_str()?, PathFlavor::Windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_path_opens_through_its_long_path_form() {
        assert_eq!(
            native(Path::new("C:/Users/test_user_1")).unwrap(),
            PathBuf::from(r"\\?\C:\Users\test_user_1")
        );
        assert_eq!(native(Path::new("C:/")).unwrap(), PathBuf::from(r"\\?\C:\"));
        assert_eq!(
            native(Path::new("//testnas.local/share/x")).unwrap(),
            PathBuf::from(r"\\?\UNC\testnas.local\share\x")
        );
    }

    #[test]
    fn a_path_that_names_no_drive_is_refused() {
        for bad in ["/Users/x", "relative/x", r"C:\Users\x"] {
            let err = native(Path::new(bad)).expect_err(bad);
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad}");
        }
    }

    #[test]
    fn a_caller_path_resolves_under_the_root_whatever_separator_built_it() {
        let root = Path::new("C:/data");
        assert_eq!(resolve_under(root, Path::new("")), PathBuf::from("C:/data"));
        assert_eq!(resolve_under(root, Path::new("/")), PathBuf::from("C:/data"));
        assert_eq!(
            resolve_under(root, Path::new("a/b.txt")),
            PathBuf::from("C:/data/a/b.txt")
        );
        assert_eq!(
            resolve_under(root, Path::new("/a/b.txt")),
            PathBuf::from("C:/data/a/b.txt")
        );
        assert_eq!(
            resolve_under(root, Path::new(r"a\b.txt")),
            PathBuf::from("C:/data/a/b.txt")
        );
        assert_eq!(resolve_under(root, Path::new("C:/data/x")), PathBuf::from("C:/data/x"));
        assert_eq!(resolve_under(root, Path::new("D:/x")), PathBuf::from("D:/x"));
    }

    #[test]
    fn a_child_keeps_the_app_spelling() {
        assert_eq!(child_app_path(Path::new("C:/"), "a.txt"), PathBuf::from("C:/a.txt"));
        assert_eq!(child_app_path(Path::new("C:/x"), "a.txt"), PathBuf::from("C:/x/a.txt"));
    }
}
