//! When a periodic network send is due, for the loops that talk to our servers on a schedule (the
//! analytics heartbeat and the update check).
//!
//! The rule is a throttle, never a debounce: at most one SUCCESSFUL send per `interval`, and a failed
//! send retried no sooner than `retry_floor` after it. A burst of triggers (a relaunch, a wake from
//! sleep) collapses into one send and never pushes the next one further out. The record is persisted
//! by its owner, so a relaunch doesn't reset the clock; the loops wake on a short tick and ask
//! [`SendRecord::due_in`] instead of sleeping a whole interval.
//!
//! Timestamps are wall-clock Unix milliseconds, since they have to survive a relaunch. A clock set
//! backwards could put a stored time in the future; it's read as "now", so the worst case is one
//! interval of waiting rather than a loop that never sends again.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How often a loop may send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendCadence {
    /// At most one successful send per this long.
    pub interval: Duration,
    /// A failed send is retried no sooner than this long after it.
    pub retry_floor: Duration,
}

/// The persisted memory of a send loop: when it last got through, and when it last didn't.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRecord {
    /// Unix ms of the last send the server acknowledged. `None` = never.
    #[serde(default)]
    pub last_success_ms: Option<i64>,
    /// Unix ms of the last failed send, cleared by the next success. `None` = the last one worked.
    #[serde(default)]
    pub last_failure_ms: Option<i64>,
}

impl SendRecord {
    /// How long until the next send is due, given the current wall clock. Zero means "send now".
    pub fn due_in(&self, now_ms: i64, cadence: SendCadence) -> Duration {
        let not_after_now = |t: i64| t.min(now_ms);
        let after_success = self
            .last_success_ms
            .map(|t| not_after_now(t).saturating_add(millis(cadence.interval)));
        let after_failure = self
            .last_failure_ms
            .map(|t| not_after_now(t).saturating_add(millis(cadence.retry_floor)));
        let due_at = after_success.max(after_failure).unwrap_or(now_ms);
        Duration::from_millis(u64::try_from(due_at.saturating_sub(now_ms)).unwrap_or(0))
    }

    /// Records a send the server acknowledged.
    pub fn record_success(&mut self, now_ms: i64) {
        self.last_success_ms = Some(now_ms);
        self.last_failure_ms = None;
    }

    /// Records a send that didn't get through.
    pub fn record_failure(&mut self, now_ms: i64) {
        self.last_failure_ms = Some(now_ms);
    }
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// The wall clock as Unix milliseconds, the unit [`SendRecord`] stores.
pub fn now_unix_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = 60 * 60 * 1000;
    const MINUTE_MS: i64 = 60 * 1000;

    fn cadence() -> SendCadence {
        SendCadence {
            interval: Duration::from_secs(3 * 60 * 60),
            retry_floor: Duration::from_secs(15 * 60),
        }
    }

    fn record(success: Option<i64>, failure: Option<i64>) -> SendRecord {
        SendRecord {
            last_success_ms: success,
            last_failure_ms: failure,
        }
    }

    #[test]
    fn a_loop_that_never_sent_is_due_now() {
        assert_eq!(SendRecord::default().due_in(1_000_000, cadence()), Duration::ZERO);
    }

    #[test]
    fn a_recent_success_holds_the_next_send_for_the_rest_of_the_interval() {
        let now = 10 * HOUR_MS;
        let r = record(Some(now - HOUR_MS), None);
        assert_eq!(r.due_in(now, cadence()), Duration::from_millis((2 * HOUR_MS) as u64));
    }

    #[test]
    fn a_success_older_than_the_interval_is_due_now() {
        let now = 10 * HOUR_MS;
        assert_eq!(
            record(Some(now - 4 * HOUR_MS), None).due_in(now, cadence()),
            Duration::ZERO
        );
    }

    /// A relaunch or a wake re-asks the same question and gets the same answer: triggers collapse
    /// and never push the next send out (throttle, not debounce).
    #[test]
    fn asking_again_never_moves_the_due_time() {
        let r = record(Some(0), None);
        let first = r.due_in(HOUR_MS, cadence());
        let later = r.due_in(HOUR_MS + 30 * MINUTE_MS, cadence());
        assert_eq!(first - later, Duration::from_millis((30 * MINUTE_MS) as u64));
    }

    #[test]
    fn a_failure_is_retried_after_the_floor_not_before() {
        let now = 10 * HOUR_MS;
        let r = record(Some(now - 5 * HOUR_MS), Some(now - 5 * MINUTE_MS));
        assert_eq!(r.due_in(now, cadence()), Duration::from_millis((10 * MINUTE_MS) as u64));
        assert_eq!(r.due_in(now + 10 * MINUTE_MS, cadence()), Duration::ZERO);
    }

    #[test]
    fn a_first_ever_send_that_failed_waits_out_the_floor() {
        let now = 10 * HOUR_MS;
        let r = record(None, Some(now - MINUTE_MS));
        assert_eq!(r.due_in(now, cadence()), Duration::from_millis((14 * MINUTE_MS) as u64));
    }

    #[test]
    fn success_clears_the_failure_and_starts_a_full_interval() {
        let mut r = record(Some(0), Some(HOUR_MS));
        r.record_success(5 * HOUR_MS);
        assert_eq!(r, record(Some(5 * HOUR_MS), None));
        assert_eq!(
            r.due_in(5 * HOUR_MS, cadence()),
            Duration::from_millis((3 * HOUR_MS) as u64)
        );
    }

    #[test]
    fn failure_keeps_the_last_success() {
        let mut r = record(Some(HOUR_MS), None);
        r.record_failure(5 * HOUR_MS);
        assert_eq!(r, record(Some(HOUR_MS), Some(5 * HOUR_MS)));
    }

    /// A clock set backwards must not park the loop until the stored future arrives.
    #[test]
    fn a_timestamp_in_the_future_waits_one_interval_at_most() {
        let now = 10 * HOUR_MS;
        let r = record(Some(now + 1000 * HOUR_MS), None);
        assert_eq!(r.due_in(now, cadence()), Duration::from_millis((3 * HOUR_MS) as u64));
    }

    #[test]
    fn the_record_serializes_in_camel_case_and_reads_an_empty_object() {
        let json = serde_json::to_value(record(Some(1), None)).expect("serialize");
        assert_eq!(json, serde_json::json!({ "lastSuccessMs": 1, "lastFailureMs": null }));
        let empty: SendRecord = serde_json::from_str("{}").expect("parse");
        assert_eq!(empty, SendRecord::default());
    }
}
