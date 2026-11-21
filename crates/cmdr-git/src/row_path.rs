//! Where a portal listing's rows live: the app paths they carry.
//!
//! gix answers a repo's worktree root as a NATIVE path (the disk the repo is on). Every
//! row a portal lists sits under that root's `.git`, so the root crosses into an app path
//! once ([`RowPath::dot_git_of`]) and each row is app-path arithmetic below it, the same
//! on every OS: `C:/x/.git/branches/main` on Windows, where `PathBuf::join` wrote
//! `C:\x\.git\branches\main` and the app's own path rules couldn't read it back.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic, RelativeAppPath};
use log::debug;

use crate::path::Cat;

/// A row's path, or the folder rows are listed under.
#[derive(Debug, Clone)]
pub(crate) enum RowPath {
    /// Every root whose native path is Unicode text (every root on macOS).
    App(AppPathBuf),
    /// A root that isn't Unicode text (Linux bytes, a Windows unpaired surrogate): its rows
    /// keep the lossy text they always had, until escapes go live and the root crosses with
    /// an escape like any other.
    Lossy(PathBuf),
}

impl RowPath {
    /// The `.git` folder of the worktree at `worktree_root` (native, as gix answers it).
    pub(crate) fn dot_git_of(worktree_root: &Path) -> Self {
        let dot_git = worktree_root.join(".git");
        match AppPathBuf::from_native_unicode(&dot_git) {
            Ok(app) => Self::App(app),
            Err(why) => {
                debug!(
                    "git portal: {} isn't an app path ({why:?}); its rows keep their lossy text",
                    cmdr_fs::log_private::LogPrivate::dir_path(&dot_git)
                );
                Self::Lossy(dot_git)
            }
        }
    }

    /// The `<cat>` folder under the worktree's `.git`.
    pub(crate) fn category_of(worktree_root: &Path, cat: Cat) -> Self {
        let dot_git = Self::dot_git_of(worktree_root);
        dot_git
            .child(cat.as_segment())
            .unwrap_or_else(|| unreachable!("a category segment is one plain name"))
    }

    /// `rel` below this path: `/`-separated names (a ref name like `feature/foo`, a tree
    /// path), `""` for this path itself. `None` for a name that is no name (`..`, an empty
    /// one, a NUL), which only a hostile repo lists: logged, and no row.
    pub(crate) fn under(&self, rel: &str) -> Option<Self> {
        match self {
            Self::App(path) => {
                let joined = RelativeAppPath::new(rel)
                    .ok()
                    .and_then(|tail| path.join_relative(tail).ok());
                if joined.is_none() {
                    refused(rel);
                }
                joined.map(Self::App)
            }
            Self::Lossy(path) => Some(Self::Lossy(path.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))),
        }
    }

    /// One listed `name` below this path, as [`under`](Self::under) but for a single name, so
    /// a `/` in it is refused rather than read as two.
    pub(crate) fn child(&self, name: &str) -> Option<Self> {
        match self {
            Self::App(path) => {
                let joined = AppName::from_app_spelling(name)
                    .ok()
                    .and_then(|name| path.join(name).ok());
                if joined.is_none() {
                    refused(name);
                }
                joined.map(Self::App)
            }
            Self::Lossy(path) => Some(Self::Lossy(path.join(name))),
        }
    }

    /// The app path, when the root crossed.
    pub(crate) fn app(&self) -> Option<&AppPath> {
        match self {
            Self::App(path) => Some(path),
            Self::Lossy(_) => None,
        }
    }

    /// The last name, as a row's `name` (`""` for a root).
    pub(crate) fn file_name(&self) -> String {
        match self {
            Self::App(path) => path.file_name().unwrap_or_default().to_string(),
            Self::Lossy(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    /// The text a `FileEntry.path` carries.
    pub(crate) fn into_text(self) -> String {
        match self {
            Self::App(path) => path.into_string(),
            Self::Lossy(path) => path.to_string_lossy().into_owned(),
        }
    }
}

/// One debug line per name a portal listing can't address, which is the count.
fn refused(name: &str) {
    debug!(
        "git portal: {} is no name a row can carry; no row",
        cmdr_fs::log_private::LogPrivate::name(name)
    );
}

/// A path gix answers outside the portal (a linked worktree's or a submodule's working
/// folder, where a row redirects), in app spelling. A path that isn't Unicode text keeps
/// the lossy text it always had, until escapes go live.
pub(crate) fn redirect_text(native: &Path) -> String {
    AppPathBuf::from_native_unicode(native).map_or_else(|_| native.display().to_string(), AppPathBuf::into_string)
}

#[cfg(test)]
mod tests {
    use cmdr_fs::app_path::{AppPathBuf, PathFlavor};

    use super::RowPath;

    fn row(text: &str, flavor: PathFlavor) -> RowPath {
        RowPath::App(AppPathBuf::parse(text, flavor).expect("an app path"))
    }

    /// A row is the app path's own arithmetic below the root on every OS: `PathBuf::join`
    /// wrote `C:\x\.git\branches\feature\foo` on Windows, which no app path reads back.
    #[test]
    fn rows_join_by_app_path_arithmetic_whatever_the_roots_shape() {
        for (dot_git, flavor, rel, expected) in [
            (
                "C:/x/.git",
                PathFlavor::Windows,
                "branches/feature/foo",
                "C:/x/.git/branches/feature/foo",
            ),
            (
                "//srv/share/r/.git",
                PathFlavor::Windows,
                "tags/v1.0",
                "//srv/share/r/.git/tags/v1.0",
            ),
            (
                "/Users/a/r/.git",
                PathFlavor::Posix,
                "commits/1234567",
                "/Users/a/r/.git/commits/1234567",
            ),
            ("/r/.git", PathFlavor::Posix, "", "/r/.git"),
        ] {
            let joined = row(dot_git, flavor).under(rel).expect("plain names").into_text();
            assert_eq!(joined, expected, "{dot_git:?} + {rel:?}");
        }
        let posix = row("/r/.git/branches/main", PathFlavor::Posix).child("a\\b.txt");
        assert_eq!(
            posix.map(RowPath::into_text).as_deref(),
            Some("/r/.git/branches/main/a\\b.txt"),
            "a tree name is one name, a `\\` in it included"
        );
        // Under a Windows root a `\` is a separator, so the name has no row there (the
        // native join made it `a`, then `b.txt`).
        assert!(
            row("C:/x/.git/branches/main", PathFlavor::Windows)
                .child("a\\b.txt")
                .is_none()
        );
    }

    /// A name that is no name (only a hostile repo lists one) is no row: `PathBuf::join`
    /// made `main/..` (the parent, which a copy or delete would act on) or replaced the
    /// whole path with `/abs`.
    #[test]
    fn a_name_that_is_no_name_is_no_row() {
        let main = row("/r/.git/branches/main", PathFlavor::Posix);
        for name in ["", ".", "..", "a/b", "/abs", "a\0b"] {
            assert!(main.child(name).is_none(), "{name:?}");
        }
        for rel in ["..", "a/../b", "a//b", "/abs", "a/"] {
            assert!(main.under(rel).is_none(), "{rel:?}");
        }
    }

    /// A row's name is its last name; the root's is empty.
    #[test]
    fn a_rows_name_is_its_last_name() {
        assert_eq!(
            row("/r/.git/branches/feature/foo", PathFlavor::Posix).file_name(),
            "foo"
        );
        assert_eq!(row("/", PathFlavor::Posix).file_name(), "");
    }
}
