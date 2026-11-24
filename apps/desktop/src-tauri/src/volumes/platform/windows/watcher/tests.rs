//! The drive watcher on a real Windows desktop session: the broadcast layouts, the seam a
//! change reaches, a real `subst` drive arriving and leaving, and a removal query letting go
//! of a drive's handles. A real USB stick's checks are in `volumes/DETAILS.md` § "Windows:
//! drive arrival and removal".

use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fs::app_path::AppPath;
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::volume::WatchCoverage;
use cmdr_fs::volume::path_grammar::{PathFlavor, app_spelling};
use cmdr_fs::volume::windows_device_change::{self as change, DeviceChange, HandleEvent, VolumeEvent};
use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DBT_DEVTYP_HANDLE, DBT_DEVTYP_VOLUME, DBTF_MEDIA, DEV_BROADCAST_HANDLE, DEV_BROADCAST_VOLUME, GUID_IO_VOLUME_LOCK,
    GUID_IO_VOLUME_LOCK_FAILED, GUID_IO_VOLUME_UNLOCK,
};

use super::{Announce, DriveWatch, Job, Lifecycle};
use crate::file_system::volume::manager::get_volume_manager;
use crate::file_system::watcher::{coverage_for_listings, start_watching, stop_watching, watch_for_real};
use crate::test_support::{TestDir, wait_until};

/// What the watcher announced, for a cell to wait on.
#[derive(Clone, Default)]
struct Recorder {
    arrived: Arc<Mutex<Vec<String>>>,
    left: Arc<Mutex<Vec<String>>>,
}

impl Announce for Recorder {
    fn arrived(&self, root: &str) {
        self.arrived
            .lock()
            .expect("a test's recorder lock isn't poisoned")
            .push(root.to_owned());
    }

    fn left(&self, root: &str) {
        self.left
            .lock()
            .expect("a test's recorder lock isn't poisoned")
            .push(root.to_owned());
    }
}

impl Recorder {
    fn saw_arrive(&self, root: &str) -> bool {
        self.arrived
            .lock()
            .expect("a test's recorder lock isn't poisoned")
            .iter()
            .any(|r| r == root)
    }

    fn saw_leave(&self, root: &str) -> bool {
        self.left
            .lock()
            .expect("a test's recorder lock isn't poisoned")
            .iter()
            .any(|r| r == root)
    }
}

impl DriveWatch {
    /// Starts a watch and waits until its worker has read the drives already there.
    fn started(recorder: Recorder) -> Self {
        let watch = Self::start(recorder).expect("the drive watch starts");
        watch
            .baseline
            .recv_timeout(Duration::from_secs(10))
            .expect("the worker reads its baseline");
        watch
    }

    fn track(&self, volume_id: &str, root: &str) -> Option<usize> {
        let (done, answer) = mpsc::sync_channel(1);
        self.jobs
            .send(Job::Track {
                volume_id: volume_id.to_owned(),
                root: root.to_owned(),
                done,
            })
            .expect("the worker runs");
        answer
            .recv_timeout(Duration::from_secs(10))
            .expect("the worker answers")
    }

    /// What the window does on a removal query, without one: the worker lets go, then answers.
    fn let_go(&self, registration: usize) {
        let (done, answer) = mpsc::sync_channel(1);
        self.jobs
            .send(Job::LetGo { registration, done })
            .expect("the worker runs");
        answer
            .recv_timeout(Duration::from_secs(20))
            .expect("the worker lets go");
    }

    fn take_back(&self, registration: usize) {
        self.jobs.send(Job::TakeBack { registration }).expect("the worker runs");
    }
}

/// A letter in `range` no drive uses. Each cell takes its own range: nextest runs cells in
/// parallel processes, and two cells on one letter would see each other's drives.
fn free_letter(range: std::ops::RangeInclusive<char>) -> char {
    // SAFETY: takes no arguments and only reads the logical drive bitmask.
    let used = unsafe { GetLogicalDrives() };
    range
        .into_iter()
        .find(|letter| used & (1 << (u32::from(*letter) - u32::from('A'))) == 0)
        .expect("a free drive letter")
}

#[test]
fn the_broadcast_layouts_match_windows_own_structs() {
    assert_eq!(
        std::mem::offset_of!(DEV_BROADCAST_HANDLE, dbch_hdevnotify),
        change::HANDLE_REGISTRATION_OFFSET
    );
    assert_eq!(
        std::mem::offset_of!(DEV_BROADCAST_HANDLE, dbch_eventguid),
        change::HANDLE_EVENT_GUID_OFFSET
    );
    assert_eq!(
        std::mem::offset_of!(DEV_BROADCAST_VOLUME, dbcv_unitmask),
        change::VOLUME_UNITMASK_OFFSET
    );
    assert_eq!(
        std::mem::offset_of!(DEV_BROADCAST_VOLUME, dbcv_flags),
        change::VOLUME_FLAGS_OFFSET
    );
    for (ours, theirs) in [
        (change::GUID_IO_VOLUME_LOCK, GUID_IO_VOLUME_LOCK),
        (change::GUID_IO_VOLUME_LOCK_FAILED, GUID_IO_VOLUME_LOCK_FAILED),
        (change::GUID_IO_VOLUME_UNLOCK, GUID_IO_VOLUME_UNLOCK),
    ] {
        let ours = windows_sys::core::GUID::from_u128(ours);
        assert_eq!(
            (ours.data1, ours.data2, ours.data3, ours.data4),
            (theirs.data1, theirs.data2, theirs.data3, theirs.data4)
        );
    }
}

/// The bytes of a `repr(C)` value, as the window hands a broadcast to `decode`.
fn bytes_of<T>(value: &T) -> &[u8] {
    // SAFETY: `value` is a live, initialized `repr(C)` struct of plain integers and pointers,
    // read as bytes for no longer than the borrow.
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}

#[test]
fn windows_own_broadcast_structs_decode() {
    let lock = DEV_BROADCAST_HANDLE {
        dbch_size: u32::try_from(size_of::<DEV_BROADCAST_HANDLE>()).expect("a small struct"),
        dbch_devicetype: DBT_DEVTYP_HANDLE,
        dbch_hdevnotify: 0x5a5a as *mut core::ffi::c_void,
        dbch_eventguid: GUID_IO_VOLUME_LOCK,
        ..DEV_BROADCAST_HANDLE::default()
    };
    assert_eq!(
        change::decode(change::DBT_CUSTOMEVENT, Some(bytes_of(&lock))),
        DeviceChange::Handle {
            event: HandleEvent::LetGo,
            registration: 0x5a5a,
        }
    );
    let card = DEV_BROADCAST_VOLUME {
        dbcv_size: u32::try_from(size_of::<DEV_BROADCAST_VOLUME>()).expect("a small struct"),
        dbcv_devicetype: DBT_DEVTYP_VOLUME,
        dbcv_reserved: 0,
        dbcv_unitmask: 1 << 25,
        dbcv_flags: DBTF_MEDIA,
    };
    assert_eq!(
        change::decode(change::DBT_DEVICEARRIVAL, Some(bytes_of(&card))),
        DeviceChange::Volumes {
            event: VolumeEvent::Arrived,
            letters: vec!['Z'],
            media: true,
            network: false,
        }
    );
}

/// The seam: an announced arrival registers the drive's volume the way the startup sweep
/// does, and its removal unregisters it.
#[test]
fn an_announced_arrival_and_removal_register_and_unregister_the_volume() {
    let root = format!("{}:/", free_letter('J'..='O'));
    let id = crate::volumes::volume_id_for_mount(&root);
    get_volume_manager().unregister(&id);

    Lifecycle.arrived(&root);
    assert!(
        get_volume_manager().get(&id).is_some(),
        "the arrival registers {id} at {root}"
    );

    Lifecycle.left(&root);
    assert!(get_volume_manager().get(&id).is_none(), "the removal unregisters {id}");
}

/// Takes a `subst` letter down however the cell ends.
struct Subst(char);

impl Drop for Subst {
    fn drop(&mut self) {
        // the cell already took it down, or never put it up
        let _ = Command::new("subst")
            .args([format!("{}:", self.0), "/d".to_owned()])
            .status();
    }
}

/// A real drive letter coming and going reaches the watcher with no refresh: `subst`
/// (`DefineDosDevice`), the arrival a hosted runner can make without hardware.
#[test]
fn a_subst_drive_arriving_and_leaving_reaches_the_watcher() {
    let dir = TestDir::new("subst-drive");
    let letter = free_letter('S'..='Y');
    let root = format!("{letter}:/");
    let recorder = Recorder::default();
    let watch = DriveWatch::started(recorder.clone());

    let subst = Subst(letter);
    let status = Command::new("subst")
        .arg(format!("{letter}:"))
        .arg(dir.as_os_str())
        .status()
        .expect("run subst");
    assert!(status.success(), "subst {letter}: failed: {status}");
    wait_until(Duration::from_secs(15), "the subst drive's arrival", || {
        recorder.saw_arrive(&root)
    });

    drop(subst);
    wait_until(Duration::from_secs(15), "the subst drive's removal", || {
        recorder.saw_leave(&root)
    });
    drop(watch);
}

/// #1363 part 2, #1340: a removal query closes every handle Cmdr holds on the drive (its own
/// root handle and a pane's listing watch), proved by NTFS: a folder above an open handle
/// can't be renamed. A refused removal takes both back.
#[test]
fn a_removal_query_lets_go_of_the_drive_and_a_refusal_takes_it_back() {
    let dir = TestDir::new("removal-query");
    let parent = dir.join("parent");
    let drive = parent.join("drive");
    std::fs::create_dir_all(&drive).expect("mkdir");
    let root = app_spelling(&drive.to_string_lossy(), PathFlavor::Windows);
    assert!(AppPath::from_normalized(&root).is_ok(), "{root} is an app path");

    let watch = DriveWatch::started(Recorder::default());
    let listing = unique_test_id("removal-query");
    watch_for_real(&listing);
    start_watching(&listing, &drive).expect("arm a real watch");
    let registration = watch
        .track("removal-query-volume", &root)
        .expect("a folder's handle registers for its volume's removal queries");

    let refused = std::fs::rename(&parent, dir.join("moved")).expect_err("the open handles pin the parent");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied, "{refused:?}");

    watch.let_go(registration);
    assert_eq!(
        coverage_for_listings(std::slice::from_ref(&listing)),
        WatchCoverage::None,
        "a let-go watch claims nothing"
    );
    std::fs::rename(&parent, dir.join("moved")).expect("let go, nothing pins the parent");
    std::fs::rename(dir.join("moved"), &parent).expect("put it back");

    watch.take_back(registration);
    wait_until(Duration::from_secs(10), "the listing watch to re-arm", || {
        coverage_for_listings(std::slice::from_ref(&listing)) == WatchCoverage::EveryWriter
    });
    let refused = std::fs::rename(&parent, dir.join("moved")).expect_err("taken back, the handles pin it again");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied, "{refused:?}");

    stop_watching(&listing);
    drop(watch);
}
