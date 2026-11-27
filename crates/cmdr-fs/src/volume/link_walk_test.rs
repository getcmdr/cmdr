//! The link policy's backstop for a repeated node the server calls a folder.

use std::future::Future;
use std::pin::Pin;

use crate::app_path::AppPath;
use crate::entry::FileEntry;
use crate::file_identity::FileId;
use crate::volume::descent::MAX_WALK_DEPTH;
use crate::volume::link_walk::{Descending, RootLinks, descend_into, walk_root};
use crate::volume::{BackendKind, EntryKind, ListingProgress, Volume, VolumeError};

/// A share whose every folder answers the same node id while its stat calls
/// each one a folder: a loop presented as a directory (older Samba's `loop ->
/// ..`), or a server whose ids can't be trusted.
struct RepeatingNodeShare;

impl Volume for RepeatingNodeShare {
    fn name(&self) -> &str {
        "Share"
    }
    fn root(&self) -> &AppPath {
        AppPath::from_normalized("/").expect("a canonical literal")
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Smb
    }
    fn list_directory<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        Box::pin(async move { Ok(FileEntry::new("d".into(), path.as_str().to_owned(), true, false)) })
    }
    fn entry_kind<'a>(
        &'a self,
        _path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<EntryKind, VolumeError>> + Send + 'a>> {
        Box::pin(async { Ok(EntryKind::Directory) })
    }
    fn reached_node<'a>(&'a self, _path: &'a AppPath) -> Pin<Box<dyn Future<Output = Option<FileId>> + Send + 'a>> {
        Box::pin(async { Some(FileId::from_stored(1, 7)) })
    }
}

/// The walk goes on past the repeat, since a real folder is never dropped on a
/// guess, but no further than the depth backstop: bounded, ❌ never on to the
/// path limit.
#[tokio::test]
async fn a_repeated_node_the_server_calls_a_folder_stops_at_the_depth_backstop() {
    let share = RepeatingNodeShare;
    let root = AppPath::from_normalized("/top").expect("a canonical path").to_owned();
    let mut here = walk_root(&share, &root).await;
    let mut root_links = RootLinks::default();
    let mut text = root.as_str().to_owned();
    let mut levels = 0;
    let stopped = loop {
        text.push_str("/d");
        let child = AppPath::from_normalized(&text).expect("a canonical path").to_owned();
        match descend_into(&share, &child, false, &here, &root, &mut root_links).await {
            Ok(Descending::Into(next)) => {
                here = next;
                levels += 1;
                assert!(levels <= MAX_WALK_DEPTH, "the walk went past the backstop");
            }
            Ok(Descending::LoopsBack) => panic!("the server says it's a folder, so it isn't left out"),
            Err(e) => break e,
        }
    };

    assert_eq!(levels, MAX_WALK_DEPTH, "walked to the backstop, and no further");
    assert!(
        matches!(stopped, VolumeError::IoError { raw_os_error, .. } if raw_os_error == crate::os_error::OsErrorClass::LinkLoop.host_code()),
        "stopped with the link-loop error, got {stopped:?}"
    );
}
