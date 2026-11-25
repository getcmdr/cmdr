//! Windows: Cmdr's own eject of a drive (task 39b), the sequence every route shares.
//!
//! 1. **Busy check**, once more right before anything lets go: a transfer that started after
//!    `eject_now`'s gate would otherwise be cut off.
//! 2. **Index stop** through the drive-release gate, under `INDEX_STOP_DEADLINE`: an index that
//!    may still hold the drive never meets the request.
//! 3. **Let go** ([`DriveSide::let_go`]): the drive watcher closes its own handle on the drive,
//!    holds every listing watch there, and marks an unmount pending, the let-go a removal query
//!    gets (`volumes/DETAILS.md` § "Windows: drive arrival and removal"), minus the index stop
//!    this flight already made. ❗ That let-go is the EJECT's: a refusal Windows sends the watcher
//!    meanwhile doesn't take it back, since a lock attempt's failure would re-open the handle
//!    under the flight's next step.
//! 4. **Request** ([`DriveSide::request`]) on a thread of its own, under [`REQUEST_DEADLINE`]:
//!    the route `cmdr_fs::volume::windows_eject` picked.
//! 5. **Answer.** Ejected: the drive leaves through the watcher's own removal signals (the panes
//!    fall back as on a pulled stick, `volume-unmounted` fires), so nothing here announces it.
//!    Refused, impossible, or panicked: [`DriveSide::take_back`] re-arms the watches and the
//!    handle and lifts the pending flag, and the index this flight stopped comes back through the
//!    gate. Timed out: the answer goes out at once, and the take-back waits for the request's own
//!    end, since the eject may still land.
//!
//! ❌ Never forced: a veto is the honest `UnmountRefused`, and the person closes what holds it.
//!
//! The sequence is pure over [`DriveSide`] and an injected gate and stop, so it's tested on every
//! OS; the Windows side is `platform/windows/`.

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
    /// The request couldn't be made, or Windows turned it down for no holder's sake.
    NotPossible { detail: String },
}

/// What the flight asks of the world: the watcher's let-go and take-back, the request, and the
/// drive's presence.
pub(super) trait DriveSide: Send + Sync + 'static {
    /// Let go of the drive: Cmdr's own handle, its listing watches, and a pending unmount.
    /// Returns once done, or once the watcher stopped answering.
    fn let_go(&self);
    /// Ask Windows to eject the drive. Blocking, and can take as long as Windows takes.
    fn request(&self) -> RequestAnswer;
    /// Take back what [`Self::let_go`] let go of. Returns once done.
    fn take_back(&self);
    /// Whether the drive is still listed: the resume's presence check.
    fn is_listed(&self) -> bool;
}

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

/// Runs the eject of `volume_id` (module docs). Blocking: up to `INDEX_STOP_DEADLINE` plus
/// [`REQUEST_DEADLINE`] (`request_deadline` in a test).
pub(super) fn eject_drive(
    gate: &'static DriveRelease,
    volume_id: &str,
    side: Arc<dyn DriveSide>,
    is_busy: impl FnOnce() -> bool,
    stop: impl Fn(&str) -> RemovableStop + Send + Sync + 'static,
    request_deadline: Duration,
) -> FlightEnd {
    if is_busy() {
        return FlightEnd::answer(Err(EjectError::Busy));
    }
    let ids = [volume_id.to_string()];
    let late_id = volume_id.to_string();
    let release = match super::super::release_for_unmount(gate, &ids, stop, move |late| {
        log::info!(target: "eject", "the index for {late_id} answered after the eject stopped waiting: {:?}", late.outcome);
    }) {
        IndexStopped::LetGo(release) => release,
        // Nothing let go but what the release says, and a stuck stop's continuation hands its
        // own answer back: the drive stays as it is.
        IndexStopped::Refused { error, .. } => return FlightEnd::answer(Err(error)),
    };
    let was_indexing = release
        .volumes
        .iter()
        .any(|volume| volume.outcome == (VolumeRelease::Released { was_indexing: true }));

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
    let undo = Undo {
        gate,
        volume_id: volume_id.to_string(),
        side,
        was_indexing,
    };
    if let Err(e) = spawned {
        return undo.run(Err(EjectError::Unexpected {
            detail: format!("couldn't start the eject request: {e}"),
        }));
    }
    match answer.recv_timeout(request_deadline) {
        Ok(RequestAnswer::Ejected) => {
            log::info!(target: "eject", "Windows ejected {volume_id}");
            FlightEnd::answer(Ok(EjectOutcome::DiskEjected))
        }
        Ok(RequestAnswer::Vetoed { detail }) => {
            log::warn!(target: "eject", "Windows refused to eject {volume_id}, since something holds it: {detail}");
            undo.run(Err(EjectError::UnmountRefused {
                // Windows names a device or a module, ❌ never a process Cmdr could word; the
                // scan that would (the Restart Manager) isn't wired up, so nobody is named.
                holders: HolderScan::not_scanned(),
                detail,
            }))
        }
        Ok(RequestAnswer::NotPossible { detail }) => {
            log::warn!(target: "eject", "Windows couldn't eject {volume_id}: {detail}");
            undo.run(Err(EjectError::Unexpected { detail }))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            log::warn!(target: "eject", "The eject request for {volume_id} ended without an answer");
            undo.run(Err(EjectError::Unexpected {
                detail: "the eject request ended without an answer".to_owned(),
            }))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            log::warn!(target: "eject", "Windows hadn't answered the eject of {volume_id} in time; it may still land");
            // The request wasn't cancelled (it can't be): what it ends in decides the take-back.
            let continued = std::thread::Builder::new()
                .name("drive-eject-end".to_owned())
                .spawn(move || match answer.recv() {
                    Ok(RequestAnswer::Ejected) => {
                        log::info!(target: "eject", "Windows ejected {} after the eject answered", undo.volume_id);
                    }
                    Ok(late) => {
                        log::info!(target: "eject", "The eject of {} ended after it answered: {late:?}", undo.volume_id);
                        drop(undo.hand_back());
                    }
                    Err(_) => {
                        drop(undo.hand_back());
                    }
                });
            if let Err(e) = continued {
                log::warn!(target: "eject", "Couldn't wait for the eject request's end ({e}); the drive stays let go until it leaves or Cmdr restarts");
            }
            FlightEnd::answer(Err(EjectError::TimedOut))
        }
    }
}

/// What a flight that didn't eject hands back.
struct Undo {
    gate: &'static DriveRelease,
    volume_id: String,
    side: Arc<dyn DriveSide>,
    was_indexing: bool,
}

impl Undo {
    /// [`Self::hand_back`], ending the flight with `answer`.
    fn run(self, answer: Result<EjectOutcome, EjectError>) -> FlightEnd {
        FlightEnd {
            answer,
            handed_back: self.hand_back(),
        }
    }

    /// The watcher's take-back first (it lifts the unmount-pending flag a resume checks), then
    /// the index this flight stopped, through the gate.
    ///
    /// ❗ The epoch is read NOW, ❌ not at the stop: a removal query Windows sent the watcher
    /// during the request may have released the drive again (its sentinel missed the let-go),
    /// and a stale epoch would leave the index stopped for good.
    fn hand_back(self) -> Option<ResumeBatch> {
        self.side.take_back();
        self.was_indexing.then(|| {
            log::info!(target: "eject", "{} stayed, so it gets its index back", self.volume_id);
            let candidate = ResumeCandidate {
                epoch: self.gate.epoch(&self.volume_id),
                volume_id: self.volume_id.clone(),
            };
            let owner = Arc::new(DriveResume {
                side: Arc::clone(&self.side),
            });
            self.gate.resume(vec![candidate], owner as Arc<dyn ResumeOwner>)
        })
    }
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
        // A flight offers its volume once and never outlives the offer.
        true
    }

    fn is_listed(&self, _candidate: &ResumeCandidate) -> bool {
        self.side.is_listed()
    }

    fn is_ejected_by_another_owner(&self, candidate: &ResumeCandidate) -> bool {
        // The resume settles after this flight landed, so any eject in flight then is another.
        super::super::in_flight::is_ejecting(&candidate.volume_id)
    }

    fn consume(&self, _candidate: &ResumeCandidate) {}
}

#[cfg(test)]
mod tests;
