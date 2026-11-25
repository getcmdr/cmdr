//! Letting go of a drive: [`DriveRelease::release`], its stop threads, and the continuations that
//! answer after the caller stopped waiting.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

use cmdr_index::{IndexVolumeKind, RemovableStop};

use super::{DriveRelease, Shared, Ticket};
use crate::ignore_poison::IgnorePoison;

/// A stop a release runs per volume: `Index::stop_removable_volume` with the caller's own wait, or
/// a test's.
pub(super) type StopFn = Arc<dyn Fn(&str) -> RemovableStop + Send + Sync>;

/// Where a stop that answered after its release stopped waiting reports.
pub(super) type LateRecorder = Arc<dyn Fn(LateRelease) + Send + Sync>;

/// How one volume's release went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeRelease {
    /// No index worked on the volume and no start was in flight.
    NothingToStop,
    /// Its index stopped and let go of the drive. `was_indexing` is whether a local external index
    /// was there before the stop, which is what an owner resumes.
    Released { was_indexing: bool },
    /// A start still held the volume's ticket, or the stop was still letting go, when the deadline
    /// passed. ❗ Not safe to unmount. The release's continuation reports the eventual answer.
    StillReleasing,
}

/// One volume's answer from a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleasedVolume {
    pub(crate) volume_id: String,
    pub(crate) outcome: VolumeRelease,
    /// The epoch this release moved the volume to; an owner records it for its resume.
    pub(crate) epoch: u64,
}

/// What a release answered, one entry per volume asked about, in the order asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Release {
    pub(crate) volumes: Vec<ReleasedVolume>,
}

impl Release {
    /// The answer for `volume_id`, `None` when the release wasn't asked about it.
    #[cfg_attr(
        all(not(test), target_os = "linux"),
        expect(
            dead_code,
            reason = "the macOS unmount approver and the macOS and Windows unmount handler read a release's own answer"
        )
    )]
    pub(crate) fn outcome(&self, volume_id: &str) -> Option<VolumeRelease> {
        self.volumes
            .iter()
            .find(|volume| volume.volume_id == volume_id)
            .map(|volume| volume.outcome)
    }
}

/// A stop that answered after its release had stopped waiting: a stop still running at the
/// deadline, or the continuation that stopped a start whose ticket was still in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LateRelease {
    pub(crate) volume_id: String,
    pub(crate) outcome: VolumeRelease,
    /// The epoch the release that gave up on it moved the volume to.
    pub(crate) epoch: u64,
}

/// A release's continuation for a volume whose ticket was still in flight at its deadline.
pub(super) struct LateStop {
    stop: StopFn,
    record: LateRecorder,
    epoch: u64,
}

/// A stop thread's slot, under the gates lock.
pub(super) enum StopSlot {
    /// Still stopping, and its release is still waiting.
    Running,
    /// Answered; its release collects it.
    Answered(VolumeRelease),
    /// Its release gave up waiting, so the thread reports to the recorder itself.
    Abandoned { record: LateRecorder, epoch: u64 },
}

/// Where one volume of a release stands.
enum Progress {
    /// A start holds the ticket; the stop waits for it to drop.
    WaitingForTicket,
    /// Its stop thread is running under this serial.
    Stopping(u64),
    Done(VolumeRelease),
}

impl DriveRelease {
    /// Let go of every volume in `volume_ids`, all bounded by one `deadline` instant.
    ///
    /// Per volume: move its epoch first (an unstarted resume aborts), wait for a start in flight to
    /// return, then run `stop` on a thread of its own, concurrently with the other volumes. A
    /// volume whose ticket is still in flight, or whose stop is still running, at the deadline
    /// answers [`VolumeRelease::StillReleasing`], and `record_late` gets its eventual answer: a
    /// stuck stop keeps running detached, and a start's continuation stops it once its ticket
    /// drops.
    ///
    /// Blocks until every volume answered or `deadline` passed: the one wait a DA queue may make.
    pub(crate) fn release(
        &self,
        volume_ids: &[String],
        deadline: Instant,
        stop: impl Fn(&str) -> RemovableStop + Send + Sync + 'static,
        record_late: impl Fn(LateRelease) + Send + Sync + 'static,
    ) -> Release {
        let stop: StopFn = Arc::new(stop);
        let record: LateRecorder = Arc::new(record_late);
        let shared = &self.shared;

        let mut asked: Vec<String> = Vec::with_capacity(volume_ids.len());
        for volume_id in volume_ids {
            if !asked.contains(volume_id) {
                asked.push(volume_id.clone());
            }
        }

        let mut gates = shared.gates.lock_ignore_poison();
        // ❗ Every epoch moves BEFORE anything waits: the other order lets a resume that hasn't
        // taken its ticket yet slip through while this waits.
        let epochs: Vec<u64> = asked.iter().map(|volume_id| gates.bump_epoch(volume_id)).collect();
        let mut progress: Vec<Progress> = asked.iter().map(|_| Progress::WaitingForTicket).collect();

        loop {
            for (index, volume_id) in asked.iter().enumerate() {
                match progress[index] {
                    Progress::WaitingForTicket if gates.gate(volume_id).ticket.is_none() => {
                        gates.last_stop += 1;
                        let serial = gates.last_stop;
                        progress[index] = match spawn_stop(shared, volume_id, serial, &stop) {
                            Ok(()) => {
                                gates.stops.insert(serial, StopSlot::Running);
                                Progress::Stopping(serial)
                            }
                            Err(e) => {
                                crate::log_error!(target: "drive_release", "Couldn't start the stop for {volume_id}: {e}");
                                Progress::Done(VolumeRelease::StillReleasing)
                            }
                        };
                    }
                    Progress::Stopping(serial) => {
                        if let Some(StopSlot::Answered(outcome)) = gates.stops.get(&serial) {
                            progress[index] = Progress::Done(*outcome);
                            gates.stops.remove(&serial);
                        }
                    }
                    Progress::WaitingForTicket | Progress::Done(_) => {}
                }
            }

            let all_done = progress.iter().all(|volume| matches!(volume, Progress::Done(_)));
            if all_done {
                break;
            }
            if shared.clock.now() >= deadline {
                for ((volume_id, volume), epoch) in asked.iter().zip(progress.iter_mut()).zip(&epochs) {
                    match *volume {
                        Progress::WaitingForTicket => gates.gate(volume_id).late_stops.push(LateStop {
                            stop: Arc::clone(&stop),
                            record: Arc::clone(&record),
                            epoch: *epoch,
                        }),
                        Progress::Stopping(serial) => {
                            gates.stops.insert(
                                serial,
                                StopSlot::Abandoned {
                                    record: Arc::clone(&record),
                                    epoch: *epoch,
                                },
                            );
                        }
                        Progress::Done(_) => continue,
                    }
                    *volume = Progress::Done(VolumeRelease::StillReleasing);
                }
                break;
            }
            gates = shared.park(gates, deadline);
        }
        drop(gates);

        let volumes: Vec<ReleasedVolume> = asked
            .into_iter()
            .zip(progress)
            .zip(epochs)
            .map(|((volume_id, volume), epoch)| ReleasedVolume {
                volume_id,
                outcome: match volume {
                    Progress::Done(outcome) => outcome,
                    Progress::WaitingForTicket | Progress::Stopping(_) => VolumeRelease::StillReleasing,
                },
                epoch,
            })
            .collect();
        for volume in &volumes {
            if volume.outcome == VolumeRelease::StillReleasing {
                log::warn!(
                    target: "drive_release",
                    "{} was still being let go of at the release's deadline; its stop answers late",
                    volume.volume_id
                );
            }
        }
        Release { volumes }
    }
}

/// Run `stop` for `volume_id` on a thread of its own. It answers into `serial`'s slot, or, when its
/// release gave up waiting, straight to that release's recorder.
fn spawn_stop(shared: &Arc<Shared>, volume_id: &str, serial: u64, stop: &StopFn) -> std::io::Result<()> {
    let shared = Arc::clone(shared);
    let volume_id = volume_id.to_string();
    let stop = Arc::clone(stop);
    std::thread::Builder::new()
        .name("drive-release-stop".to_string())
        .spawn(move || {
            let outcome = run_stop(&shared, &volume_id, &stop);
            let abandoned = {
                let mut gates = shared.gates.lock_ignore_poison();
                match gates.stops.remove(&serial) {
                    Some(StopSlot::Abandoned { record, epoch }) => Some((record, epoch)),
                    _ => {
                        gates.stops.insert(serial, StopSlot::Answered(outcome));
                        None
                    }
                }
            };
            shared.changed.notify_all();
            if let Some((record, epoch)) = abandoned {
                log::info!(target: "drive_release", "The stop for {volume_id} answered late: {outcome:?}");
                record(LateRelease {
                    volume_id,
                    outcome,
                    epoch,
                });
            }
        })
        .map(|_| ())
}

/// One stop, with what was there before it. A stop that panicked says nothing about whether the
/// index let go, so it reads as still releasing.
fn run_stop(shared: &Shared, volume_id: &str, stop: &StopFn) -> VolumeRelease {
    let was_indexing = shared.door.volume_kind(volume_id) == Some(IndexVolumeKind::LocalExternal);
    match catch_unwind(AssertUnwindSafe(|| stop(volume_id))) {
        Ok(RemovableStop::NothingToStop) => VolumeRelease::NothingToStop,
        Ok(RemovableStop::Released) => VolumeRelease::Released { was_indexing },
        Ok(RemovableStop::StillReleasing) => VolumeRelease::StillReleasing,
        Err(_) => {
            crate::log_error!(target: "drive_release", "The index stop for {volume_id} panicked; reading it as still releasing");
            VolumeRelease::StillReleasing
        }
    }
}

/// Stop a volume on behalf of every release that gave up waiting for its ticket, holding the
/// ticket while it does, so no start slips in between the start that returned and this stop.
pub(super) fn run_late_stops(ticket: Ticket, late_stops: Vec<LateStop>) {
    let volume_id = ticket.volume_id.clone();
    let spawned = std::thread::Builder::new()
        .name("drive-release-late-stop".to_string())
        .spawn(move || {
            // Every release that gave up is owed the same answer: one stop serves them all.
            let Some(first) = late_stops.first() else {
                return;
            };
            let outcome = run_stop(&ticket.shared, &ticket.volume_id, &first.stop);
            log::info!(
                target: "drive_release",
                "Stopped {} once the start its release waited on returned: {outcome:?}",
                ticket.volume_id
            );
            for late in &late_stops {
                (late.record)(LateRelease {
                    volume_id: ticket.volume_id.clone(),
                    outcome,
                    epoch: late.epoch,
                });
            }
            drop(ticket);
        });
    if let Err(e) = spawned {
        crate::log_error!(
            target: "drive_release",
            "Couldn't start the late stop for {volume_id}: {e}; whatever its start stood up keeps running"
        );
    }
}
