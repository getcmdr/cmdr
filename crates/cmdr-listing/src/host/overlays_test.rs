//! The seam itself: registration, the shadowing merge rule, and the
//! zero-contributor fast path. What the GIT contributor decides is pinned
//! beside it in `file_system/git/overlay_tests.rs`.

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathArithmetic};
use std::sync::Arc;

use super::overlays::ListingOverlay;
use crate::testing::{TestEngine, app_text, decorate};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{InMemoryVolume, Volume};

/// An overlay that contributes one row per name it was built with, for
/// whatever directory it was pointed at.
struct FixedRows {
    id: &'static str,
    at: AppPathBuf,
    names: Vec<&'static str>,
}

impl ListingOverlay for FixedRows {
    fn id(&self) -> &'static str {
        self.id
    }

    fn applies_to(&self, _volume: &dyn Volume, path: &AppPath, _entries: &[FileEntry]) -> bool {
        *path == *self.at
    }

    fn extra_entries(&self, _volume_id: &str, _volume: &dyn Volume, path: &AppPath) -> Vec<FileEntry> {
        self.names
            .iter()
            .map(|name| {
                FileEntry::new(
                    (*name).to_string(),
                    path.join(AppName::from_app_spelling(name).expect("one plain name"))
                        .expect("joins")
                        .into_string(),
                    true,
                    false,
                )
            })
            .collect()
    }
}

fn real_row(name: &str) -> FileEntry {
    FileEntry::new(name.to_string(), format!("/anywhere/{name}"), false, false)
}

fn a_volume() -> Arc<dyn Volume> {
    Arc::new(InMemoryVolume::new("Test")) as Arc<dyn Volume>
}

/// `decorate` folds a contributor's rows in and reports how many it added.
#[tokio::test]
async fn a_contributor_adds_its_rows_to_the_listing() {
    let at = app_text("/overlay/adds");
    let engine = TestEngine::builder()
        .overlay(Arc::new(FixedRows {
            id: "test-adds",
            at: at.clone(),
            names: vec!["branches", "tags"],
        }))
        .build();

    let mut entries = vec![real_row("HEAD"), real_row("config")];
    let added = decorate(&engine, "test", &a_volume(), &at, &mut entries).await;

    assert_eq!(added, 2);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"HEAD") && names.contains(&"config"), "{names:?}");
    assert!(names.contains(&"branches") && names.contains(&"tags"), "{names:?}");
}

/// The merge rule: a contributed row SHADOWS a real one of the same name, so
/// the listing never carries two rows called `branches`.
#[tokio::test]
async fn a_contributed_row_shadows_a_real_one_of_the_same_name() {
    let at = app_text("/overlay/shadows");
    let engine = TestEngine::builder()
        .overlay(Arc::new(FixedRows {
            id: "test-shadows",
            at: at.clone(),
            names: vec!["branches"],
        }))
        .build();

    // A repo made by an older git carries a real, deprecated `.git/branches/`.
    let mut entries = vec![real_row("branches"), real_row("HEAD")];
    let added = decorate(&engine, "test", &a_volume(), &at, &mut entries).await;

    assert_eq!(added, 1);
    let branches: Vec<&FileEntry> = entries.iter().filter(|e| e.name == "branches").collect();
    assert_eq!(branches.len(), 1, "exactly one row called branches: {entries:?}");
    assert!(branches[0].is_directory, "the contributed row is the one that survived");
    assert_eq!(entries.len(), 2, "the unrelated real row is untouched");
}

/// A directory no contributor claims comes back exactly as the volume read it,
/// which is every listing in the app but a handful.
#[tokio::test]
async fn a_directory_no_contributor_claims_is_left_alone() {
    let engine = TestEngine::new();
    let mut entries = vec![real_row("a.txt"), real_row("b.txt")];
    let added = decorate(
        &engine,
        "test",
        &a_volume(),
        cmdr_fs::app_path!("/overlay/untouched"),
        &mut entries,
    )
    .await;

    assert_eq!(added, 0);
    assert_eq!(entries.len(), 2);
}

/// The real entry whose presence makes [`MarkerInEntries`] apply.
const MARKER: &str = "config";

/// The row [`MarkerInEntries`] contributes.
const MARKER_ROW: &str = "Contributed";

/// An overlay that decides from the listing's ENTRIES, not the path: it applies
/// wherever a real entry called [`MARKER`] is in the listing, the way a
/// contributor recognizes a directory by what it holds.
struct MarkerInEntries;

impl ListingOverlay for MarkerInEntries {
    fn id(&self) -> &'static str {
        "test-marker-in-entries"
    }

    fn applies_to(&self, _volume: &dyn Volume, _path: &AppPath, entries: &[FileEntry]) -> bool {
        entries.iter().any(|entry| entry.name == MARKER)
    }

    fn extra_entries(&self, _volume_id: &str, _volume: &dyn Volume, path: &AppPath) -> Vec<FileEntry> {
        vec![FileEntry::new(
            MARKER_ROW.to_string(),
            path.join(AppName::from_app_spelling(MARKER_ROW).expect("one plain name"))
                .expect("joins")
                .into_string(),
            true,
            false,
        )]
    }
}

fn marker_engine() -> TestEngine {
    TestEngine::builder().overlay(Arc::new(MarkerInEntries)).build()
}

/// A contributor sees the entries the volume read, so a listing holding the
/// marker gets the row.
#[tokio::test]
async fn a_contributor_that_reads_entries_applies_when_the_marker_is_there() {
    let engine = marker_engine();
    let mut entries = vec![real_row(MARKER), real_row("data")];
    let added = decorate(&engine, "test", &a_volume(), cmdr_fs::app_path!("/repo"), &mut entries).await;

    assert_eq!(added, 1);
    assert!(entries.iter().any(|e| e.name == MARKER_ROW), "{entries:?}");
}

/// The same contributor stays out of a listing without the marker, whatever
/// the path.
#[tokio::test]
async fn a_contributor_that_reads_entries_stays_out_without_the_marker() {
    let engine = marker_engine();
    let mut entries = vec![real_row("data"), real_row("index")];
    let added = decorate(&engine, "test", &a_volume(), cmdr_fs::app_path!("/repo"), &mut entries).await;

    assert_eq!(added, 0);
    assert_eq!(entries.len(), 2);
}

/// A device backend's synchronous hand-over asks the same question of the
/// handed-over entries: the marker there means "re-read instead" (`None`), and
/// its absence lets the entries in with zero contributed rows.
#[test]
fn a_handed_over_listing_holding_the_marker_asks_for_a_re_read() {
    let volume_id = "test-handed-over-marker";
    let engine = TestEngine::builder()
        .overlay(Arc::new(MarkerInEntries))
        .volume(volume_id, a_volume())
        .build();
    let path = cmdr_fs::app_path!("/repo");

    let with_marker = vec![real_row(MARKER), real_row("data")];
    assert!(
        engine.inner.prepare_handed_over(volume_id, path, with_marker).is_none(),
        "an applicable overlay escalates to a full re-read"
    );

    let without_marker = vec![real_row("data")];
    let prepared = engine
        .inner
        .prepare_handed_over(volume_id, path, without_marker)
        .expect("no overlay applies, so the entries go in as handed over");
    assert_eq!(prepared.len(), 1);
}

/// Registering the same id twice keeps the first, so a double setup can't
/// contribute one set of rows two times.
#[tokio::test]
async fn a_contributor_registered_twice_only_counts_once() {
    let at = app_text("/overlay/twice");
    let mut builder = TestEngine::builder();
    for _ in 0..2 {
        builder = builder.overlay(Arc::new(FixedRows {
            id: "test-twice",
            at: at.clone(),
            names: vec!["branches"],
        }));
    }
    let engine = builder.build();

    let mut entries = Vec::new();
    assert_eq!(decorate(&engine, "test", &a_volume(), &at, &mut entries).await, 1);
    assert_eq!(entries.len(), 1);
}
