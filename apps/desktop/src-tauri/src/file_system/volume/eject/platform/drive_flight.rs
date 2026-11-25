//! Windows: Cmdr's own eject of a drive (task 39b), the sequence every route shares.
//!
//! An eject takes a whole UNIT: every volume the request takes down with the one clicked (each
//! partition of a stick, each slot of a card reader, each volume of a VHD). The flight acts on all
//! of them as one, since a copy cut off on partition 2 is as cut off as one on partition 1.
//!
//! 1. **Busy check** over the unit, once more right before anything lets go: a transfer that
//!    started after `eject_now`'s gate (or one on a sibling, which that gate doesn't see) would
//!    otherwise be cut off. The unit is already in the ejecting set, so a transfer that starts
//!    from here on waits in the queue (`OperationVolumes::is_ejecting`).
//! 2. **Index stop** of every volume through the drive-release gate, under
//!    `INDEX_STOP_DEADLINE`: an index that may still hold the drive never meets the request.
//! 3. **Let go** ([`DriveSide::let_go`]) of every volume: the drive watcher closes its own handle,
//!    holds every listing watch, and marks an unmount pending, the let-go a removal query gets
//!    (`volumes/DETAILS.md` § "Windows: drive arrival and removal"), minus the index stop this
//!    flight already made. ❗ That let-go is the EJECT's: a refusal Windows sends the watcher
//!    meanwhile doesn't take it back, since a lock attempt's failure would re-open the handle
//!    under the flight's next step.
//! 4. **Request** ([`DriveSide::request`]) on a thread of its own, under [`REQUEST_DEADLINE`]:
//!    the route `cmdr_fs::volume::windows_eject` picked.
//! 5. **Answer.** Ejected: the drive leaves through the watcher's own removal signals (the panes
//!    fall back as on a pulled stick, `volume-unmounted` fires), so nothing here announces it.
//!    Refused, impossible, or panicked: [`DriveSide::take_back`] re-arms the watches and the
//!    handles and lifts the pending flags, and every index this flight stopped comes back through
//!    the gate. Timed out: the answer goes out at once, but the unit STAYS in the ejecting set
//!    (the `linger` hook: `in_flight::disk::DiskOwnership::hold_past_landing`) until the request
//!    truly ends and its take-back ran, so a second eject joins this one instead of letting go
//!    under it.
//!
//! ❌ Never forced: a veto is the honest `UnmountRefused`, and the person closes what holds it.
//!
//! The sequence is pure over [`DriveSide`] and an injected gate and stop, so it's tested on every
//! OS; the Windows side is `platform/windows.rs`.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use cmdr_index::RemovableStop;

use super::super::holders::HolderScan;
use super::super::{EjectError, EjectOutcome, IndexStopped};
use crate::file_system::volume::drive_release::{
    DriveRelease, ResumeBatch, ResumeCandidate, ResumeOwner, VolumeRelease,
};

/// How long Windows gets to answer an eject. `CM_Request_Device_EjectW` asks every app and
/// service holding a handle first and waits on their answers; the lock route retries its lock.
/// Same budget as a `diskutil` run (`unmount_tool::TOOL_TIMEOUT`).
pub(super) const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// How Windows answered the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RequestAnswer {
    /// The device, disc, or image went.
    Ejected,
    /// Someone holds the drive. `detail` (the veto type and name, or the lock's error) is for
    /// the log and the details line.
    Vetoed { detail: String },
    /// Windows wants an administrator's rights for this (a VHD's lock or detach). Nobody holds
    /// the drive, so ❌ never worded as a veto.
    NeedsRights { detail: String },
    /// The request couldn't be made, or Windows turned it down for no holder's sake.
    NotPossible { detail: String },
}

/// What the flight asks of the world: the watcher's let-go and take-back, the request, and each
/// volume's presence. Every method acts on the whole unit.
pub(super) trait DriveSide: Send + Sync + 'static {
    /// Let go of every volume of the unit: Cmdr's own handle, its listing watches, and a pending
    /// unmount. Returns once done, or once the watcher stopped answering.
    fn let_go(&self);
    /// Ask Windows to eject the unit. Blocking, and can take as long as Windows takes.
    fn request(&self) -> RequestAnswer;
    /// Take back what [`Self::let_go`] let go of. Returns once done.
    fn take_back(&self);
    /// Whether `volume_id` is still listed: the resume's presence check.
    fn is_listed(&self, volume_id: &str) -> bool;
}

/// Called at a timeout, before the answer goes out: keeps the unit in the ejecting set and hands
/// back what ends that hold, called once the request truly ends and its take-back ran.
pub(super) type Linger = Box<dyn FnOnce() -> Box<dyn FnOnce() + Send> + Send>;

/// How a flight ended, and the index hand-back it started, which a test waits on.
pub(super) struct FlightEnd {
    pub(super) answer: Result<EjectOutcome, EjectError>,
    #[cfg_attr(not(test), expect(dead_code, reason = "only a test waits for a hand-back"))]
    pub(super) handed_back: Option<ResumeBatch>,
}

impl FlightEnd {
    fn answer(answer: Result<EjectOutcome, EjectError>) -> Self {
        Self {
            answer,
            handed_back: None,
        }
    }
}

/// Runs the eject of `unit` (module docs), the clicked volume first. Blocking: up to
/// `INDEX_STOP_DEADLINE` plus [`REQUEST_DEADLINE`] (`request_deadline` in a test).
pub(super) fn eject_drive(
    gate: &'static DriveRelease,
    unit: &[String],
    side: Arc<dyn DriveSide>,
    is_busy: impl Fn(&str) -> bool,
    stop: impl Fn(&str) -> RemovableStop + Send + Sync + 'static,
    request_deadline: Duration,
    linger: Linger,
) -> FlightEnd {
    if let Some(busy) = unit.iter().find(|id| is_busy(id)) {
        log::info!(target: "eject", "Not ejecting {unit:?}: an operation is running on {busy}");
        return FlightEnd::answer(Err(EjectError::Busy));
    }
    let release = match super::super::release_for_unmount(gate, unit, stop, |late| {
        log::info!(
            target: "eject",
            "the index for {} answered after the eject stopped waiting: {:?}",
            late.volume_id,
            late.outcome
        );
    }) {
        IndexStopped::LetGo(release) => release,
        // A stuck stop's continuation hands its own answer back, and the volumes that did let
        // go come back here: nothing else was touched.
        IndexStopped::Refused { release, error } => {
            let owed = release.as_ref().map(owed_ids).unwrap_or_default();
            hand_back_index(gate, &side, owed);
            return FlightEnd::answer(Err(error));
        }
    };
    let owed = owed_ids(&release);

    side.let_go();

    let (answered, answer) = mpsc::sync_channel(1);
    let requester = Arc::clone(&side);
    let spawned = std::thread::Builder::new()
        .name("drive-eject".to_owned())
        .spawn(move || {
            // A receiver that stopped waiting (a timeout) still gets its answer from the
            // continuation below.
            let _ = answered.send(requester.request());
        });
    let undo = Undo { gate, side, owed };
    if let Err(e) = spawned {
        return undo.run(Err(EjectError::Unexpected {
            detail: format!("couldn't start the eject request: {e}"),
        }));
    }
    let volume_ids = unit.join(", ");
    match answer.recv_timeout(request_deadline) {
        Ok(answer) => settle(&volume_ids, answer, undo),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            log::warn!(target: "eject", "The eject request for {volume_ids} ended without an answer");
            undo.run(Err(EjectError::Unexpected {
                detail: "the eject request ended without an answer".to_owned(),
            }))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            log::warn!(target: "eject", "Windows hadn't answered the eject of {volume_ids} in time; it may still land");
            // The request wasn't cancelled (it can't be): the unit stays ejecting, and what the
            // request ends in decides the take-back and what a joined eject hears.
            let release_hold = linger();
            let continued = std::thread::Builder::new()
                .name("drive-eject-end".to_owned())
                .spawn(move || {
                    let end = match answer.recv() {
                        Ok(late) => settle(&volume_ids, late, undo),
                        Err(_) => undo.run(Err(EjectError::Unexpected {
                            detail: "the eject request ended without an answer".to_owned(),
                        })),
                    };
                    log::info!(target: "eject", "The eject of {volume_ids} that timed out ended: {:?}", end.answer);
                    release_hold();
                });
            if let Err(e) = continued {
                log::warn!(target: "eject", "Couldn't wait for the eject request's end ({e}); the drive stays let go until it leaves or Cmdr restarts");
            }
            FlightEnd::answer(Err(EjectError::TimedOut))
        }
    }
}

/// What the request's answer means, taking back on anything but an eject.
fn settle(volume_ids: &str, answer: RequestAnswer, undo: Undo) -> FlightEnd {
    match answer {
        RequestAnswer::Ejected => {
            log::info!(target: "eject", "Windows ejected {volume_ids}");
            FlightEnd::answer(Ok(EjectOutcome::DiskEjected))
        }
        RequestAnswer::Vetoed { detail } => {
            log::warn!(target: "eject", "Windows refused to eject {volume_ids}, since something holds it: {detail}");
            undo.run(Err(EjectError::UnmountRefused {
                // Windows names a device or a module, ❌ never a process Cmdr could word; the
                // scan that would (the Restart Manager) isn't wired up, so nobody is named.
                holders: HolderScan::not_scanned(),
                detail,
            }))
        }
        RequestAnswer::NeedsRights { detail } => {
            log::warn!(target: "eject", "Windows wants an administrator's rights to eject {volume_ids}: {detail}");
            undo.run(Err(EjectError::NeedsAdministrator { detail }))
        }
        RequestAnswer::NotPossible { detail } => {
            log::warn!(target: "eject", "Windows couldn't eject {volume_ids}: {detail}");
            undo.run(Err(EjectError::Unexpected { detail }))
        }
    }
}

/// Every volume the release stopped that was indexing before it: what the flight owes back.
fn owed_ids(release: &crate::file_system::volume::drive_release::Release) -> Vec<String> {
    release
        .volumes
        .iter()
        .filter(|volume| volume.outcome == (VolumeRelease::Released { was_indexing: true }))
        .map(|volume| volume.volume_id.clone())
        .collect()
}

/// What a flight that didn't eject hands back.
struct Undo {
    gate: &'static DriveRelease,
    side: Arc<dyn DriveSide>,
    /// The volumes whose index the flight stopped.
    owed: Vec<String>,
}

impl Undo {
    /// [`Self::hand_back`], ending the flight with `answer`.
    fn run(self, answer: Result<EjectOutcome, EjectError>) -> FlightEnd {
        FlightEnd {
            answer,
            handed_back: self.hand_back(),
        }
    }

    /// The watcher's take-back first (it lifts the unmount-pending flags a resume checks), then
    /// every index this flight stopped, through the gate.
    fn hand_back(self) -> Option<ResumeBatch> {
        self.side.take_back();
        hand_back_index(self.gate, &self.side, self.owed)
    }
}

/// Resumes `owed` through the gate.
///
/// ❗ Each epoch is read NOW, ❌ not at the stop: a removal query Windows sent the watcher
/// during the request may have released a volume again (its sentinel missed the let-go), and a
/// stale epoch would leave that index stopped for good.
fn hand_back_index(gate: &DriveRelease, side: &Arc<dyn DriveSide>, owed: Vec<String>) -> Option<ResumeBatch> {
    if owed.is_empty() {
        return None;
    }
    log::info!(target: "eject", "{owed:?} stayed, so they get their index back");
    let candidates = owed
        .into_iter()
        .map(|volume_id| ResumeCandidate {
            epoch: gate.epoch(&volume_id),
            volume_id,
        })
        .collect();
    let owner = Arc::new(DriveResume { side: Arc::clone(side) });
    Some(gate.resume(candidates, owner as Arc<dyn ResumeOwner>))
}

/// The flight as a resume owner: the index it stopped comes back while the drive is listed and
/// nobody else is ejecting it.
struct DriveResume {
    side: Arc<dyn DriveSide>,
}

impl ResumeOwner for DriveResume {
    fn name(&self) -> &'static str {
        "Cmdr's own eject"
    }

    fn still_owns(&self, _candidate: &ResumeCandidate) -> bool {
        // A flight offers its volumes once and never outlives the offer.
        true
    }

    fn is_listed(&self, candidate: &ResumeCandidate) -> bool {
        self.side.is_listed(&candidate.volume_id)
    }

    fn is_ejected_by_another_owner(&self, candidate: &ResumeCandidate) -> bool {
        // The resume settles after this flight (and any hold of it) landed, so any eject in
        // flight then is another.
        super::super::in_flight::is_ejecting(&candidate.volume_id)
    }

    fn consume(&self, _candidate: &ResumeCandidate) {}
}

#[cfg(test)]
mod tests;
