//! `..` across links over a made-up link table. The real disks' cells are
//! `cmdr-local`'s and `cmdr-sftp`'s.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::app_path::AppPathBuf;

/// Links by path, as stored; a path in `missing` answers `NotFound`, in
/// `refusing` a refusal.
#[derive(Default)]
struct Links {
    links: HashMap<String, PathBuf>,
    refusing: Vec<String>,
}

impl Links {
    fn with(pairs: &[(&str, &str)]) -> Self {
        Self {
            links: pairs
                .iter()
                .map(|(at, to)| ((*at).to_string(), PathBuf::from(to)))
                .collect(),
            refusing: Vec::new(),
        }
    }
}

impl LinkReader for Links {
    fn read_link<'a>(&'a self, path: &'a AppPath) -> LinkFuture<'a> {
        let text = path.as_str().to_string();
        Box::pin(async move {
            if self.refusing.contains(&text) {
                return Err(VolumeError::PermissionDenied {
                    path: text,
                    raw_os_error: None,
                });
            }
            if text.contains("missing") {
                return Err(VolumeError::NotFound(text));
            }
            Ok(self.links.get(&text).cloned())
        })
    }

    fn absolute_target(&self, target: &Path) -> Option<AppPathBuf> {
        AppPathBuf::parse(target.to_str()?, crate::app_path::PathFlavor::Posix).ok()
    }
}

async fn up(links: &Links, path: &str) -> Result<String, VolumeError> {
    let path = AppPathBuf::parse(path, crate::app_path::PathFlavor::Posix).expect("a test path");
    parent_across_links(links, &path).await.map(AppPathBuf::into_string)
}

#[tokio::test]
async fn a_name_that_isnt_a_link_answers_its_lexical_parent() {
    assert_eq!(up(&Links::default(), "/x/dir").await.expect("answers"), "/x");
}

#[tokio::test]
async fn a_link_answers_its_targets_parent() {
    let links = Links::with(&[("/x/link", "/real/dir")]);
    assert_eq!(up(&links, "/x/link").await.expect("answers"), "/real");
}

#[tokio::test]
async fn a_relative_target_reads_from_the_links_folder_and_asks_its_own_dot_dot() {
    // `/x/link -> ../real/dir`: from `/x`, up to `/`, then `real/dir`.
    let links = Links::with(&[("/x/link", "../real/dir")]);
    assert_eq!(up(&links, "/x/link").await.expect("answers"), "/real");
    // The target's `..` crosses another link: `/x/hop -> /far/away`, so `/x/hop/..` is `/far`.
    let links = Links::with(&[("/x/link", "../x/hop/../dir"), ("/x/hop", "/far/away")]);
    assert_eq!(up(&links, "/x/link").await.expect("answers"), "/far");
}

#[tokio::test]
async fn a_target_that_is_itself_a_link_is_followed() {
    let links = Links::with(&[("/x/link", "/p/link2"), ("/p/link2", "/q/dir")]);
    assert_eq!(up(&links, "/x/link").await.expect("answers"), "/q");
}

#[tokio::test]
async fn a_link_to_the_root_answers_the_root() {
    let links = Links::with(&[("/x/link", "/")]);
    assert_eq!(up(&links, "/x/link").await.expect("answers"), "/");
}

#[tokio::test]
async fn a_missing_name_is_no_link() {
    assert_eq!(up(&Links::default(), "/x/missing").await.expect("answers"), "/x");
}

#[tokio::test]
async fn a_refusal_is_refused_never_guessed() {
    let mut links = Links::default();
    links.refusing.push("/x/locked".to_string());
    assert!(matches!(
        up(&links, "/x/locked").await,
        Err(VolumeError::PermissionDenied { .. })
    ));
}

#[tokio::test]
async fn a_link_loop_is_refused() {
    let links = Links::with(&[("/x/a", "/x/b"), ("/x/b", "/x/a")]);
    assert!(matches!(up(&links, "/x/a").await, Err(VolumeError::IoError { .. })));
}
