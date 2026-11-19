//! The classification over a made-up host: a fixed set of folders and files, `~` as
//! `/home/test_user_1`. The local disk's cells (temp dirs, links, zips) are the desktop's
//! `go_to_path` tests.

use std::path::Path;

use super::*;

struct Fixed;

impl GoToHost for Fixed {
    fn expand_tilde(&self, input: &str) -> String {
        crate::tilde::expand_tilde_with(
            input,
            Some("/home/test_user_1"),
            crate::volume::path_grammar::PathFlavor::Posix,
        )
    }

    fn found(&self, path: &Path) -> Option<FoundKind> {
        match path.to_str()? {
            "/" | "/home" | "/home/test_user_1" | "/home/test_user_1/Downloads" => Some(FoundKind::Directory),
            "/home/test_user_1/notes.md" => Some(FoundKind::File),
            _ => None,
        }
    }

    fn app_spelling(&self, path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }
}

fn go(input: &str, base: &str) -> GoToPathResolution {
    resolve_with(input, base.as_ref(), &Fixed)
}

#[test]
fn a_tilde_path_to_a_folder_is_that_folder() {
    assert_eq!(
        go("~/Downloads", "/"),
        GoToPathResolution::Directory {
            path: "/home/test_user_1/Downloads".to_string()
        }
    );
}

#[test]
fn a_relative_path_resolves_under_the_pane_and_a_file_names_its_parent() {
    assert_eq!(
        go("./notes.md", "/home/test_user_1/Downloads/.."),
        GoToPathResolution::File {
            path: "/home/test_user_1/notes.md".to_string(),
            parent_dir: "/home/test_user_1".to_string(),
            file_name: "notes.md".to_string(),
        }
    );
}

#[test]
fn an_absolute_input_ignores_the_pane() {
    assert_eq!(
        go("/home", "/home/test_user_1"),
        GoToPathResolution::Directory {
            path: "/home".to_string()
        }
    );
}

#[test]
fn a_missing_path_lands_on_its_nearest_existing_ancestor() {
    assert_eq!(
        go("~/Downloads/nope/deeper", "/"),
        GoToPathResolution::NearestAncestor {
            requested: "/home/test_user_1/Downloads/nope/deeper".to_string(),
            ancestor_dir: "/home/test_user_1/Downloads".to_string(),
        }
    );
}

#[test]
fn blank_input_is_invalid() {
    assert!(matches!(go("  ", "/"), GoToPathResolution::Invalid { .. }));
}

#[test]
fn an_archive_the_host_browses_enters_as_a_folder() {
    struct Archives;
    impl GoToHost for Archives {
        fn expand_tilde(&self, input: &str) -> String {
            input.to_string()
        }
        fn found(&self, _path: &Path) -> Option<FoundKind> {
            None
        }
        fn app_spelling(&self, path: &Path) -> String {
            path.to_string_lossy().into_owned()
        }
        fn enters_archive(&self, path: &Path) -> bool {
            path.starts_with("/a.zip")
        }
    }
    assert_eq!(
        resolve_with("/a.zip/inner", Path::new("/"), &Archives),
        GoToPathResolution::Directory {
            path: "/a.zip/inner".to_string()
        }
    );
}
