//! The drive watcher's worker: re-reads the drive table, announces what changed, and holds one
//! handle per local drive to hear its removal query.
//!
//! **Why a handle of our own.** Windows sends a removal query (`DBT_DEVICEQUERYREMOVE`) only to
//! whoever registered a handle on the device (`RegisterDeviceNotificationW`, `DBT_DEVTYP_HANDLE`).
//! The handles Cmdr already holds there (the index's root watch, a pane's listing watch) live
//! inside `notify`, so the worker opens one more on each drive's root, registers it, and on the
//! query closes it along with everything else, which is what lets "Safely remove" go through
//! (#1363 part 2, #1340). The pattern is Microsoft's own ("Processing a Request to Remove a
//! Device", learn.microsoft.com, read 2026-11-25).
//!
//! **Letting go** of a drive, all bounded by the window's wait:
//!
//! 1. Its handle closes.
//! 2. The drive-release gate marks an unmount pending (no start lands meanwhile) and stops its
//!    index (`Index::stop_removable_volume`), the stop macOS's unmount approver runs.
//! 3. Every listing watch on it lets go (`file_system::watcher::release::hold_at_or_below`).
//!
//! A refused removal (or an ended lock) takes it all back: the handle reopens, the watches re-arm
//! and re-read, and an index the query stopped resumes through the gate, which still checks the
//! drive's intent. A removal that went through leaves it all let go; the volume's own removal
//! then unregisters it and moves its panes.

use std::collections::{HashMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cmdr_fs::app_path::AppPath;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::windows_device_change::{diff_drives, hears_removal_queries};
use cmdr_fs::volume::windows_volume_ids::WindowsDrive;
use log::{debug, info, warn};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DBT_DEVTYP_HANDLE, DEV_BROADCAST_HANDLE, DEVICE_NOTIFY_WINDOW_HANDLE, RegisterDeviceNotificationW,
    UnregisterDeviceNotification,
};

use super::{Announce, Job};
use crate::file_system::volume::drive_release::{self, LateRelease, ResumeCandidate, ResumeOwner, VolumeRelease};
use crate::file_system::watcher::release::{HeldWatches, hold_at_or_below, take_back_after};
use crate::ignore_poison::IgnorePoison;

/// How long a query's index stop may take. Inside the window's own wait, so the answer to
/// Windows comes after it.
const INDEX_LET_GO: Duration = Duration::from_secs(6);

/// One drive the worker holds a handle on.
struct Sentinel {
    volume_id: String,
    /// Its app root (`E:/`).
    root: String,
    /// The open root handle; `None` while let go.
    handle: Option<OwnedHandle>,
    /// What a query let go of, to take back if it's refused.
    held: Option<HeldWatches>,
}

impl Sentinel {
    fn native_root(&self) -> Option<PathBuf> {
        native_root(&self.root)
    }
}

/// An app root's native spelling (`E:\`), through the app path door.
fn native_root(root: &str) -> Option<PathBuf> {
    AppPath::from_normalized(root).ok()?.to_native_plain().ok()
}

pub(super) struct Worker<A: Announce> {
    window: isize,
    known: Vec<WindowsDrive>,
    /// By registration (`HDEVNOTIFY` as an integer).
    sentinels: HashMap<usize, Sentinel>,
    announce: A,
    owner: Arc<QueryOwner>,
}

impl<A: Announce> Worker<A> {
    pub(super) fn new(window: isize, announce: A) -> Self {
        Self {
            window,
            known: Vec::new(),
            sentinels: HashMap::new(),
            announce,
            owner: Arc::new(QueryOwner::default()),
        }
    }

    pub(super) fn run(mut self, inbox: &Receiver<Job>, baseline: &SyncSender<()>) {
        // The baseline: what's here now is no news. An unreadable table starts from nothing,
        // so the first readable re-read announces what's there (registration is idempotent).
        if let Some(now) = super::super::listed_drive_table() {
            self.known = now.to_vec();
            self.owner.set_listed(&self.known);
        }
        let local: Vec<WindowsDrive> = self
            .known
            .iter()
            .filter(|d| hears_removal_queries(d))
            .cloned()
            .collect();
        for drive in &local {
            self.track(&drive.id, &drive.root);
        }
        // Nobody waits on it but a test.
        let _ = baseline.try_send(());
        while let Ok(job) = inbox.recv() {
            match job {
                Job::Rescan => self.rescan(),
                Job::LetGo { registration, done } => {
                    self.let_go(registration);
                    // a window that stopped waiting answered already
                    let _ = done.send(());
                }
                Job::TakeBack { registration } => self.take_back(registration),
                Job::Gone { registration } => {
                    self.forget(registration);
                    self.rescan();
                }
                #[cfg(test)]
                Job::Track { volume_id, root, done } => {
                    // the test is waiting on it
                    let _ = done.send(self.track(&volume_id, &root));
                }
                Job::Stop => break,
            }
        }
        let registrations: Vec<usize> = self.sentinels.keys().copied().collect();
        for registration in registrations {
            self.forget(registration);
        }
    }

    /// Re-reads the drive table and announces each root that left or arrived. A table that
    /// wouldn't answer changes nothing: ❌ never read as every drive gone.
    fn rescan(&mut self) {
        let Some(now) = super::super::listed_drive_table() else {
            warn!(target: "volumes::windows", "The drive table wouldn't answer; keeping the drives as they were");
            return;
        };
        let changes = diff_drives(&self.known, &now);
        self.known = now.to_vec();
        self.owner.set_listed(&self.known);
        if changes.is_empty() {
            return;
        }
        for drive in &changes.left {
            info!(target: "volumes::windows", "Drive left: {}", LogPrivate::dir_path(&drive.root));
            if let Some(registration) = self.registration_at(&drive.root) {
                self.forget(registration);
            }
            self.announce.left(&drive.root);
        }
        for drive in &changes.arrived {
            info!(target: "volumes::windows", "Drive arrived: {}", LogPrivate::dir_path(&drive.root));
            self.announce.arrived(&drive.root);
            if hears_removal_queries(drive) {
                self.track(&drive.id, &drive.root);
            }
        }
    }

    fn registration_at(&self, root: &str) -> Option<usize> {
        self.sentinels
            .iter()
            .find(|(_, sentinel)| sentinel.root == root)
            .map(|(registration, _)| *registration)
    }

    /// Opens a handle on `root` and registers it for the drive's removal queries. `None` when
    /// either is refused (logged): the drive still works, Windows' "Safely remove" just won't
    /// get Cmdr's handles out of its way.
    fn track(&mut self, volume_id: &str, root: &str) -> Option<usize> {
        let handle = open_root(root)?;
        let registration = register(self.window, &handle)?;
        debug!(target: "volumes::windows", "Listening for removal queries on {}", LogPrivate::dir_path(root));
        self.sentinels.insert(
            registration,
            Sentinel {
                volume_id: volume_id.to_owned(),
                root: root.to_owned(),
                handle: Some(handle),
                held: None,
            },
        );
        Some(registration)
    }

    /// A removal or lock query: let go of everything Cmdr holds on the drive.
    fn let_go(&mut self, registration: usize) {
        let Some(sentinel) = self.sentinels.get_mut(&registration) else {
            debug!(target: "volumes::windows", "A removal query for a drive no longer held");
            return;
        };
        info!(target: "volumes::windows", "Letting go of {} for its removal", LogPrivate::dir_path(&sentinel.root));
        drop(sentinel.handle.take());
        let ids = vec![sentinel.volume_id.clone()];
        let gate = drive_release::gate();
        gate.set_unmount_pending(&ids);
        if let Some(root) = sentinel.native_root()
            && let Some(held) = hold_at_or_below(&root)
        {
            debug!(target: "volumes::windows", "Let go of {} listing watch(es)", held.len());
            // A second query while held keeps the first hold: its watches are already let go.
            sentinel.held.get_or_insert(held);
        }
        let owner = Arc::clone(&self.owner);
        let release = gate.release(
            &ids,
            Instant::now() + INDEX_LET_GO,
            |id| crate::index_host::index().stop_removable_volume(id, INDEX_LET_GO),
            move |late: LateRelease| owner.note(&late.volume_id, late.outcome, late.epoch),
        );
        for released in release.volumes {
            let outcome = released.outcome;
            self.owner.note(&released.volume_id, outcome, released.epoch);
            debug!(target: "volumes::windows", "Index of {} for the removal: {outcome:?}", released.volume_id);
        }
    }

    /// The removal was refused or the lock ended: take back what the query let go of.
    fn take_back(&mut self, registration: usize) {
        let Some(sentinel) = self.sentinels.get_mut(&registration) else {
            return;
        };
        info!(target: "volumes::windows", "Taking {} back: its removal didn't happen", LogPrivate::dir_path(&sentinel.root));
        if sentinel.handle.is_none() {
            sentinel.handle = open_root(&sentinel.root);
        }
        let ids = vec![sentinel.volume_id.clone()];
        let gate = drive_release::gate();
        gate.clear_unmount_pending(&ids);
        if let Some(held) = sentinel.held.take() {
            take_back_after(held, Duration::ZERO);
        }
        if let Some(epoch) = self.owner.epoch_of(&sentinel.volume_id) {
            let candidate = ResumeCandidate {
                volume_id: sentinel.volume_id.clone(),
                epoch,
            };
            let owner: Arc<dyn ResumeOwner> = Arc::clone(&self.owner) as Arc<dyn ResumeOwner>;
            drop(gate.resume(vec![candidate], owner));
        }
    }

    /// The drive is going or gone: close its handle and end its registration. What a query let
    /// go of stays let go; the volume's removal moves its panes.
    fn forget(&mut self, registration: usize) {
        let Some(sentinel) = self.sentinels.remove(&registration) else {
            return;
        };
        drop(sentinel.handle);
        // SAFETY: `registration` is the `HDEVNOTIFY` `register` got, ended once: it's out of the
        // map now.
        unsafe { UnregisterDeviceNotification(registration as *mut core::ffi::c_void) };
        drive_release::gate().clear_unmount_pending(std::slice::from_ref(&sentinel.volume_id));
        self.owner.forget(&sentinel.volume_id);
    }
}

/// A handle on `root`'s folder, any sharing allowed, so it's in nobody's way but a removal's.
fn open_root(root: &str) -> Option<OwnedHandle> {
    let native = native_root(root)?;
    let wide: Vec<u16> = native.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated path that outlives the call; no security attributes
    // and no template. `FILE_FLAG_BACKUP_SEMANTICS` is what opens a folder.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        debug!(target: "volumes::windows", "Couldn't open {} to hear its removal (error {code})", LogPrivate::dir_path(root));
        return None;
    }
    // SAFETY: a valid handle `CreateFileW` just opened, owned by nothing else.
    Some(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Registers `handle`'s device for removal queries, sent to `window`.
fn register(window: isize, handle: &OwnedHandle) -> Option<usize> {
    let filter = DEV_BROADCAST_HANDLE {
        dbch_size: u32::try_from(size_of::<DEV_BROADCAST_HANDLE>()).ok()?,
        dbch_devicetype: DBT_DEVTYP_HANDLE,
        dbch_handle: handle.as_raw_handle() as HANDLE,
        ..DEV_BROADCAST_HANDLE::default()
    };
    // SAFETY: `window` is the watcher's window, alive while the worker runs (the watch closes it
    // only after the worker ends); `filter` is a filled `DEV_BROADCAST_HANDLE` that outlives the
    // call, and Windows copies it.
    let registration = unsafe {
        RegisterDeviceNotificationW(
            window as HANDLE,
            (&raw const filter).cast(),
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
    };
    if registration.is_null() {
        // SAFETY: reads this thread's last-error value, right after the call that set it.
        let code = unsafe { GetLastError() };
        warn!(target: "volumes::windows", "Couldn't listen for a drive's removal queries (error {code})");
        return None;
    }
    Some(registration as usize)
}

/// The worker as a resume owner: it stands behind the index a removal query stopped until the
/// drive leaves or a newer query moves its epoch, and reads presence from its last drive table.
#[derive(Default)]
struct QueryOwner {
    /// Volume id to the epoch its query's release set, for an index that was running.
    stopped: Mutex<HashMap<String, u64>>,
    listed: Mutex<HashSet<String>>,
}

impl QueryOwner {
    fn set_listed(&self, drives: &[WindowsDrive]) {
        *self.listed.lock_ignore_poison() = drives.iter().map(|drive| drive.id.clone()).collect();
    }

    /// Records a stop the query made, so a refusal resumes it. Only an index that was running.
    fn note(&self, volume_id: &str, outcome: VolumeRelease, epoch: u64) {
        if outcome == (VolumeRelease::Released { was_indexing: true }) {
            self.stopped.lock_ignore_poison().insert(volume_id.to_owned(), epoch);
        }
    }

    fn epoch_of(&self, volume_id: &str) -> Option<u64> {
        self.stopped.lock_ignore_poison().get(volume_id).copied()
    }

    fn forget(&self, volume_id: &str) {
        self.stopped.lock_ignore_poison().remove(volume_id);
    }
}

impl ResumeOwner for QueryOwner {
    fn name(&self) -> &'static str {
        "the Windows drive watcher"
    }

    fn still_owns(&self, candidate: &ResumeCandidate) -> bool {
        self.epoch_of(&candidate.volume_id) == Some(candidate.epoch)
    }

    fn is_listed(&self, candidate: &ResumeCandidate) -> bool {
        self.listed.lock_ignore_poison().contains(&candidate.volume_id)
    }

    fn is_ejected_by_another_owner(&self, _candidate: &ResumeCandidate) -> bool {
        // Cmdr has no eject of its own on Windows yet (task 39b).
        false
    }

    fn consume(&self, candidate: &ResumeCandidate) {
        self.forget(&candidate.volume_id);
    }
}
