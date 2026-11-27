//! A batch's stats that find nothing are tallied for ONE log line, never one each.
//!
//! A copy onto the share stages every file on a `.cmdr-tmp-*` name and renames it
//! in, so most `Added` and `Modified` events name a temp that's already gone by
//! the time the watcher stats it. Logged per file, that was 10,895 lines (3.5 MB)
//! in six minutes, and it rotated the evidence out of ERR-BGEKC's bundle.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::listings::RecordingListings;
use smb2::FileNotifyAction;

use super::super::MountAnchor;
use super::process_event_batch_with;

const MOUNT: &str = "/Volumes/fixture-share";

#[tokio::test]
async fn stats_that_find_nothing_are_counted_once_per_batch() {
    let listings = Arc::new(RecordingListings::new());
    let host = VolumeHost::builder().listings(listings.clone()).build();
    let mut events: Vec<(FileNotifyAction, String)> = (0..20)
        .flat_map(|n| {
            [
                (FileNotifyAction::Added, format!(".cmdr-tmp-{n}")),
                (FileNotifyAction::Modified, format!(".cmdr-tmp-{n}")),
            ]
        })
        .collect();
    events.push((FileNotifyAction::Added, "landed.mov".to_string()));
    let batch = HashMap::from([(PathBuf::from(MOUNT), events)]);
    // The temps were renamed away before their stat; the landed file is there.
    let stat = |path: PathBuf| async move {
        let name = path.file_name()?.to_string_lossy().to_string();
        (!name.starts_with(".cmdr-tmp-"))
            .then(|| FileEntry::new(name, path.to_string_lossy().to_string(), false, false))
    };

    let misses = process_event_batch_with(
        &host,
        batch,
        "smb-stat-misses-test",
        &MountAnchor::at_share_root(MOUNT),
        stat,
    )
    .await;

    assert_eq!(misses.count, 40);
    assert_eq!(
        misses.first.as_deref(),
        Some(std::path::Path::new("/Volumes/fixture-share/.cmdr-tmp-0"))
    );
    assert_eq!(listings.changes().len(), 1, "only the landed file reached the listing");
}
