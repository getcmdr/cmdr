//! Names the app path spells with an escape: a Linux name that isn't UTF-8, and on macOS (whose
//! APFS refuses one) a name holding a literal escape-range code point, the same row shape there.
//! Until escapes go live (plan milestone 10f of `docs/specs/app-path-plan.md`), every row the
//! volume hands back for such a name is today's lossy text, so a pane holds ONE row for the file
//! whichever reader built it: the listing, `get_metadata`, or a mutation's patch.

#![cfg(unix)]

use super::*;
use cmdr_fs::app_path::{AppNameBuf, AppPathBuf};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::DirectoryChange;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::listings::RecordingListings;
use cmdr_fs::volume::{MutationEvent, Volume};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;

/// The native name of a file whose app spelling holds an escape on this OS.
fn escaped_native_name() -> &'static OsStr {
    if cfg!(target_os = "linux") {
        OsStr::from_bytes(b"caf\xe9.txt")
    } else {
        OsStr::new("caf\u{10FEE9}.txt")
    }
}

struct Fixture {
    dir: TestDir,
    listings: Arc<RecordingListings>,
    volume: LocalPosixVolume,
    /// The row the listing gives the file.
    listed: FileEntry,
}

async fn fixture(tag: &str) -> Fixture {
    let dir = TestDir::new(tag);
    std::fs::write(dir.join(escaped_native_name()), b"bytes").expect("the file is created");
    let listings = Arc::new(RecordingListings::new());
    let host = VolumeHost::builder().listings(listings.clone()).build();
    let volume = LocalPosixVolume::local_folder("Test", &*dir, host);
    let listed = volume
        .list_directory(volume.root(), None)
        .await
        .expect("the folder lists")
        .into_iter()
        .find(|entry| entry.name.starts_with("caf"))
        .expect("the file lists");
    Fixture {
        dir,
        listings,
        volume,
        listed,
    }
}

/// `get_metadata` of the file's app path (escaped, as a path made from the native one is)
/// answers the listing's row text, ❌ never the escaped spelling.
#[tokio::test]
async fn get_metadata_of_an_escaped_path_answers_the_listed_row() {
    let f = fixture("names_get_metadata").await;
    let native = f.dir.join(escaped_native_name());
    let path = AppPathBuf::from_native(&native).expect("a native path crosses up");
    assert!(
        path.as_str().chars().any(cmdr_fs::app_path::is_escape),
        "the path the volume is asked about holds an escape: {path:?}"
    );

    let entry = f.volume.get_metadata(&path).await.expect("the file stats");

    assert_eq!((&entry.name, &entry.path), (&f.listed.name, &f.listed.path));
    assert_eq!(entry.size, Some(5), "the stat reached the file itself");
}

/// A mutation named by the file's ESCAPED spelling patches no second row beside the listed
/// one: the patch names the file as the folder's text joins it, the way it always did.
#[tokio::test]
async fn a_mutation_named_by_the_escaped_spelling_patches_no_second_row() {
    let f = fixture("names_mutation_escaped").await;
    let escaped = AppNameBuf::from_native_os(escaped_native_name())
        .expect("one plain name")
        .into_string();

    f.volume
        .notify_mutation("vol", f.volume.root(), MutationEvent::Created(escaped))
        .await;

    for (_, _, change) in f.listings.changes() {
        if let DirectoryChange::Added(entry) = change {
            assert_eq!(
                (&entry.name, &entry.path),
                (&f.listed.name, &f.listed.path),
                "a patch may only repeat the listed row"
            );
        }
    }
}

/// A mutation named by the listed row's own name (what an engine sends: a lossy
/// `file_name()`) patches at most that same row.
#[tokio::test]
async fn a_mutation_named_by_the_listed_name_patches_the_listed_row() {
    let f = fixture("names_mutation_listed").await;

    f.volume
        .notify_mutation("vol", f.volume.root(), MutationEvent::Created(f.listed.name.clone()))
        .await;

    let added: Vec<FileEntry> = f
        .listings
        .changes()
        .into_iter()
        .filter_map(|(_, _, change)| match change {
            DirectoryChange::Added(entry) => Some(entry),
            _ => None,
        })
        .collect();
    if cfg!(target_os = "macos") {
        // The listed text IS the file's name there, so the re-stat finds it.
        assert_eq!(added.len(), 1, "{added:?}");
    }
    for entry in added {
        assert_eq!((&entry.name, &entry.path), (&f.listed.name, &f.listed.path));
    }
}

/// ❗ A source name that isn't one plain name is refused typed, ❌ never joined: `Path::join`
/// of `..` asked about the destination's PARENT (and reported it as a conflict), and of `/x`
/// about `/x` itself. The refusal is that ITEM's: it names no destination, so it can't
/// collide, and every other item in the batch is still checked (a conflict has no per-item
/// error, so the refusal is logged). An intended change (plan milestone 9): no listing yields
/// such a name, so only a broken or hostile caller ever sent one. A real name holding escape
/// code points of both alphabets (which no typed name can spell) is checked as before.
#[tokio::test]
async fn a_conflict_scan_refuses_only_the_source_name_that_is_not_one_plain_name() {
    use cmdr_fs::volume::SourceItemInfo;

    let dir = TestDir::new("names_conflicts");
    let odd_real_name = "m\u{10FEE9}x\u{10E801}";
    std::fs::create_dir(dir.join("dest")).unwrap();
    std::fs::write(dir.join("dest/kept.txt"), b"x").unwrap();
    std::fs::write(dir.join("dest").join(odd_real_name), b"x").unwrap();
    let volume = LocalPosixVolume::local_folder("Test", dir.join("dest"), VolumeHost::detached());
    let item = |name: &str| SourceItemInfo {
        name: name.to_string(),
        size: 1,
        modified: None,
        is_directory: false,
    };
    let source_items = ["..", "/etc", "a/b", "", "kept.txt", odd_real_name].map(item);

    let conflicts = volume
        .scan_for_conflicts(&source_items, volume.root())
        .await
        .expect("an odd name doesn't fail the batch");

    let names: Vec<&str> = conflicts.iter().map(|c| c.source_path.as_str()).collect();
    assert_eq!(names, ["kept.txt", odd_real_name], "{conflicts:?}");
}
