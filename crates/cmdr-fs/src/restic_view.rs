//! Where a restic repo's snapshot view sits in a path, lexically.
//!
//! The view is `<repo>/Browse snapshots`: a folder a pane shows inside a repo,
//! with no inode behind it. These are the naming questions about it, with no
//! I/O. They live here, not in `cmdr-restic`, so the listing engine and the
//! write guards can ask them without linking `rustic_core`, the same reason
//! `archive_format` holds the archive suffix test. `cmdr-restic` re-exports
//! both names; the confirm (is that folder really a repo?) stays there.

use std::path::{Path, PathBuf};

use crate::app_path::AppPath;

/// The name of the folder a pane shows inside a repo, and the path component
/// every snapshot path runs through: `<repo>/Browse snapshots/latest/…`.
///
/// A path component, so it stays English in every locale: history, favorites,
/// and the agent all address paths, and a translated name would move them when
/// the language changes.
pub const VIRTUAL_ENTRY_NAME: &str = "Browse snapshots";

/// The repo folder a path would browse into, if it runs through the view: the
/// folder holding its first [`VIRTUAL_ENTRY_NAME`] component.
///
/// Lexical only, so it's a candidate, ❗ not a verdict: a real folder of that
/// name outside a repo answers too. A host confirms the candidate before it
/// routes.
pub fn restic_route(path: &Path) -> Option<PathBuf> {
    restic_route_candidates(path).next()
}

/// Whether an app path runs through the view: [`restic_route`]'s `is_some`,
/// asked by name, for a caller holding an [`AppPath`] (the listing engine's
/// fresh-listing oracle). Lexical, so a candidate, ❗ not a verdict.
pub fn runs_through_view(path: &AppPath) -> bool {
    path.names().any(|name| name == VIRTUAL_ENTRY_NAME)
}

/// Every folder in `path` that holds a [`VIRTUAL_ENTRY_NAME`] component, left
/// to right. A router walks them in order and takes the first it confirms, so a
/// real non-repo folder of that name hands over to a repo deeper in the path.
pub fn restic_route_candidates(path: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let mut root = PathBuf::new();
    path.components().filter_map(move |component| {
        let candidate = (component.as_os_str() == VIRTUAL_ENTRY_NAME).then(|| root.clone());
        root.push(component);
        candidate
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_view_component_names_the_repo() {
        assert_eq!(
            restic_route(Path::new("/r/Browse snapshots/latest/backup/Browse snapshots/x")),
            Some(PathBuf::from("/r"))
        );
        assert_eq!(
            restic_route(Path::new("/r/Browse snapshots")),
            Some(PathBuf::from("/r"))
        );
        assert_eq!(restic_route(Path::new("/r/browse snapshots/latest")), None);
        assert_eq!(restic_route(Path::new("/r/Browse snapshots.txt")), None);
        assert_eq!(restic_route(Path::new("/")), None);
    }

    #[test]
    fn an_app_path_runs_through_the_view_by_whole_name() {
        assert!(runs_through_view(crate::app_path!("/r/Browse snapshots/latest")));
        assert!(runs_through_view(crate::app_path!("/r/Browse snapshots")));
        assert!(!runs_through_view(crate::app_path!("/r/browse snapshots/latest")));
        assert!(!runs_through_view(crate::app_path!("/r/Browse snapshots.txt")));
        assert!(!runs_through_view(crate::app_path!("/")));
    }

    #[test]
    fn candidates_come_left_to_right() {
        let candidates: Vec<PathBuf> =
            restic_route_candidates(Path::new("/a/Browse snapshots/b/Browse snapshots/c")).collect();
        assert_eq!(
            candidates,
            [PathBuf::from("/a"), PathBuf::from("/a/Browse snapshots/b")]
        );
    }
}
