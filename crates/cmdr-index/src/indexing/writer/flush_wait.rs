//! What the writer thread is working on right now, and how far a waiting flush
//! still is from the front of its queue, so the waiter can say what it's behind.
//!
//! The writer's own slow-message WARN (`SLOW_MESSAGE` in `mod.rs`) fires only once
//! a message ENDS, so a message that never ends never names itself. A flush that
//! has waited [`STALL_REPORT_INTERVAL`] reads this and says it instead, once per
//! interval, for as long as it keeps waiting. It never gives up: a single message
//! can legitimately run for minutes (a full aggregate over millions of rows), and
//! a waiter that walked away would leave the work it was waiting for unfinished.
//!
//! The level tells the two waits apart. A wedge (GitHub issue #564: one message
//! held the writer a whole interval, or the writer took nothing off its queue in
//! one) is a WARN naming the message. A flush queued behind thousands of short
//! writes that keep draining (a walk feeding the writer) is expected, so it's INFO
//! with the count still ahead of it.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::pluralize::pluralize_grouped;

use crate::indexing::store::IndexStoreError;

/// How long a flush waits between "still waiting" lines.
pub(super) const STALL_REPORT_INTERVAL: Duration = Duration::from_secs(30);

/// Where a `Flush` reply goes: an async waiter's oneshot, or a blocking waiter's
/// channel. The blocking one is a std channel because it has to wait with a
/// timeout, so it can say what it's waiting on.
pub enum FlushReply {
    /// `IndexWriter::flush`'s.
    Async(oneshot::Sender<()>),
    /// `IndexWriter::flush_blocking`'s.
    Blocking(mpsc::SyncSender<()>),
}

impl FlushReply {
    pub(super) fn send(self) {
        // The waiter may have gone (a dropped task, a test's timeout); nothing to tell.
        match self {
            FlushReply::Async(tx) => drop(tx.send(())),
            FlushReply::Blocking(tx) => drop(tx.send(())),
        }
    }
}

impl From<oneshot::Sender<()>> for FlushReply {
    fn from(tx: oneshot::Sender<()>) -> Self {
        FlushReply::Async(tx)
    }
}

/// Where a flush stood in the queue when it was sent: how many messages were
/// ahead of it, and how many the writer had taken by then. Best effort, since the
/// depth is a gauge shared by every sender, which is why the line says "about".
#[derive(Clone, Copy)]
pub(super) struct FlushTicket {
    ahead: u64,
    taken: u64,
}

impl FlushTicket {
    /// Read right after the `Flush` went in: the depth counts it, so one less is ahead.
    pub(super) fn now(queue_depth: &AtomicUsize, busy_on: &BusyOn) -> Self {
        let taken = busy_on.taken();
        let depth = queue_depth.load(Ordering::Relaxed) as u64;
        FlushTicket {
            ahead: depth.saturating_sub(1),
            taken,
        }
    }

    /// How many of the messages ahead of the flush the writer hasn't taken yet.
    fn still_ahead(&self, taken_now: u64) -> u64 {
        self.ahead.saturating_sub(taken_now.saturating_sub(self.taken))
    }
}

/// What a waiting flush reports each interval. Kept apart from the clock and the
/// logger so the wording and the level are testable.
struct WaitTracker<'a> {
    busy_on: &'a BusyOn,
    volume_id: &'a str,
    ticket: FlushTicket,
    interval: Duration,
    started: Instant,
    taken_at_last_report: u64,
}

impl<'a> WaitTracker<'a> {
    fn new(busy_on: &'a BusyOn, volume_id: &'a str, ticket: FlushTicket, interval: Duration) -> Self {
        WaitTracker {
            busy_on,
            volume_id,
            ticket,
            interval,
            started: Instant::now(),
            taken_at_last_report: ticket.taken,
        }
    }

    fn report(&mut self) {
        let taken = self.busy_on.taken();
        let state = WaitState {
            still_ahead: self.ticket.still_ahead(taken),
            busy_on: self.busy_on.snapshot(),
            progressed: taken > self.taken_at_last_report,
        };
        self.taken_at_last_report = taken;
        let (warn, line) = wait_report(self.volume_id, self.started.elapsed(), self.interval, state);
        if warn {
            log::warn!("{line}");
        } else {
            log::info!("{line}");
        }
    }
}

/// Await an async flush's reply, reporting each [`STALL_REPORT_INTERVAL`] it takes.
pub(super) async fn wait_async(
    mut reply: oneshot::Receiver<()>,
    busy_on: &BusyOn,
    volume_id: &str,
    ticket: FlushTicket,
) -> Result<(), IndexStoreError> {
    let mut tracker = WaitTracker::new(busy_on, volume_id, ticket, STALL_REPORT_INTERVAL);
    loop {
        match tokio::time::timeout(STALL_REPORT_INTERVAL, &mut reply).await {
            Ok(answer) => return answer.map_err(|_| dropped_reply()),
            Err(_) => tracker.report(),
        }
    }
}

/// Block on a flush's reply, reporting each `interval` it takes.
pub(super) fn wait_blocking(
    reply: &mpsc::Receiver<()>,
    interval: Duration,
    busy_on: &BusyOn,
    volume_id: &str,
    ticket: FlushTicket,
) -> Result<(), IndexStoreError> {
    let mut tracker = WaitTracker::new(busy_on, volume_id, ticket, interval);
    loop {
        match reply.recv_timeout(interval) {
            Ok(()) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => tracker.report(),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(dropped_reply()),
        }
    }
}

fn dropped_reply() -> IndexStoreError {
    IndexStoreError::Io(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "Writer thread dropped flush reply",
    ))
}

/// The message the writer thread is inside, if any, and how many messages it has
/// taken off its queue. Written by the writer around each message (an uncontended
/// lock and an atomic), read only by a waiter that has already waited a whole
/// [`STALL_REPORT_INTERVAL`].
#[derive(Default)]
pub(crate) struct BusyOn {
    current: Mutex<Option<(&'static str, Instant)>>,
    taken: AtomicU64,
}

impl BusyOn {
    /// The writer took a message off its queue and is starting on it.
    pub(super) fn begin_message(&self, kind: &'static str) {
        self.taken.fetch_add(1, Ordering::Relaxed);
        self.start(kind);
    }

    /// The writer is starting on work that isn't a queued message (the ledger
    /// settle, an idle vacuum slice): named, but not counted.
    pub(super) fn start(&self, kind: &'static str) {
        *self.current.lock_ignore_poison() = Some((kind, Instant::now()));
    }

    pub(super) fn end(&self) {
        *self.current.lock_ignore_poison() = None;
    }

    /// How many messages the writer has taken off its queue so far.
    pub(super) fn taken(&self) -> u64 {
        self.taken.load(Ordering::Relaxed)
    }

    /// The message the writer is on and how long it has been on it, or `None`
    /// while it's between messages.
    pub(crate) fn snapshot(&self) -> Option<(&'static str, Duration)> {
        self.current
            .lock_ignore_poison()
            .map(|(kind, since)| (kind, since.elapsed()))
    }
}

/// What a waiting flush knows at one report.
struct WaitState {
    /// Messages still between the flush and the writer.
    still_ahead: u64,
    /// What the writer is on, and for how long.
    busy_on: Option<(&'static str, Duration)>,
    /// Whether the writer took any message since the previous report.
    progressed: bool,
}

/// The "still waiting" line and whether it's a warning: `(warn, line)`. A wedge
/// warns (one message held the writer a whole interval, or the queue didn't move);
/// a queue that keeps draining is information.
fn wait_report(volume_id: &str, waited: Duration, interval: Duration, state: WaitState) -> (bool, String) {
    let stuck_on_one = state.busy_on.is_some_and(|(_, on_it)| on_it >= interval);
    let warn = stuck_on_one || !state.progressed;
    let head = format!(
        "Waited {} s for '{volume_id}''s index writer to flush",
        waited.as_secs()
    );
    let ahead = if state.still_ahead == 0 {
        "nothing else is ahead of it".to_string()
    } else {
        let count = if state.still_ahead == 1 {
            "about one message is".to_string()
        } else {
            format!("about {} are", pluralize_grouped(state.still_ahead, "message"))
        };
        format!("{count} still ahead of it")
    };
    let writer = match state.busy_on {
        Some((kind, on_it)) if stuck_on_one => {
            format!("but the writer has been on one {kind} for {} s", on_it.as_secs())
        }
        Some((kind, _)) if state.still_ahead > 0 => {
            format!("and the writer is working through them (now on one {kind})")
        }
        Some((kind, _)) => format!("and the writer is on one {kind}"),
        None => "and the writer is between messages".to_string(),
    };
    (warn, format!("{head}: {ahead}, {writer}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_names_the_message_until_it_ends() {
        let busy = BusyOn::default();
        assert!(busy.snapshot().is_none());
        busy.start("DeleteSubtreeById");
        assert_eq!(busy.snapshot().map(|(kind, _)| kind), Some("DeleteSubtreeById"));
        busy.end();
        assert!(busy.snapshot().is_none());
    }

    /// A sink that parks the writer inside its first emit until the test lets go:
    /// a stand-in for a message that never ends.
    struct ParksTheWriter {
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl crate::EventSink for ParksTheWriter {
        fn emit(&self, _event: crate::IndexEvent) {
            let _ = self.release.lock_ignore_poison().recv();
        }
    }

    /// The scan start's wait (`flush_blocking`) behind a writer stuck in one
    /// message: it keeps waiting, interval after interval, while the writer says
    /// which message holds it, and it answers as soon as the message ends.
    #[test]
    fn a_flush_behind_a_stuck_message_keeps_waiting_and_can_name_it() {
        use crate::indexing::writer::tests::setup_db;
        use crate::indexing::writer::{IndexWriter, WriteMessage};

        let (db_path, _dir) = setup_db();
        let (release, parked) = mpsc::channel();
        let sink = std::sync::Arc::new(ParksTheWriter {
            release: Mutex::new(parked),
        });
        let writer = IndexWriter::spawn(&db_path, sink).unwrap();

        writer
            .send(WriteMessage::EmitDirUpdated(vec!["/a".to_string()]))
            .unwrap();
        let (answered_tx, answered) = mpsc::channel();
        let flusher = writer.clone();
        std::thread::spawn(move || {
            let _ = answered_tx.send(flusher.flush_blocking_reporting_every_for_test(Duration::from_millis(20)));
        });

        // Several report intervals go by: the flush neither returns nor gives up.
        assert!(answered.recv_timeout(Duration::from_millis(200)).is_err());
        assert_eq!(writer.busy_on().map(|(kind, _)| kind), Some("EmitDirUpdated"));

        release.send(()).unwrap();
        answered
            .recv_timeout(Duration::from_secs(5))
            .expect("the flush answers once the message ends")
            .unwrap();
        drop(release);
        writer.shutdown();
    }

    fn state(ahead: u64, busy_on: Option<(&'static str, Duration)>, progressed: bool) -> WaitState {
        WaitState {
            still_ahead: ahead,
            busy_on,
            progressed,
        }
    }

    /// A wedge: one message has held the writer for longer than a report interval.
    /// That's a WARN, and it names the message and the volume.
    #[test]
    fn one_message_holding_the_writer_is_a_warning() {
        let (warn, line) = wait_report(
            "root",
            Duration::from_secs(60),
            STALL_REPORT_INTERVAL,
            state(0, Some(("DeleteSubtreeById", Duration::from_secs(61))), true),
        );
        assert!(warn);
        assert_eq!(
            line,
            "Waited 60 s for 'root''s index writer to flush: nothing else is ahead of it, \
             but the writer has been on one DeleteSubtreeById for 61 s"
        );
    }

    /// The 2026-11-25 dev log: a phase drain sat behind thousands of short walk
    /// writes for two minutes while the writer kept working through them. The line
    /// says how many are still ahead, and it's INFO, because the queue is moving.
    #[test]
    fn a_long_queue_that_keeps_draining_is_information_with_a_count() {
        let (warn, line) = wait_report(
            "root",
            Duration::from_secs(120),
            STALL_REPORT_INTERVAL,
            state(3_111, Some(("InsertSubtreeEntries", Duration::from_millis(120))), true),
        );
        assert!(!warn);
        assert_eq!(
            line,
            "Waited 120 s for 'root''s index writer to flush: about 3,111 messages are still ahead of it, \
             and the writer is working through them (now on one InsertSubtreeEntries)"
        );
    }

    /// A writer that took no message off its queue in a whole interval is stuck,
    /// whatever it's on.
    #[test]
    fn a_queue_that_stopped_moving_is_a_warning() {
        let (warn, line) = wait_report(
            "root",
            Duration::from_secs(30),
            STALL_REPORT_INTERVAL,
            state(1, None, false),
        );
        assert!(warn);
        assert_eq!(
            line,
            "Waited 30 s for 'root''s index writer to flush: about one message is still ahead of it, \
             and the writer is between messages"
        );
    }

    /// The count ahead is the depth at send time minus what the writer has taken
    /// since, floored at zero (the depth is a best-effort gauge).
    #[test]
    fn the_count_ahead_shrinks_as_the_writer_takes_messages() {
        let ticket = FlushTicket { ahead: 100, taken: 7 };
        assert_eq!(ticket.still_ahead(7), 100);
        assert_eq!(ticket.still_ahead(57), 50);
        assert_eq!(ticket.still_ahead(500), 0);
    }

    #[test]
    fn the_writer_counts_the_messages_it_takes() {
        let busy = BusyOn::default();
        busy.begin_message("UpsertEntryV2");
        busy.end();
        busy.start("ledger settle");
        busy.end();
        busy.begin_message("Flush");
        assert_eq!(busy.taken(), 2, "work that isn't a message doesn't count");
    }
}
