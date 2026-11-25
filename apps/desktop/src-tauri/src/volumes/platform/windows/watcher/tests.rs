//! The drive watcher on a real Windows desktop session: the broadcast layouts, the seam a
//! change reaches, a real `subst` drive and a real `net use` mapping arriving and leaving, and
//! a removal query letting go of a drive's handles. A real USB stick's checks are in
//! `volumes/DETAILS.md` § "Windows: drive arrival and removal".

use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fs::app_path::AppPath;
use cmdr_fs::testing::unique_test_id;
use cmdr_fs::volume::WatchCoverage;
use cmdr_fs::volume::path_grammar::{PathFlavor, app_spelling};
use cmdr_fs::volume::windows_device_change::{self as change, DeviceChange, HandleEvent, LetGoCause, VolumeEvent};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DBT_DEVTYP_HANDLE, DBT_DEVTYP_VOLUME, DBTF_MEDIA, DEV_BROADCAST_HANDLE, DEV_BROADCAST_VOLUME, GUID_IO_VOLUME_LOCK,
    GUID_IO_VOLUME_LOCK_FAILED, GUID_IO_VOLUME_UNLOCK, SendMessageW, WM_DEVICECHANGE,
};
use windows_sys::core::GUID;

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

    /// Sends the window a handle event for `registration`, as Windows does: synchronously,
    /// answering what the window answered. A query returns only once the worker let go.
    fn send_handle_event(&self, event: usize, registration: usize, guid: GUID) -> isize {
        let broadcast = DEV_BROADCAST_HANDLE {
            dbch_size: u32::try_from(size_of::<DEV_BROADCAST_HANDLE>()).expect("a small struct"),
            dbch_devicetype: DBT_DEVTYP_HANDLE,
            dbch_hdevnotify: registration as *mut core::ffi::c_void,
            dbch_eventguid: guid,
            ..DEV_BROADCAST_HANDLE::default()
        };
        // SAFETY: the watch's own window, alive until the watch drops; `broadcast` outlives the
        // call, which returns only after the window procedure did.
        unsafe {
            SendMessageW(
                self.window as HWND,
                WM_DEVICECHANGE,
                event,
                (&raw const broadcast) as LPARAM,
            )
        }
    }

    fn query_remove(&self, registration: usize) {
        let answer = self.send_handle_event(change::DBT_DEVICEQUERYREMOVE, registration, GUID::from_u128(0));
        assert_eq!(answer, 1, "the window grants the query");
    }

    fn lock(&self, registration: usize) {
        let answer = self.send_handle_event(change::DBT_CUSTOMEVENT, registration, GUID_IO_VOLUME_LOCK);
        assert_eq!(answer, 1, "the window grants the lock");
    }

    fn refuse_removal(&self, registration: usize) {
        self.send_handle_event(change::DBT_DEVICEQUERYREMOVEFAILED, registration, GUID::from_u128(0));
    }

    fn unlock(&self, registration: usize) {
        self.send_handle_event(change::DBT_CUSTOMEVENT, registration, GUID_IO_VOLUME_UNLOCK);
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
        let ours = GUID::from_u128(ours);
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
            event: HandleEvent::LetGo(LetGoCause::Lock),
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

/// Takes a `net use` letter down however the cell ends.
struct NetUse(char);

impl Drop for NetUse {
    fn drop(&mut self) {
        // the cell already took it down, or never put it up
        let _ = Command::new("net")
            .args([
                "use".to_owned(),
                format!("{}:", self.0),
                "/delete".to_owned(),
                "/y".to_owned(),
            ])
            .status();
    }
}

/// `\\localhost\X$\rest` for the local folder `dir`, through the drive's admin share.
fn admin_share_path(dir: &std::path::Path) -> String {
    let native = dir.to_string_lossy();
    let (drive, rest) = native.split_once(":\\").expect("a drive-letter path");
    format!(r"\\localhost\{drive}$\{rest}")
}

/// #1481: a drive mapped from a console (`net use`, `WNetAddConnection2`) to a folder below a
/// share reaches the watcher with no refresh, as a share of its own rooted at the folder, and
/// nothing indexes it. A runner whose admin share can't be mapped skips, saying so.
#[test]
fn a_drive_mapped_to_a_folder_arrives_and_leaves_as_its_own_share() {
    let dir = TestDir::new("mapped-folder");
    let letter = free_letter('P'..='R');
    let root = format!("{letter}:/");
    let unc = admin_share_path(dir.as_ref());
    let recorder = Recorder::default();
    let watch = DriveWatch::started(recorder.clone());

    let mapping = NetUse(letter);
    let mapped = Command::new("net")
        .args([
            "use".to_owned(),
            format!("{letter}:"),
            unc.clone(),
            "/persistent:no".to_owned(),
        ])
        .status()
        .expect("run net use");
    if !mapped.success() {
        log::warn!(target: "volumes::windows", "Skipping: this machine wouldn't map {unc} ({mapped})");
        return;
    }
    wait_until(Duration::from_secs(15), "the mapped drive's arrival", || {
        recorder.saw_arrive(&root)
    });

    let expected = cmdr_fs::volume::windows_volume_ids::mapped_drive(
        &root,
        Some(&unc),
        &cmdr_fs::mount_layout::boot_volume_root(),
    );
    let id = crate::volumes::volume_id_for_mount(&root);
    assert_eq!(id, expected.id, "the drive keys on the folder it maps");
    assert_ne!(
        id,
        crate::volumes::volume_id_for_mount(&format!("//localhost/{letter}$/")),
        "a folder below the share isn't the share"
    );
    Lifecycle.arrived(&root);
    let registered = get_volume_manager().get(&id).expect("the arrival registers it");
    assert_eq!(
        registered.root().as_str(),
        root,
        "rooted at its letter, the folder it maps"
    );
    // By id, as `list_volumes` reads a Windows row (`tools/resources/volumes.rs::index_statuses`).
    let status = crate::index_host::index().volume_status(&id);
    assert!(
        !status.enabled && status.freshness.is_none(),
        "nothing indexes a mapped share uninvited"
    );
    Lifecycle.left(&root);

    drop(mapping);
    wait_until(Duration::from_secs(15), "the mapped drive's removal", || {
        recorder.saw_leave(&root)
    });
    drop(watch);
}

/// A folder above `drive`, which NTFS refuses to rename while any handle is open below it.
struct Pinned {
    dir: TestDir,
    parent: std::path::PathBuf,
    drive: std::path::PathBuf,
}

impl Pinned {
    fn new(tag: &str) -> Self {
        let dir = TestDir::new(tag);
        let parent = dir.join("parent");
        let drive = parent.join("drive");
        std::fs::create_dir_all(&drive).expect("mkdir");
        Self { dir, parent, drive }
    }

    fn root(&self) -> String {
        let root = app_spelling(&self.drive.to_string_lossy(), PathFlavor::Windows);
        assert!(AppPath::from_normalized(&root).is_ok(), "{root} is an app path");
        root
    }

    /// Whether some handle below the parent still pins it; a rename that went through is put
    /// back.
    fn is_pinned(&self) -> bool {
        let moved = self.dir.join("moved");
        match std::fs::rename(&self.parent, &moved) {
            Ok(()) => {
                std::fs::rename(&moved, &self.parent).expect("put it back");
                false
            }
            Err(refused) => {
                assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied, "{refused:?}");
                true
            }
        }
    }
}

/// A pane's real listing watch on `drive`, and the means to wait for it.
fn watch_listing(drive: &std::path::Path, tag: &str) -> String {
    let listing = unique_test_id(tag);
    watch_for_real(&listing);
    start_watching(&listing, drive).expect("arm a real watch");
    listing
}

fn rearmed(listing: &str) -> bool {
    coverage_for_listings(&[listing.to_owned()]) == WatchCoverage::EveryWriter
}

/// #1363 part 2, #1340: a removal query, sent through the window as Windows sends it, closes
/// every handle Cmdr holds on the drive (its own root handle and a pane's listing watch) before
/// the window answers, proved by NTFS: a folder above an open handle can't be renamed. A refused
/// removal takes both back, the root handle on its own too.
#[test]
fn a_removal_query_lets_go_of_the_drive_and_a_refusal_takes_it_back() {
    let pinned = Pinned::new("removal-query");
    let watch = DriveWatch::started(Recorder::default());
    let listing = watch_listing(&pinned.drive, "removal-query");
    let registration = watch
        .track("removal-query-volume", &pinned.root())
        .expect("a folder's handle registers for its volume's removal queries");
    assert!(pinned.is_pinned(), "the handles pin the parent");

    watch.query_remove(registration);
    assert_eq!(
        coverage_for_listings(std::slice::from_ref(&listing)),
        WatchCoverage::None,
        "a let-go watch claims nothing"
    );
    assert!(!pinned.is_pinned(), "let go, nothing pins the parent");

    watch.refuse_removal(registration);
    wait_until(Duration::from_secs(10), "the listing watch to re-arm", || {
        rearmed(&listing)
    });
    stop_watching(&listing);
    assert!(pinned.is_pinned(), "taken back, the root handle alone pins it again");
    drop(watch);
}

/// A removal can bring a query AND a volume lock, and a refusal one take-back: the second
/// let-go changes nothing, and the one take-back restores everything.
#[test]
fn a_query_and_a_lock_let_go_once_and_one_take_back_restores_it() {
    let pinned = Pinned::new("query-and-lock");
    let watch = DriveWatch::started(Recorder::default());
    let listing = watch_listing(&pinned.drive, "query-and-lock");
    let registration = watch.track("query-and-lock-volume", &pinned.root()).expect("registers");

    watch.query_remove(registration);
    watch.lock(registration);
    assert!(!pinned.is_pinned(), "let go");

    watch.refuse_removal(registration);
    wait_until(Duration::from_secs(10), "the listing watch to re-arm", || {
        rearmed(&listing)
    });
    stop_watching(&listing);
    assert!(pinned.is_pinned(), "the root handle is back");
    drop(watch);
}

/// A refusal and a lock's end can both arrive: the second take-back does nothing, and the drive
/// still lets go on the next query.
#[test]
fn a_second_take_back_changes_nothing_and_the_next_query_still_lets_go() {
    let pinned = Pinned::new("two-take-backs");
    let watch = DriveWatch::started(Recorder::default());
    let listing = watch_listing(&pinned.drive, "two-take-backs");
    let registration = watch.track("two-take-backs-volume", &pinned.root()).expect("registers");

    watch.query_remove(registration);
    watch.refuse_removal(registration);
    watch.unlock(registration);
    wait_until(Duration::from_secs(10), "the listing watch to re-arm", || {
        rearmed(&listing)
    });
    assert!(pinned.is_pinned(), "taken back once");

    watch.query_remove(registration);
    assert!(!pinned.is_pinned(), "the next query lets go again");
    watch.refuse_removal(registration);
    wait_until(Duration::from_secs(10), "the listing watch to re-arm again", || {
        rearmed(&listing)
    });
    stop_watching(&listing);
    drop(watch);
}
