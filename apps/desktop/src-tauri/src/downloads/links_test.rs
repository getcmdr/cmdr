//! A Downloads folder holding links out of itself (#1538).
//!
//! The reporter's `~/Downloads` held a Wine `.app` bundle whose prefix links
//! `dosdevices/z:` to `/`. `notify-debouncer-full`'s default `FileIdMap` cache
//! walks the watched tree following links, so arming the watch walked the
//! whole disk, on the main thread, and hung launch. These tests build that
//! folder: a link to `/`, a link to an ancestor (a loop), a link to a sibling
//! folder outside, and a few thousand ordinary files.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::EventKind;
use notify::event::{ModifyKind, RenameMode};
use notify_debouncer_full::DebounceEventResult;

use super::watcher::{BatchHandler, arm_debouncer, scan_latest};
use super::watcher_test::{emitted, make_event, scripted, touch_stamped, unhidden_tempdir};

/// Files in the ordinary part of the fixture. Enough that a per-file stat at
/// arm time shows up, few enough to build in well under a second.
const ORDINARY_FILES: usize = 2_000;

/// A Downloads root (`<tmp>/Downloads`) holding every kind of escape, plus a
/// newer eligible file OUTSIDE it (`<tmp>/outside/newest.zip`) that only a
/// walk following a link could reach. Returns the root.
fn hostile_downloads(tmp: &Path) -> PathBuf {
    let root = tmp.join("Downloads");
    let dosdevices = root.join("Heroes.app/Contents/wine/dosdevices");
    fs::create_dir_all(&dosdevices).unwrap();
    cmdr_fs::testing::symlink("/", dosdevices.join("z:")).unwrap();
    cmdr_fs::testing::symlink(tmp, root.join("loop-to-ancestor")).unwrap();

    let outside = tmp.join("outside");
    fs::create_dir(&outside).unwrap();
    touch_stamped(&outside, "newest.zip", 1_800_000_000);
    cmdr_fs::testing::symlink(&outside, root.join("escape")).unwrap();

    for dir in 0..ORDINARY_FILES / 50 {
        let sub = root.join(format!("archive-{dir}"));
        fs::create_dir(&sub).unwrap();
        for file in 0..50 {
            touch_stamped(&sub, &format!("file-{file}.bin"), 1_600_000_000);
        }
    }
    root
}

fn quiet_handler() -> BatchHandler {
    Arc::new(|_: DebounceEventResult| {})
}

/// Arming the watch does no filesystem walk at all. `NullWatcher` stands in
/// for FSEvents, so the time measured is the debouncer's own work: with the
/// default `FileIdMap` it's a walk of the whole disk through `z:`.
#[test]
fn arming_never_walks_the_tree() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let tmp = unhidden_tempdir();
    let root = hostile_downloads(tmp.path());

    let started = Instant::now();
    let debouncer = arm_debouncer::<notify::NullWatcher>(&root, quiet_handler()).expect("arm");
    let took = started.elapsed();

    assert!(
        took < Duration::from_millis(500),
        "arming took {took:?}: the debouncer walked the watched tree"
    );
    drop(debouncer);
}

/// The cold-start fallback stays inside Downloads: the newest eligible file is
/// behind `escape`, and `z:` reaches the whole disk, but neither is walked.
#[test]
fn scan_latest_never_follows_a_link_out_of_the_root() {
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    let tmp = unhidden_tempdir();
    let root = hostile_downloads(tmp.path());
    let inside = touch_stamped(&root, "inside.pdf", 1_700_000_000);

    assert_eq!(scan_latest(&root), Some(inside));
}

/// Without a file-ID cache nothing pairs a rename's halves, so every platform
/// hands the pipeline the halves on their own. macOS's direction-less shape is
/// `partial_rename_to_final_emits_for_final_path`; this is the explicit
/// `From` + `To` shape Windows reports. The final name still surfaces once.
#[test]
fn unpaired_from_and_to_halves_emit_the_final_path_once() {
    let (_td, root, watcher, rx) = scripted();
    let partial = root.join("setup.exe.part");
    let final_path = root.join("setup.exe");
    fs::write(&final_path, b"data").unwrap();

    watcher.deliver(vec![
        make_event(EventKind::Modify(ModifyKind::Name(RenameMode::From)), vec![partial]),
        make_event(
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            vec![final_path.clone()],
        ),
    ]);

    assert_eq!(emitted(&rx), vec!["setup.exe"]);
}
