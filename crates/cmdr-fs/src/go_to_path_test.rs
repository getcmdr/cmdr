//! The classification over a made-up host: a fixed set of folders and files, `~` as
//! `/home/test_user_1`, every `..` lexical. The local disk's cells (temp dirs, links,
//! zips) are the desktop's `go_to_path` tests; the `..` rules are `typed_path`'s.

use super::*;
use crate::app_path::{AppPathBuf, PathFlavor};
use crate::volume::VolumeError;
use crate::volume::dot_dot::lexical_parent;

struct Fixed;

impl TypedPathHost for Fixed {
    fn flavor(&self) -> PathFlavor {
        PathFlavor::Posix
    }

    fn expand_tilde(&self, input: &str) -> String {
        crate::tilde::expand_tilde_with(input, Some("/home/test_user_1"), PathFlavor::Posix)
    }

    async fn parent_for_dot_dot(&self, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
        Ok(lexical_parent(path))
    }
}

impl GoToHost for Fixed {
    async fn found(&self, path: &AppPath) -> Option<FoundKind> {
        match path.as_str() {
            "/" | "/home" | "/home/test_user_1" | "/home/test_user_1/Downloads" => Some(FoundKind::Directory),
            "/home/test_user_1/notes.md" => Some(FoundKind::File),
            _ => None,
        }
    }

    async fn enters_archive(&self, path: &AppPath) -> bool {
        path.as_str().starts_with("/a.zip")
    }
}

async fn go(input: &str, base: &str) -> GoToPathResolution {
    let base = AppPathBuf::parse(base, PathFlavor::Posix).expect("a test base");
    resolve(input, Some(&base), &Fixed).await
}

fn folder(path: &str) -> GoToPathResolution {
    GoToPathResolution::Directory {
        path: path.to_string(),
        crossed_link: false,
    }
}

#[tokio::test]
async fn a_tilde_path_to_a_folder_is_that_folder() {
    assert_eq!(go("~/Downloads", "/").await, folder("/home/test_user_1/Downloads"));
}

#[tokio::test]
async fn a_relative_path_resolves_under_the_pane_and_a_file_names_its_parent() {
    assert_eq!(
        go("./../notes.md", "/home/test_user_1/Downloads").await,
        GoToPathResolution::File {
            path: "/home/test_user_1/notes.md".to_string(),
            parent_dir: "/home/test_user_1".to_string(),
            file_name: "notes.md".to_string(),
            crossed_link: false,
        }
    );
}

#[tokio::test]
async fn an_absolute_input_ignores_the_pane() {
    assert_eq!(go("/home", "/home/test_user_1").await, folder("/home"));
}

#[tokio::test]
async fn a_missing_path_lands_on_its_nearest_existing_ancestor() {
    assert_eq!(
        go("~/Downloads/nope/deeper", "/").await,
        GoToPathResolution::NearestAncestor {
            requested: "/home/test_user_1/Downloads/nope/deeper".to_string(),
            ancestor_dir: "/home/test_user_1/Downloads".to_string(),
        }
    );
}

#[tokio::test]
async fn blank_input_is_invalid() {
    assert!(matches!(go("  ", "/").await, GoToPathResolution::Invalid { .. }));
}

#[tokio::test]
async fn a_relative_input_with_no_pane_folder_is_invalid() {
    assert!(matches!(
        resolve("notes.md", None, &Fixed).await,
        GoToPathResolution::Invalid { .. }
    ));
}

#[tokio::test]
async fn an_archive_the_host_browses_enters_as_a_folder() {
    assert_eq!(go("/a.zip/inner", "/").await, folder("/a.zip/inner"));
}
