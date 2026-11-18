//! The queue's work folded into one bar. Pure: the engine's snapshots and
//! progress ticks in, a [`ShellProgress`] out, no clock and no OS. The rules,
//! with their whys: `DETAILS.md` § "The aggregate".

use std::collections::HashMap;

use cmdr_operations::{
    LifecycleStatus, OperationSnapshot, WriteOperationPhase, WriteOperationType, WriteProgressEvent,
};

/// What the OS shell shows for the app's work, before any OS mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellProgress {
    /// Nothing in flight and no failure waiting: no bar.
    Hidden,
    /// A bar. `percent` is `None` while no operation of this session has
    /// measured its size yet (every one is queued or still counting).
    Shown { percent: Option<u8>, tone: Tone },
}

/// The bar's color, where the OS has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Work is moving.
    Normal,
    /// Nothing is moving because of a person: every started operation is
    /// paused or waiting on a conflict answer.
    Paused,
    /// Nothing is live and the queue holds a failure the person hasn't
    /// dismissed.
    Error,
}

/// One operation of the current session, as far as the bar cares.
#[derive(Debug, Default)]
struct Entry {
    /// Done share of this operation's work, 0–1, once it has measured any.
    fraction: Option<f64>,
    /// The biggest byte total it reported: its weight when the session is
    /// weighed in bytes. `None` for work measured only in files.
    bytes_total: Option<u64>,
    /// Its last tick said it's parked on a person (paused or a conflict).
    parked: bool,
    /// It completed. Stays in the session at full so the bar doesn't fall back
    /// when a finished operation leaves the queue.
    completed: bool,
}

/// The queue as the bar sees it. Fed by [`Aggregate::note_operations`] (the
/// row set and each row's status) and [`Aggregate::note_progress`] (how far
/// each one is), read by [`Aggregate::summary`].
#[derive(Debug, Default)]
pub(crate) struct Aggregate {
    /// The last snapshot's non-instant rows, by operation id.
    statuses: HashMap<String, LifecycleStatus>,
    /// Every operation live during the current session, plus the completed
    /// ones. Empties when nothing is live any more.
    session: HashMap<String, Entry>,
}

/// Rename and create run inline and are over before the eye lands on them; the
/// status corner leaves them out for the same reason (`operation-chip.ts`).
fn is_instant(operation_type: WriteOperationType) -> bool {
    matches!(
        operation_type,
        WriteOperationType::Rename | WriteOperationType::CreateFolder | WriteOperationType::CreateFile
    )
}

fn is_live(status: LifecycleStatus) -> bool {
    matches!(
        status,
        LifecycleStatus::Queued | LifecycleStatus::Running | LifecycleStatus::Paused
    )
}

impl Aggregate {
    /// Takes a new `operations-changed` snapshot: the full row set.
    ///
    /// A live row joins the session. A row that left (or turned `Cancelled` or
    /// `Failed`) leaves it, unless it completed: a cancelled or failed
    /// operation's work is no longer coming, so it stops weighing on the bar.
    /// With nothing live left, the session ends.
    pub(crate) fn note_operations(&mut self, operations: &[OperationSnapshot]) {
        self.statuses = operations
            .iter()
            .filter(|row| !is_instant(row.operation_type))
            .map(|row| (row.operation_id.clone(), row.status))
            .collect();

        for (id, status) in &self.statuses {
            if is_live(*status) {
                self.session.entry(id.clone()).or_default();
            } else if *status == LifecycleStatus::Done
                && let Some(entry) = self.session.get_mut(id)
            {
                entry.completed = true;
            }
        }
        let statuses = &self.statuses;
        self.session
            .retain(|id, entry| entry.completed || statuses.get(id).is_some_and(|status| is_live(*status)));
        if !self.statuses.values().any(|status| is_live(*status)) {
            self.session.clear();
        }
    }

    /// Takes a progress tick. Ignored for an operation the snapshot hasn't
    /// listed yet (the next tick lands 200 ms later), so a late tick can't
    /// bring a gone operation back.
    pub(crate) fn note_progress(&mut self, event: &WriteProgressEvent) {
        let Some(entry) = self.session.get_mut(&event.operation_id) else {
            return;
        };
        if entry.completed {
            return;
        }
        entry.parked = event
            .activity
            .as_ref()
            .is_some_and(|activity| activity.is_held_by_a_person());
        if event.bytes_total > 0 {
            entry.bytes_total = Some(
                entry
                    .bytes_total
                    .map_or(event.bytes_total, |b| b.max(event.bytes_total)),
            );
        }
        if let Some(fraction) = measured_fraction(event) {
            entry.fraction = Some(fraction);
        }
    }

    /// Takes a `write-complete`: the operation's work is all done.
    pub(crate) fn note_completed(&mut self, operation_id: &str) {
        if let Some(entry) = self.session.get_mut(operation_id) {
            entry.completed = true;
            entry.parked = false;
        }
    }

    /// The bar for the queue as it stands.
    pub(crate) fn summary(&self) -> ShellProgress {
        let failed = self.statuses.values().any(|status| *status == LifecycleStatus::Failed);
        let live = self.statuses.values().any(|status| is_live(*status));
        if !live {
            // A full red bar: the work stopped and something needs a look. It
            // stays while the queue keeps the failure, like the status corner's
            // failure mark.
            return if failed {
                ShellProgress::Shown {
                    percent: Some(100),
                    tone: Tone::Error,
                }
            } else {
                ShellProgress::Hidden
            };
        }
        // Live work wins over a failure waiting in the queue, as in the status
        // corner (`operation-chip.ts`, progress beats failure): the bar says
        // what's happening now, and the failure turns it red once nothing is.
        let tone = if self.all_started_parked() {
            Tone::Paused
        } else {
            Tone::Normal
        };
        ShellProgress::Shown {
            percent: self.percent(),
            tone,
        }
    }

    /// Every operation that has started is held by a person, and at least one
    /// has started. A queued one waits on a lane, which a parked one holds, so
    /// it doesn't count as moving.
    fn all_started_parked(&self) -> bool {
        let mut started = self
            .statuses
            .iter()
            .filter(|(_, status)| matches!(status, LifecycleStatus::Running | LifecycleStatus::Paused))
            .peekable();
        started.peek().is_some()
            && started.all(|(id, status)| {
                *status == LifecycleStatus::Paused || self.session.get(id).is_some_and(|entry| entry.parked)
            })
    }

    /// The session's done share as a whole percent, or `None` while nothing in
    /// it has measured its size. Floored, and capped at 99 while work is live:
    /// the bar never says "done" before the queue does.
    fn percent(&self) -> Option<u8> {
        let fraction = session_fraction(self.session.values())?;
        let percent = (fraction * 100.0).floor().clamp(0.0, 99.0);
        Some(percent as u8)
    }
}

/// How far one tick says its operation is, 0–1, or `None` when the tick
/// measures nothing.
///
/// - Scanning counts the work, so its totals are what it's FOR: no measure.
/// - Rolling back undoes work: the bar holds still rather than read the undo
///   as progress.
/// - The finishing phases have no denominator left; all their measured work is
///   done, so the step reads full.
/// - Otherwise bytes, or files when no bytes move (a same-volume move), the
///   status corner's rule (`operation-chip.ts`).
///
/// A multi-step operation (compress, then upload) folds its steps into one
/// run: step 2 of 2 at half is three quarters.
fn measured_fraction(event: &WriteProgressEvent) -> Option<f64> {
    let within_step = match event.phase {
        WriteOperationPhase::Scanning | WriteOperationPhase::RollingBack => return None,
        WriteOperationPhase::FinishingCompression | WriteOperationPhase::FinishingTransfer => 1.0,
        _ if event.bytes_total > 0 => ratio(event.bytes_done, event.bytes_total),
        _ if event.files_total > 0 => ratio(event.files_done as u64, event.files_total as u64),
        _ => return None,
    };
    let fraction = match event.step {
        Some(step) if step.total > 1 && (1..=step.total).contains(&step.number) => {
            (f64::from(step.number - 1) + within_step) / f64::from(step.total)
        }
        _ => within_step,
    };
    Some(fraction.clamp(0.0, 1.0))
}

fn ratio(done: u64, total: u64) -> f64 {
    (done as f64 / total as f64).clamp(0.0, 1.0)
}

/// The weighted done share of a session, or `None` while nothing in it has
/// measured anything.
///
/// Weighed in bytes when every measured operation moved bytes, so a 1 MB copy
/// beside a 100 GB one doesn't count for half. Otherwise (a delete measures
/// files, not bytes) every operation weighs the same. An operation still
/// queued or counting weighs as much as the average measured one and counts as
/// nothing done: its work is real, its size not yet known.
fn session_fraction<'a>(entries: impl Iterator<Item = &'a Entry> + Clone) -> Option<f64> {
    let measured = |entry: &&Entry| entry.completed || entry.fraction.is_some();
    let mut measured_count = 0_u32;
    let mut weighed_in_bytes = true;
    let mut byte_sum = 0_f64;
    for entry in entries.clone().filter(measured) {
        measured_count += 1;
        match entry.bytes_total {
            Some(bytes) => byte_sum += bytes as f64,
            None => weighed_in_bytes = false,
        }
    }
    if measured_count == 0 {
        return None;
    }
    let unknown_weight = if weighed_in_bytes {
        byte_sum / f64::from(measured_count)
    } else {
        1.0
    };
    let (mut done, mut total) = (0_f64, 0_f64);
    for entry in entries {
        let weight = match (weighed_in_bytes, entry.bytes_total, measured(&entry)) {
            (true, Some(bytes), true) => bytes as f64,
            (true, _, _) => unknown_weight,
            (false, _, _) => 1.0,
        };
        let fraction = if entry.completed {
            1.0
        } else {
            entry.fraction.unwrap_or(0.0)
        };
        done += weight * fraction;
        total += weight;
    }
    if total <= 0.0 {
        return None;
    }
    Some((done / total).clamp(0.0, 1.0))
}

#[cfg(test)]
#[path = "aggregate_tests.rs"]
mod tests;
