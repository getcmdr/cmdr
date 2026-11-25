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
//! 3. Every listing watch on it lets go and stays let go (`file_system::watcher::release::hold_drive`),
//!    a Cmdr write's quiet timer included.
//!
//! ❗ **One let-go per drive at a time.** A removal can bring a query AND a volume lock, and a
//! refusal can bring two take-backs: a second let-go only notes a lock, and a take-back with
//! nothing let go does nothing. A second release would move the drive's epoch and void the
//! resume; a second hold would orphan the first's watches.
//!
//! **Cmdr's own eject** lets go the same way, minus the index stop (the eject's flight made it,
//! `file_system/volume/eject/platform/drive_flight.rs`), and owns that let-go: a refusal or lock's end
//! Windows sends meanwhile doesn't take it back, since the eject's own lock attempt would then
//! meet the handle it just closed. Only the eject's take-back (`Job::EjectTakeBack`) or the
//! drive leaving ends it.
//!
//! A refused removal (or an ended lock) takes it all back: the handle reopens, the watches re-arm
//! and re-read, and an index the query stopped resumes through the gate, which still checks the
//! drive's intent. After a lock it rescans instead: a format or `chkdsk /f` rewrote the drive
//! under the same id. A removal that went through leaves it all let go; the volume's own removal
//! then unregisters it, moves its panes, and only then lifts the unmount-pending flag.

use std::collections::{HashMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cmdr_fs::app_path::AppPath;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::windows_device_change::{LetGoCause, diff_drives, hears_removal_queries, sort_leaving};
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
use crate::file_system::volume::drive_release::{
    self, Gated, LateRelease, ResumeCandidate, ResumeOwner, StartKind, VolumeRelease,
};
use crate::file_system::watcher::release::{forget_drive, hold_drive, take_back_drive};
use crate::ignore_poison::IgnorePoison;

/// How long a query's index stop may take. Inside the window's own wait, so the answer to
/// Windows comes after it.
const INDEX_LET_GO: Duration = Duration::from_secs(6);

/// How long after an unreadable drive table the worker reads it once more: a pull can leave the
/// mount table briefly unreadable, and no later signal is due.
const RETRY_UNREADABLE: Duration = Duration::from_secs(2);

/// How long the worker asks a share whose mapped letter went whether it still answers. A dead
/// server would hold the read for minutes; past this the share leaves with its letter.
const SHARE_PROBE_WAIT: Duration = Duration::from_secs(2);

/// Whether the UNC root `root` (`//server/share/`) answers a stat within `wait`. The stat runs
/// on a thread of its own, left to finish alone when it outlives the wait.
fn answers_within(root: &str, wait: Duration) -> bool {
    let Some(native) = native_root(root) else {
        return false;
    };
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("share-probe".to_owned())
        .spawn(move || {
            // The receiver may have stopped waiting.
            let _ = tx.send(std::fs::metadata(&native).is_ok_and(|meta| meta.is_dir()));
        });
    spawned.is_ok() && rx.recv_timeout(wait).unwrap_or(false)
}

/// One drive the worker holds a handle on.
struct Sentinel {
    volume_id: String,
    /// Its app root (`E:/`).
    root: String,
    /// The open root handle; `None` while let go.
    handle: Option<OwnedHandle>,
    /// The let-go in force, if any.
    let_go: Option<LetGo>,
}

/// A let-go in force on a drive.
struct LetGo {
    /// Whether a volume lock came with it (or alone), so the take-back rescans.
    locked: bool,
    /// Whether Cmdr's own eject holds it, so only the eject's take-back ends it.
    by_eject: bool,
}

/// Who asks for a take-back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asker {
    /// Windows: a refused removal, a failed lock, a lock's end.
    Windows,
    /// Cmdr's own eject, which didn't go through.
    Eject,
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
    /// Volumes with an unmount pending at the gate, lifted when the drive leaves the table or
    /// its removal is refused.
    pending: HashSet<String>,
    /// When to read an unreadable table once more.
    retry_at: Option<Instant>,
    /// Roots Cmdr's eject let go of with no handle of the worker's on them (its registration
    /// was refused): their listing watches are held until the take-back or the drive leaving.
    untracked_holds: HashSet<String>,
    announce: A,
    owner: Arc<QueryOwner>,
}

impl<A: Announce> Worker<A> {
    pub(super) fn new(window: isize, announce: A) -> Self {
        Self {
            window,
            known: Vec::new(),
            sentinels: HashMap::new(),
            pending: HashSet::new(),
            retry_at: None,
            untracked_holds: HashSet::new(),
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
            // Which drives offer Eject reads a device probe, made here, off every listing path.
            super::super::storage::probe_soon(&self.known, crate::volume_broadcast::emit_volumes_changed);
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
        loop {
            let job = match self.retry_at {
                Some(at) => match inbox.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        self.retry_at = None;
                        self.rescan_once_more();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match inbox.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                },
            };
            match job {
                Job::Rescan => self.rescan(),
                Job::LetGo {
                    registration,
                    cause,
                    done,
                } => {
                    self.let_go(registration, cause);
                    // A window that stopped waiting answered already.
                    let _ = done.send(());
                }
                Job::TakeBack { registration } => self.take_back(registration, Asker::Windows),
                Job::EjectLetGo { volume_id, root, done } => {
                    self.let_go_for_eject(&volume_id, &root);
                    // An eject that stopped waiting goes ahead without the answer.
                    let _ = done.send(());
                }
                Job::EjectTakeBack { volume_id, root, done } => {
                    self.take_back_for_eject(&volume_id, &root);
                    let _ = done.send(());
                }
                Job::Gone { registration } => {
                    self.forget(registration);
                    self.rescan();
                }
                #[cfg(test)]
                Job::Track { volume_id, root, done } => {
                    // The test is waiting on it.
                    let _ = done.send(self.track(&volume_id, &root));
                }
                Job::Stop => break,
            }
        }
        let registrations: Vec<usize> = self.sentinels.keys().copied().collect();
        for registration in registrations {
            self.forget(registration);
        }
        let pending: Vec<String> = self.pending.drain().collect();
        drive_release::gate().clear_unmount_pending(&pending);
    }

    /// Re-reads the drive table and announces each root that left or arrived. A table that
    /// wouldn't answer changes nothing (❌ never read as every drive gone) and is read once more
    /// shortly.
    fn rescan(&mut self) {
        if !self.read_table() {
            self.retry_at = Some(Instant::now() + RETRY_UNREADABLE);
        }
    }

    /// The one retry after an unreadable table; a second failure waits for the next signal.
    fn rescan_once_more(&mut self) {
        if !self.read_table() {
            warn!(target: "volumes::windows", "The drive table still wouldn't answer; waiting for the next drive signal");
        }
    }

    /// One read and diff. `false` when the table wouldn't answer.
    fn read_table(&mut self) -> bool {
        let Some(now) = super::super::listed_drive_table() else {
            warn!(target: "volumes::windows", "The drive table wouldn't answer; keeping the drives as they were");
            return false;
        };
        self.retry_at = None;
        let changes = diff_drives(&self.known, &now);
        self.known = now.to_vec();
        self.owner.set_listed(&self.known);
        let leaving = sort_leaving(&changes.left, |alias| answers_within(&alias.root, SHARE_PROBE_WAIT));
        // First, so the letter's leave below promotes the share's root instead of ending the
        // volume a UNC pane is on (#1492). Registration is idempotent.
        for share in &leaving.kept_shares {
            info!(target: "volumes::windows", "Keeping share {}: its mapped letter went, the share still answers", LogPrivate::dir_path(&share.root));
            self.announce.arrived(&share.root);
        }
        for drive in &leaving.gone {
            info!(target: "volumes::windows", "Drive left: {}", LogPrivate::dir_path(&drive.root));
            if let Some(registration) = self.registration_at(&drive.root) {
                self.forget(registration);
            }
            if self.untracked_holds.remove(&drive.root)
                && let Some(root) = native_root(&drive.root)
            {
                forget_drive(&root);
            }
            self.announce.left(&drive.root);
            // Only now, with the root gone from the table and the registry: no index start can
            // land on the drive any more.
            self.lift_pending(&drive.id);
        }
        super::super::storage::probe_soon(&changes.arrived, crate::volume_broadcast::emit_volumes_changed);
        for drive in &changes.arrived {
            info!(target: "volumes::windows", "Drive arrived: {}", LogPrivate::dir_path(&drive.root));
            self.lift_pending(&drive.id);
            self.announce.arrived(&drive.root);
            if hears_removal_queries(drive) {
                self.track(&drive.id, &drive.root);
            }
        }
        true
    }

    fn lift_pending(&mut self, volume_id: &str) {
        if self.pending.remove(volume_id) {
            drive_release::gate().clear_unmount_pending(&[volume_id.to_owned()]);
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
                let_go: None,
            },
        );
        Some(registration)
    }

    /// A removal or lock query: let go of everything Cmdr holds on the drive, once.
    fn let_go(&mut self, registration: usize, cause: LetGoCause) {
        let Some(sentinel) = self.sentinels.get_mut(&registration) else {
            debug!(target: "volumes::windows", "A removal query for a drive no longer held");
            return;
        };
        let locked = cause == LetGoCause::Lock;
        if let Some(let_go) = sentinel.let_go.as_mut() {
            // Already let go: a second release would move the epoch and void the resume.
            let_go.locked |= locked;
            debug!(target: "volumes::windows", "A second {cause:?} query for a drive already let go");
            return;
        }
        info!(target: "volumes::windows", "Letting go of {} for {cause:?}", LogPrivate::dir_path(&sentinel.root));
        sentinel.let_go = Some(LetGo {
            locked,
            by_eject: false,
        });
        drop(sentinel.handle.take());
        let volume_id = sentinel.volume_id.clone();
        let root = sentinel.native_root();
        let ids = vec![volume_id.clone()];
        let gate = drive_release::gate();
        gate.set_unmount_pending(&ids);
        self.pending.insert(volume_id);
        if let Some(root) = root {
            let closed = hold_drive(&root);
            debug!(target: "volumes::windows", "Let go of {closed} listing watch(es)");
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

    /// Cmdr's own eject is about to ask Windows: let go of everything the worker holds on the
    /// drive but the index (the eject stopped it), as the eject's. A let-go already in force (a
    /// removal query that came first) becomes the eject's.
    fn let_go_for_eject(&mut self, volume_id: &str, root: &str) {
        info!(target: "volumes::windows", "Letting go of {} for Cmdr's own eject", LogPrivate::dir_path(root));
        if let Some(registration) = self.registration_at(root)
            && let Some(sentinel) = self.sentinels.get_mut(&registration)
        {
            if let Some(let_go) = sentinel.let_go.as_mut() {
                let_go.by_eject = true;
                return;
            }
            sentinel.let_go = Some(LetGo {
                locked: false,
                by_eject: true,
            });
            drop(sentinel.handle.take());
        } else if !self.untracked_holds.insert(root.to_owned()) {
            return;
        }
        let ids = [volume_id.to_owned()];
        drive_release::gate().set_unmount_pending(&ids);
        self.pending.insert(volume_id.to_owned());
        if let Some(native) = native_root(root) {
            let closed = hold_drive(&native);
            debug!(target: "volumes::windows", "Let go of {closed} listing watch(es)");
        }
    }

    /// Cmdr's own eject didn't go through: take the drive back.
    fn take_back_for_eject(&mut self, volume_id: &str, root: &str) {
        if let Some(registration) = self.registration_at(root) {
            self.take_back(registration, Asker::Eject);
        } else if self.untracked_holds.remove(root) {
            info!(target: "volumes::windows", "Taking {} back: its eject didn't happen", LogPrivate::dir_path(root));
            if let Some(native) = native_root(root) {
                take_back_drive(&native);
            }
            self.lift_pending(volume_id);
        }
    }

    /// The removal was refused, the lock ended, or Cmdr's eject didn't go through: take back what
    /// the let-go let go of. Nothing when nothing is let go (a refusal and a lock's end can both
    /// arrive), and nothing for Windows' signals while Cmdr's eject holds the let-go.
    fn take_back(&mut self, registration: usize, asker: Asker) {
        let Some(sentinel) = self.sentinels.get_mut(&registration) else {
            return;
        };
        if asker == Asker::Windows && sentinel.let_go.as_ref().is_some_and(|let_go| let_go.by_eject) {
            debug!(target: "volumes::windows", "A take-back Windows asked for while Cmdr's own eject holds the drive");
            return;
        }
        let Some(let_go) = sentinel.let_go.take() else {
            debug!(target: "volumes::windows", "A take-back for a drive that wasn't let go");
            return;
        };
        info!(target: "volumes::windows", "Taking {} back: its removal didn't happen", LogPrivate::dir_path(&sentinel.root));
        if sentinel.handle.is_none() {
            sentinel.handle = open_root(&sentinel.root);
        }
        let volume_id = sentinel.volume_id.clone();
        if let Some(root) = sentinel.native_root() {
            take_back_drive(&root);
        }
        self.lift_pending(&volume_id);
        let Some(epoch) = self.owner.epoch_of(&volume_id) else {
            return;
        };
        if let_go.locked {
            // A lock may have rewritten the drive under the same id (a format, `chkdsk /f`): its
            // rows can't be trusted, so it walks again instead of resuming as it was.
            self.owner.forget(&volume_id);
            rescan_after_a_lock(volume_id);
        } else {
            let candidate = ResumeCandidate { volume_id, epoch };
            let owner: Arc<dyn ResumeOwner> = Arc::clone(&self.owner) as Arc<dyn ResumeOwner>;
            drop(drive_release::gate().resume(vec![candidate], owner));
        }
    }

    /// The drive is going or gone: close its handle, end its registration, and stop holding its
    /// watches. What a query let go of stays let go; the unmount-pending flag lifts once the
    /// drive leaves the table (`read_table`).
    fn forget(&mut self, registration: usize) {
        let Some(sentinel) = self.sentinels.remove(&registration) else {
            return;
        };
        drop(sentinel.handle);
        // SAFETY: `registration` is the `HDEVNOTIFY` `register` got, ended once: it's out of the
        // map now.
        unsafe { UnregisterDeviceNotification(registration as *mut core::ffi::c_void) };
        if let Some(root) = native_root(&sentinel.root) {
            forget_drive(&root);
        }
        self.owner.forget(&sentinel.volume_id);
    }
}

/// An index a lock stopped walks its drive again, through the gate like every non-root start.
/// A person's rescan is the start this is (the lock only made the index's rows untrustworthy);
/// the take-back has lifted the unmount-pending flag already, so it doesn't wait on one.
fn rescan_after_a_lock(volume_id: String) {
    tauri::async_runtime::spawn(async move {
        let id = volume_id.clone();
        let gated = drive_release::gate()
            .start(&volume_id, StartKind::UserRescan, move || async move {
                crate::index_host::index().rescan_volume(&id).await
            })
            .await;
        match gated {
            Gated::Ran(Ok(outcome)) => {
                info!(target: "volumes::windows", "Rescanning {volume_id} after its lock: {outcome:?}");
            }
            Gated::Ran(Err(e)) => {
                warn!(target: "volumes::windows", "{volume_id} couldn't rescan after its lock: {e}");
            }
            Gated::Skipped(reason) => {
                info!(target: "volumes::windows", "{volume_id} didn't rescan after its lock: {reason:?}");
            }
        }
    });
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

    fn is_ejected_by_another_owner(&self, candidate: &ResumeCandidate) -> bool {
        // Cmdr's own eject stops and hands back its index itself (`eject::drive_flight`).
        crate::file_system::volume::eject::is_ejecting(&candidate.volume_id)
    }

    fn consume(&self, candidate: &ResumeCandidate) {
        self.forget(&candidate.volume_id);
    }
}
