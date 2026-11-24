//! Handing back what a stop let go of: [`DriveRelease::resume`], and the unmount-pending flag an
//! unmount's owner sets and clears.
#![cfg_attr(
    all(not(test), not(any(target_os = "macos", target_os = "windows"))),
    expect(
        dead_code,
        reason = "only the macOS unmount approver and the Windows drive watcher resume through the gate"
    )
)]

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{DriveRelease, Ticket, TicketFor};
use crate::ignore_poison::IgnorePoison;

/// The quiet period before a resume starts anything. Any release of a volume in the meantime moves
/// its epoch, which voids its resume.
pub(crate) const RESUME_SETTLE: Duration = Duration::from_secs(2);

/// A volume an owner stopped and wants back, with the epoch its release set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeCandidate {
    pub(crate) volume_id: String,
    pub(crate) epoch: u64,
}

/// Whoever stopped a volume and may start it again: the unmount approver, or an eject flight. A
/// vanished drive has no owner, so it's never resumed.
///
/// Every method answers from memory or the non-blocking mount table: they run at the gate.
pub(crate) trait ResumeOwner: Send + Sync + 'static {
    /// For the log.
    fn name(&self) -> &'static str;
    /// Whether the owner still stands behind the candidate (the approver: no newer ask on its whole
    /// disk since the record).
    fn still_owns(&self, candidate: &ResumeCandidate) -> bool;
    /// Whether the volume is still mounted, by the owner's own evidence.
    fn is_listed(&self, candidate: &ResumeCandidate) -> bool;
    /// Whether an eject the owner doesn't run is taking the volume down.
    fn is_ejected_by_another_owner(&self, candidate: &ResumeCandidate) -> bool;
    /// The candidate's record is spent: its resume took the ticket or failed a check. A later idle
    /// must never offer it again, or a first scan the stop cut short gets `force_scan`.
    fn consume(&self, candidate: &ResumeCandidate);
}

/// What a resume did with one candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumeVerdict {
    /// A start or an earlier resume of the volume was already under way; the candidate joined it.
    Joined,
    /// The resume took the ticket and handed the start to the index.
    Started,
    /// A check failed, so nothing started.
    NotResumed(ResumeRefusal),
}

/// The check a resume failed, in the order they're asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumeRefusal {
    /// A disable or a late stop held the volume.
    TicketInFlight,
    UnmountPending,
    /// A release or a disable came after the record.
    EpochMoved,
    OwnerMovedOn,
    NotListed,
    /// The volume's persisted intent doesn't say it should index (or it's already indexing).
    NoIntent,
    EjectedByAnotherOwner,
}

/// A resume in progress. Dropping it detaches the batch; a test waits for it.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the app lets a batch settle on its own; only a test waits on one"
    )
)]
pub(crate) struct ResumeBatch {
    verdicts: Vec<(String, ResumeVerdict)>,
    settling: Option<std::thread::JoinHandle<Vec<(String, ResumeVerdict)>>>,
}

impl ResumeBatch {
    /// Wait for the batch's checks and hand back every candidate's verdict, joined ones first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the app lets a batch settle on its own; only a test waits on one"
        )
    )]
    pub(crate) fn wait(self) -> Vec<(String, ResumeVerdict)> {
        let mut verdicts = self.verdicts;
        if let Some(settling) = self.settling {
            match settling.join() {
                Ok(more) => verdicts.extend(more),
                Err(_) => {
                    crate::log_error!(target: "drive_release", "A resume batch panicked; its volumes stay stopped")
                }
            }
        }
        verdicts
    }
}

impl DriveRelease {
    /// Start again what `owner` stopped, after [`RESUME_SETTLE`], for every candidate every check
    /// still vouches for. Returns at once: the batch settles on a thread of its own, so it's safe
    /// from any queue.
    ///
    /// 1. A candidate whose volume already has a start in flight or a pending resume joins it. A
    ///    release or disable since that resume was queued cancelled it, so the candidate queues.
    /// 2. The rest wait out [`RESUME_SETTLE`] together.
    /// 3. The batch reads intent once (`Index::drives_to_resume`, which opens databases).
    /// 4. Per candidate, at the gate: no ticket in flight, no unmount pending, an unchanged epoch,
    ///    the owner still standing behind it, still listed, intended, and not ejected by someone
    ///    else. Its record is consumed either way.
    /// 5. The start runs on the async runtime, holding the ticket until it returns.
    pub(crate) fn resume(&self, candidates: Vec<ResumeCandidate>, owner: Arc<dyn ResumeOwner>) -> ResumeBatch {
        let mut verdicts = Vec::new();
        let mut batch = Vec::new();
        let serial = {
            let mut gates = self.shared.gates.lock_ignore_poison();
            gates.last_resume_batch += 1;
            let serial = gates.last_resume_batch;
            for candidate in candidates {
                let gate = gates.gate(&candidate.volume_id);
                if gate.resume_batch.is_some_and(|queued| queued != serial) || gate.ticket == Some(TicketFor::Start) {
                    verdicts.push((candidate.volume_id, ResumeVerdict::Joined));
                } else if gate.resume_batch.is_none() {
                    gate.resume_batch = Some(serial);
                    batch.push(candidate);
                }
            }
            serial
        };
        if batch.is_empty() {
            return ResumeBatch {
                verdicts,
                settling: None,
            };
        }

        let settle_ends = self.shared.clock.now() + RESUME_SETTLE;
        let gate = self.clone();
        let batch_ids: Vec<String> = batch.iter().map(|candidate| candidate.volume_id.clone()).collect();
        let spawned = std::thread::Builder::new()
            .name("drive-resume".to_string())
            .spawn(move || gate.settle_and_resume(batch, serial, owner.as_ref(), settle_ends));
        match spawned {
            Ok(settling) => ResumeBatch {
                verdicts,
                settling: Some(settling),
            },
            Err(e) => {
                crate::log_error!(target: "drive_release", "Couldn't start a resume batch for {batch_ids:?}: {e}");
                let mut gates = self.shared.gates.lock_ignore_poison();
                for volume_id in &batch_ids {
                    let gate = gates.gate(volume_id);
                    if gate.resume_batch == Some(serial) {
                        gate.resume_batch = None;
                    }
                }
                ResumeBatch {
                    verdicts,
                    settling: None,
                }
            }
        }
    }

    fn settle_and_resume(
        &self,
        batch: Vec<ResumeCandidate>,
        serial: u64,
        owner: &dyn ResumeOwner,
        settle_ends: Instant,
    ) -> Vec<(String, ResumeVerdict)> {
        {
            let mut gates = self.shared.gates.lock_ignore_poison();
            while self.shared.clock.now() < settle_ends {
                gates = self.shared.park(gates, settle_ends);
            }
        }
        let intent: HashSet<String> = self.shared.door.drives_to_resume().into_iter().collect();

        batch
            .into_iter()
            .map(|candidate| {
                let checked = self.check_resume(&candidate, serial, owner, &intent);
                owner.consume(&candidate);
                let verdict = match checked {
                    Ok(ticket) => {
                        log::info!(
                            target: "drive_release",
                            "Resuming {} for {}",
                            candidate.volume_id,
                            // allowed-log-private: the resume owner's fixed name of our own
                            owner.name()
                        );
                        self.shared.door.start_resumed(candidate.volume_id.clone(), ticket);
                        ResumeVerdict::Started
                    }
                    Err(refusal) => {
                        log::info!(
                            target: "drive_release",
                            "Not resuming {} for {}: {refusal:?}",
                            candidate.volume_id,
                            // allowed-log-private: the resume owner's fixed name of our own
                            owner.name()
                        );
                        ResumeVerdict::NotResumed(refusal)
                    }
                };
                (candidate.volume_id, verdict)
            })
            .collect()
    }

    /// Step 4 for one candidate: the gate's own checks and the ticket atomically, then the owner's
    /// and intent's while holding the ticket, which drops again on any refusal.
    fn check_resume(
        &self,
        candidate: &ResumeCandidate,
        serial: u64,
        owner: &dyn ResumeOwner,
        intent: &HashSet<String>,
    ) -> Result<Ticket, ResumeRefusal> {
        let volume_id = candidate.volume_id.as_str();
        {
            let mut gates = self.shared.gates.lock_ignore_poison();
            let gate = gates.gate(volume_id);
            if gate.resume_batch != Some(serial) {
                // An epoch move cancelled this batch's claim, and a newer batch may hold the
                // volume now: leave its claim alone.
                return Err(ResumeRefusal::EpochMoved);
            }
            gate.resume_batch = None;
            if gate.ticket.is_some() {
                return Err(ResumeRefusal::TicketInFlight);
            }
            if gate.unmount_pending {
                return Err(ResumeRefusal::UnmountPending);
            }
            if gate.epoch != candidate.epoch {
                return Err(ResumeRefusal::EpochMoved);
            }
            gate.ticket = Some(TicketFor::Start);
        }
        let ticket = Ticket::taken(&self.shared, volume_id);
        if !owner.still_owns(candidate) {
            return Err(ResumeRefusal::OwnerMovedOn);
        }
        if !owner.is_listed(candidate) {
            return Err(ResumeRefusal::NotListed);
        }
        if !intent.contains(volume_id) {
            return Err(ResumeRefusal::NoIntent);
        }
        if owner.is_ejected_by_another_owner(candidate) {
            return Err(ResumeRefusal::EjectedByAnotherOwner);
        }
        Ok(ticket)
    }

    /// Mark every volume in `volume_ids` as having an unmount on its way: a person's start waits it
    /// out, and every other start and every resume skips the volume.
    pub(crate) fn set_unmount_pending(&self, volume_ids: &[String]) {
        let mut gates = self.shared.gates.lock_ignore_poison();
        for volume_id in volume_ids {
            gates.gate(volume_id).unmount_pending = true;
        }
    }

    /// Clear every unmount-pending flag, waking the starts waiting them out: DiskArbitration's queue
    /// went quiet, so no unmount is on its way any more. An ask is the only thing that sets one.
    pub(crate) fn clear_all_unmount_pending(&self) {
        {
            let mut gates = self.shared.gates.lock_ignore_poison();
            for gate in gates.volumes.values_mut() {
                gate.unmount_pending = false;
            }
        }
        self.shared.changed.notify_all();
    }

    /// Clear the unmount-pending flag of every volume in `volume_ids`, waking the starts waiting it
    /// out.
    pub(crate) fn clear_unmount_pending(&self, volume_ids: &[String]) {
        {
            let mut gates = self.shared.gates.lock_ignore_poison();
            for volume_id in volume_ids {
                gates.gate(volume_id).unmount_pending = false;
            }
        }
        self.shared.changed.notify_all();
    }

    /// The volume's current epoch. An eject flight reads it after its own teardown settles, since
    /// the asks its unmount triggers move it.
    pub(crate) fn epoch(&self, volume_id: &str) -> u64 {
        self.shared.gates.lock_ignore_poison().gate(volume_id).epoch
    }
}
