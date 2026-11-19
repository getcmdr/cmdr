//! "Go to path" backend: pure path resolution plus the recents store.
//!
//! The dialog lets the user type a path (absolute, `~`-rooted, or relative to
//! the focused pane) and jump to it. All the path reasoning lives here so the
//! frontend stays a thin presenter (AGENTS.md principle 3, "smart backend, thin
//! frontend"). The IPC wrappers in `commands/go_to_path.rs` are pass-throughs.
//!
//! ## What `resolve` does
//!
//! 1. Expands a leading `~` via `crate::commands::file_system::expand_tilde`.
//! 2. Joins a relative `input` against `base_dir` (the focused pane's path).
//! 3. **Lexically** normalizes `.` / `..` components (see the no-canonicalize
//!    decision below) without touching the disk.
//! 4. Classifies the lexical path against the **local** filesystem into
//!    [`GoToPathResolution`].
//!
//! `resolve` is a pure function (no `AppHandle`) so it's unit-testable over a
//! `tempfile::tempdir()`. The async command wraps it in `blocking_with_timeout`.
//!
//! The steps themselves are `cmdr_fs::go_to_path::resolve_with`, which the browser
//! demo's engine runs over its in-memory volume too; this module is the local disk
//! as that function asks it (`LocalDisk`).
//!
//! ## Decision: backend owns resolution
//!
//! A single resolution path serves three callers (live as-you-type warning, the
//! actual jump, the clipboard-prefill decision), so the preview and the action
//! can never drift. The frontend switches on the returned enum variant and acts.
//!
//! ## Decision: no `canonicalize()`
//!
//! We normalize `.`/`..` lexically, not via `Path::canonicalize`. `canonicalize`
//! requires the *whole* path to exist (it errors otherwise), which would break
//! the nearest-ancestor case where the tail doesn't exist. It also resolves
//! symlinks, silently rewriting the path we show and navigate to into something
//! the user didn't type. Lexical normalization keeps the displayed path faithful
//! to the input and lets nearest-ancestor work. `metadata()` (which *does* follow
//! symlinks) only classifies the existing target as file vs. directory, so a
//! symlinked directory navigates into the symlink path and the listing follows
//! it - correct and intended.
//!
//! ## v1 limitations
//!
//! - **Relative paths on a non-local pane.** `base_dir` is the focused pane's
//!   path; if that pane is on MTP/SMB, a relative input resolves against a
//!   non-local base and the local-fs walk falls back to nearest-ancestor (often
//!   `/`). Absolute and `~` paths always work. Accepted degraded behavior.
//! - **Case-insensitive dedupe.** The recents store (see [`history`]) dedupes by
//!   a raw resolved-path string compare, so on case-insensitive APFS
//!   `/Users/x/Foo` and `/Users/x/foo` show as two entries. Worst case: a
//!   duplicate-looking row. We don't `canonicalize()` (reasons above) to fix it.

pub mod history;

use std::path::Path;

use cmdr_fs::go_to_path::{FoundKind, GoToHost, resolve_with};
use cmdr_fs::volume::path_grammar::{self, PathFlavor};

use crate::commands::file_system::expand_tilde;

/// The contract the dialog branches on, shared with the browser demo's engine:
/// `cmdr_fs::go_to_path`, where the classification lives.
pub use cmdr_fs::go_to_path::GoToPathResolution;

/// The local disk, as `resolve_with` asks it.
struct LocalDisk;

impl GoToHost for LocalDisk {
    fn expand_tilde(&self, input: &str) -> String {
        expand_tilde(input)
    }

    /// `metadata` follows symlinks, so a symlinked dir/file classifies as its target.
    fn found(&self, path: &Path) -> Option<FoundKind> {
        std::fs::metadata(path).ok().map(|meta| {
            if meta.is_dir() {
                FoundKind::Directory
            } else {
                FoundKind::File
            }
        })
    }

    /// The app-path spelling of a path the local OS reads (`C:/Users/test_user_1` on Windows; the path
    /// itself on macOS and Linux), the one form a pane holds. A path the grammar can't read keeps its
    /// native spelling.
    fn app_spelling(&self, path: &Path) -> String {
        path_grammar::app_spelling(&path.to_string_lossy(), PathFlavor::HOST)
    }

    /// A path that crosses into a supported archive is browsed as a folder, even though the inner
    /// path (or the `.zip` file itself) isn't a real directory on disk, so typing it in the path
    /// bar enters the archive. The boundary check confirms a real archive FILE + zip magic, so a
    /// directory named `foo.zip` or a mislabeled file still classifies normally.
    fn enters_archive(&self, path: &Path) -> bool {
        cmdr_archive::path_crosses_archive_boundary(path)
    }
}

/// Resolves a typed input against `base_dir` into a [`GoToPathResolution`].
///
/// Pure: no `AppHandle`, no IPC. The disk is only read via `metadata` for
/// classification; the async command wraps this in `blocking_with_timeout`
/// so a hung mount can't freeze IPC.
pub fn resolve(input: &str, base_dir: &str) -> GoToPathResolution {
    resolve_with(input, Path::new(base_dir), &LocalDisk)
}

#[cfg(test)]
mod wire_shape_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn existing_dir_resolves_to_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).expect("create sub");

        let res = resolve(&sub.to_string_lossy(), dir.path().to_str().unwrap());
        assert_eq!(
            res,
            GoToPathResolution::Directory {
                path: sub.to_string_lossy().to_string()
            }
        );
    }

    #[test]
    fn typing_a_path_into_a_zip_enters_it_as_a_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("bundle.zip");
        // A zip start-of-file signature is enough for the boundary magic check.
        fs::write(&zip, b"PK\x03\x04rest-of-archive").expect("write zip");

        // An inner path that doesn't exist on disk still enters the archive.
        let inner = zip.join("docs");
        let res = resolve(&inner.to_string_lossy(), dir.path().to_str().unwrap());
        assert_eq!(
            res,
            GoToPathResolution::Directory {
                path: inner.to_string_lossy().to_string()
            }
        );

        // Typing the `.zip` path itself also enters it (lists the archive root).
        let res_root = resolve(&zip.to_string_lossy(), dir.path().to_str().unwrap());
        assert!(matches!(res_root, GoToPathResolution::Directory { .. }));
    }

    #[test]
    fn a_directory_named_like_a_zip_does_not_enter_as_an_archive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = dir.path().join("foo.zip");
        fs::create_dir(&fake).expect("create dir named foo.zip");

        // A non-existent child: the archive branch would wrongly return
        // `Directory`; a real directory correctly falls to `NearestAncestor`.
        let child = fake.join("ghost");
        let res = resolve(&child.to_string_lossy(), dir.path().to_str().unwrap());
        assert!(matches!(res, GoToPathResolution::NearestAncestor { .. }));
    }

    #[test]
    fn existing_file_resolves_to_file_with_parent_and_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("notes.txt");
        fs::write(&file, b"hi").expect("write file");

        let res = resolve(&file.to_string_lossy(), dir.path().to_str().unwrap());
        assert_eq!(
            res,
            GoToPathResolution::File {
                path: file.to_string_lossy().to_string(),
                parent_dir: dir.path().to_string_lossy().to_string(),
                file_name: "notes.txt".to_string(),
            }
        );
    }

    #[test]
    fn tilde_expands_to_home() {
        // `~` alone expands to the home dir, which exists, so it resolves to a Directory.
        let home = dirs::home_dir().expect("home dir");
        let res = resolve("~", "/tmp");
        assert_eq!(
            res,
            GoToPathResolution::Directory {
                path: home.to_string_lossy().to_string()
            }
        );
    }

    #[test]
    fn relative_input_joins_against_base_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("child");
        fs::create_dir(&sub).expect("create child");

        let res = resolve("child", dir.path().to_str().unwrap());
        assert_eq!(
            res,
            GoToPathResolution::Directory {
                path: sub.to_string_lossy().to_string()
            }
        );
    }

    #[test]
    fn dot_and_dotdot_are_lexically_normalized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a");
        let b = a.join("b");
        fs::create_dir_all(&b).expect("create a/b");

        // From base `a/b`, `./../.` should land back on `a`.
        let res = resolve("./../.", b.to_str().unwrap());
        assert_eq!(
            res,
            GoToPathResolution::Directory {
                path: a.to_string_lossy().to_string()
            }
        );
    }

    #[test]
    fn dotdot_past_a_nonexistent_middle_segment_normalizes_lexically() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `<dir>/nope/../` lexically normalizes to `<dir>`, which exists, even
        // though `nope` never did. Canonicalize would have errored here; lexical
        // normalization is exactly why we don't use it.
        let input = dir.path().join("nope").join("..");
        let res = resolve(&input.to_string_lossy(), "/tmp");
        assert_eq!(
            res,
            GoToPathResolution::Directory {
                path: dir.path().to_string_lossy().to_string()
            }
        );
    }

    #[test]
    fn deep_nonexistent_path_falls_back_to_nearest_ancestor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope").join("a.txt");
        let res = resolve(&missing.to_string_lossy(), "/tmp");
        assert_eq!(
            res,
            GoToPathResolution::NearestAncestor {
                requested: missing.to_string_lossy().to_string(),
                ancestor_dir: dir.path().to_string_lossy().to_string(),
            }
        );
    }

    /// The answer is an app path, so a `..` above the root clamps there: a pane never holds `/..`.
    #[test]
    fn dotdot_above_the_root_answers_the_root() {
        assert_eq!(
            resolve("/..", "/tmp"),
            GoToPathResolution::Directory { path: "/".to_string() }
        );
    }

    #[test]
    fn totally_nonexistent_absolute_path_falls_back_to_root() {
        let res = resolve("/totally-nonexistent-xyz-123", "/tmp");
        assert_eq!(
            res,
            GoToPathResolution::NearestAncestor {
                requested: "/totally-nonexistent-xyz-123".to_string(),
                ancestor_dir: "/".to_string(),
            }
        );
    }

    #[test]
    fn empty_input_is_invalid() {
        let res = resolve("", "/tmp");
        assert!(matches!(res, GoToPathResolution::Invalid { .. }));
    }

    #[test]
    fn whitespace_only_input_is_invalid() {
        let res = resolve("   ", "/tmp");
        assert!(matches!(res, GoToPathResolution::Invalid { .. }));
    }

    #[test]
    fn resolution_serializes_with_camelcase_kind_and_fields() {
        let res = GoToPathResolution::File {
            path: "/Users/x/a.txt".to_string(),
            parent_dir: "/Users/x".to_string(),
            file_name: "a.txt".to_string(),
        };
        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains("\"kind\":\"file\""), "got {json}");
        assert!(json.contains("\"path\""), "got {json}");
        assert!(json.contains("\"parentDir\""), "got {json}");
        assert!(json.contains("\"fileName\""), "got {json}");

        let ancestor = GoToPathResolution::NearestAncestor {
            requested: "/tmp/nope".to_string(),
            ancestor_dir: "/tmp".to_string(),
        };
        let json = serde_json::to_string(&ancestor).unwrap();
        assert!(json.contains("\"kind\":\"nearestAncestor\""), "got {json}");
        assert!(json.contains("\"ancestorDir\""), "got {json}");
    }
}
