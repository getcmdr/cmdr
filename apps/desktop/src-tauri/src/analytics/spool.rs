//! The on-disk queue feature events wait in until a heartbeat carries them.
//!
//! One JSON object per line (`analytics-events.jsonl` in the app data dir), appended as events fire
//! and read from the front by the heartbeat. It survives quits and crashes, which is also what makes
//! an offline session's events arrive later instead of never.
//!
//! Three rules, all enforced here under one lock:
//!
//! - **Only an acknowledged batch leaves.** [`Spool::take_batch`] copies from the front and removes
//!   nothing; [`Spool::acknowledge`] drops exactly those lines once the server answered 2xx. Events
//!   appended while a beat is in flight land behind the batch and stay.
//! - **Bounded, oldest first.** Past [`MAX_SPOOLED_EVENTS`] plus some slack, the front is cut back to
//!   the cap, so a long-offline install can't grow the file without bound.
//! - **A trim during a beat can't cost an unsent event.** Every line removed from the front bumps
//!   `removed_from_front`, and a batch remembers the count it was taken at, so its acknowledgment
//!   removes only the part of it that's still there.
//!
//! A line that doesn't parse (a torn write from a crash) is skipped by the batch but counted in it, so
//! the next acknowledgment clears it rather than it blocking the front forever.

use crate::ignore_poison::IgnorePoison;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// The spool never holds more than this many events once a trim has run. At a few hundred events a
/// day that's weeks of being offline.
pub(super) const MAX_SPOOLED_EVENTS: usize = 5_000;

/// How far past the cap the file may grow before a trim rewrites it, so a full spool isn't rewritten
/// on every append.
const TRIM_SLACK: usize = MAX_SPOOLED_EVENTS / 10;

/// A single spooled line longer than this is refused at append: no categorical event gets near it,
/// and one that did could push a whole beat past the server's body cap.
pub(super) const MAX_EVENT_LINE_BYTES: usize = 8 * 1024;

/// The filename, in the app data dir.
pub(super) const SPOOL_FILE_NAME: &str = "analytics-events.jsonl";

/// One feature event as it waits in the spool, and exactly as a heartbeat carries it (the `events`
/// items of `POST /heartbeat`, `docs/specs/network-chatter-plan.md`). Only the event's own
/// properties: the server adds the install's identity and config when it forwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpooledEvent {
    /// The event name, `[a-z0-9_$]`, 1–100 chars.
    pub event: String,
    /// When it fired on this machine, RFC 3339 UTC.
    pub timestamp: String,
    /// A lowercase hyphenated v4 UUID minted when the event fired. A beat the server stored but
    /// whose answer got lost is sent again, and the server keeps each id once.
    pub id: String,
    /// The app version that fired it. An event spooled before an update can ship after it, and the
    /// server prefers this over the beat's own version.
    pub app_version: String,
    /// The event's own PII-free properties.
    pub properties: serde_json::Map<String, serde_json::Value>,
}

pub(super) struct Spool {
    path: PathBuf,
    state: Mutex<SpoolState>,
}

#[derive(Default)]
struct SpoolState {
    /// Lines in the file, counted on first use. `None` until then.
    lines: Option<usize>,
    /// Lines removed from the front since this process started (trims, acknowledgments, clears).
    removed_from_front: u64,
}

/// What a beat carries, plus what the spool needs to acknowledge it later.
#[derive(Debug)]
pub(super) struct Batch {
    pub events: Vec<SpooledEvent>,
    /// Lines this batch covers, including any that didn't parse.
    lines: usize,
    /// `removed_from_front` when the batch was taken.
    front_mark: u64,
}

impl Spool {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(SpoolState::default()),
        }
    }

    /// Appends one event. Refuses (and logs) an event whose line would exceed
    /// [`MAX_EVENT_LINE_BYTES`]. Trims the front when the spool runs past its cap.
    pub fn append(&self, event: &SpooledEvent) {
        let Ok(line) = serde_json::to_string(event) else { return };
        if line.len() > MAX_EVENT_LINE_BYTES {
            log::warn!(target: "analytics", "Event '{}' dropped: {} bytes is over the spool's line cap", event.event, line.len());
            return;
        }
        let mut state = self.state.lock_ignore_poison();
        let lines = self.line_count(&mut state);
        if let Err(e) = self.append_line(&line) {
            log::debug!(target: "analytics", "Couldn't spool event '{}': {e}", event.event);
            return;
        }
        state.lines = Some(lines + 1);
        if lines + 1 > MAX_SPOOLED_EVENTS + TRIM_SLACK {
            let excess = lines + 1 - MAX_SPOOLED_EVENTS;
            self.remove_front(&mut state, excess);
        }
    }

    /// Copies up to `max_events` events from the front, stopping early rather than going past
    /// `max_bytes` of serialized lines. Removes nothing.
    pub fn take_batch(&self, max_events: usize, max_bytes: usize) -> Batch {
        let state = self.state.lock_ignore_poison();
        let front_mark = state.removed_from_front;
        let contents = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut events = Vec::new();
        let mut lines = 0;
        let mut bytes = 0;
        for line in contents.lines() {
            if events.len() == max_events || (lines > 0 && bytes + line.len() > max_bytes) {
                break;
            }
            lines += 1;
            match serde_json::from_str::<SpooledEvent>(line) {
                Ok(event) => {
                    bytes += line.len();
                    events.push(event);
                }
                Err(_) => log::debug!(target: "analytics", "Skipping an unreadable spool line"),
            }
        }
        Batch {
            events,
            lines,
            front_mark,
        }
    }

    /// Drops what `batch` covered, now that the server has it. Lines a trim already removed since
    /// the batch was taken aren't removed twice.
    pub fn acknowledge(&self, batch: &Batch) {
        let mut state = self.state.lock_ignore_poison();
        let already_gone = usize::try_from(state.removed_from_front - batch.front_mark).unwrap_or(usize::MAX);
        let remaining = batch.lines.saturating_sub(already_gone);
        if remaining > 0 {
            self.remove_front(&mut state, remaining);
        }
    }

    /// Deletes every spooled event (the user opted out).
    pub fn clear(&self) {
        let mut state = self.state.lock_ignore_poison();
        let lines = self.line_count(&mut state);
        if lines == 0 && !self.path.exists() {
            return;
        }
        if let Err(e) = std::fs::remove_file(&self.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::debug!(target: "analytics", "Couldn't clear the event spool: {e}");
            return;
        }
        state.lines = Some(0);
        state.removed_from_front += lines as u64;
    }

    /// Lines in the file. The first call reads it, and closes off a torn last line (a crash
    /// mid-append) so the next append starts a line of its own instead of gluing onto it.
    fn line_count(&self, state: &mut SpoolState) -> usize {
        if let Some(lines) = state.lines {
            return lines;
        }
        let contents = std::fs::read_to_string(&self.path).unwrap_or_default();
        if !contents.is_empty() && !contents.ends_with('\n') && self.append_line("").is_err() {
            log::debug!(target: "analytics", "Couldn't close off a torn spool line");
        }
        let lines = contents.lines().count();
        state.lines = Some(lines);
        lines
    }

    /// Appends `line` plus a newline. An empty `line` writes just the newline.
    fn append_line(&self, line: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        file.write_all(format!("{line}\n").as_bytes())
    }

    /// Rewrites the file without its first `count` lines (temp + rename, so a crash leaves either
    /// the old file or the new one).
    fn remove_front(&self, state: &mut SpoolState, count: usize) {
        let contents = std::fs::read_to_string(&self.path).unwrap_or_default();
        let kept: Vec<&str> = contents.lines().skip(count).collect();
        let removed = contents.lines().count() - kept.len();
        let mut rest = kept.join("\n");
        if !rest.is_empty() {
            rest.push('\n');
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        if let Err(e) = crate::config::durable_write_json(&self.path, &tmp, &rest) {
            log::debug!(target: "analytics", "Couldn't rewrite the event spool: {e}");
            return;
        }
        state.lines = Some(kept.len());
        state.removed_from_front += removed as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &str) -> SpooledEvent {
        SpooledEvent {
            event: name.to_string(),
            timestamp: "2026-09-24T10:00:00.000Z".to_string(),
            id: "0f8fad5b-d9cb-469f-a165-70867728950e".to_string(),
            app_version: "1.2.3".to_string(),
            properties: json!({ "kind": "local" }).as_object().cloned().unwrap_or_default(),
        }
    }

    fn names(events: &[SpooledEvent]) -> Vec<&str> {
        events.iter().map(|e| e.event.as_str()).collect()
    }

    fn spool() -> (tempfile::TempDir, Spool) {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = Spool::new(dir.path().join(SPOOL_FILE_NAME));
        (dir, spool)
    }

    #[test]
    fn appended_events_replay_in_order() {
        let (_dir, spool) = spool();
        spool.append(&event("a"));
        spool.append(&event("b"));
        let batch = spool.take_batch(500, usize::MAX);
        assert_eq!(names(&batch.events), ["a", "b"]);
        assert_eq!(batch.events[0], event("a"));
    }

    #[test]
    fn taking_a_batch_removes_nothing() {
        let (_dir, spool) = spool();
        spool.append(&event("a"));
        let _ = spool.take_batch(500, usize::MAX);
        assert_eq!(names(&spool.take_batch(500, usize::MAX).events), ["a"]);
    }

    #[test]
    fn a_batch_stops_at_the_event_limit() {
        let (_dir, spool) = spool();
        for name in ["a", "b", "c"] {
            spool.append(&event(name));
        }
        assert_eq!(names(&spool.take_batch(2, usize::MAX).events), ["a", "b"]);
    }

    #[test]
    fn a_batch_stops_before_the_byte_budget() {
        let (_dir, spool) = spool();
        for name in ["a", "b", "c"] {
            spool.append(&event(name));
        }
        let one_line = serde_json::to_string(&event("a")).expect("serialize").len();
        let batch = spool.take_batch(500, one_line * 2 + 1);
        assert_eq!(names(&batch.events), ["a", "b"]);
    }

    #[test]
    fn acknowledging_drops_only_the_batch() {
        let (_dir, spool) = spool();
        spool.append(&event("a"));
        spool.append(&event("b"));
        let batch = spool.take_batch(1, usize::MAX);
        spool.acknowledge(&batch);
        assert_eq!(names(&spool.take_batch(500, usize::MAX).events), ["b"]);
    }

    /// The concurrency case: an event that fires while a beat is on the wire must survive that
    /// beat's acknowledgment.
    #[test]
    fn events_appended_during_a_beat_survive_its_acknowledgment() {
        let (_dir, spool) = spool();
        spool.append(&event("a"));
        let batch = spool.take_batch(500, usize::MAX);
        spool.append(&event("during"));
        spool.acknowledge(&batch);
        assert_eq!(names(&spool.take_batch(500, usize::MAX).events), ["during"]);
    }

    /// A trim that runs while a beat is in flight already removed part of the batch; the
    /// acknowledgment must not then eat that many unsent events from behind it.
    #[test]
    fn a_trim_during_a_beat_never_costs_an_unsent_event() {
        let (_dir, spool) = spool();
        for i in 0..MAX_SPOOLED_EVENTS {
            spool.append(&event(&format!("old_{i}")));
        }
        let batch = spool.take_batch(usize::MAX, usize::MAX);
        for i in 0..=TRIM_SLACK {
            spool.append(&event(&format!("new_{i}")));
        }
        spool.acknowledge(&batch);
        let rest = spool.take_batch(usize::MAX, usize::MAX);
        assert_eq!(
            rest.events.len(),
            TRIM_SLACK + 1,
            "only the events appended during the beat are left"
        );
        assert!(rest.events.iter().all(|e| e.event.starts_with("new_")));
    }

    #[test]
    fn the_spool_is_capped_by_dropping_the_oldest() {
        let (_dir, spool) = spool();
        for i in 0..MAX_SPOOLED_EVENTS + TRIM_SLACK + 1 {
            spool.append(&event(&format!("e_{i}")));
        }
        let all = spool.take_batch(usize::MAX, usize::MAX);
        assert_eq!(all.events.len(), MAX_SPOOLED_EVENTS);
        assert_eq!(all.events[0].event, format!("e_{}", TRIM_SLACK + 1));
    }

    #[test]
    fn an_unreadable_line_is_skipped_and_cleared_by_the_next_acknowledgment() {
        let (dir, spool) = spool();
        std::fs::write(dir.path().join(SPOOL_FILE_NAME), "{\"torn").expect("write");
        spool.append(&event("a"));
        let batch = spool.take_batch(500, usize::MAX);
        assert_eq!(names(&batch.events), ["a"]);
        spool.acknowledge(&batch);
        assert!(spool.take_batch(500, usize::MAX).events.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(SPOOL_FILE_NAME)).expect("read"),
            ""
        );
    }

    #[test]
    fn an_oversized_event_is_refused() {
        let (_dir, spool) = spool();
        let mut big = event("big");
        big.properties
            .insert("blob".to_string(), json!("x".repeat(MAX_EVENT_LINE_BYTES)));
        spool.append(&big);
        assert!(spool.take_batch(500, usize::MAX).events.is_empty());
    }

    #[test]
    fn clearing_empties_the_spool() {
        let (_dir, spool) = spool();
        spool.append(&event("a"));
        spool.clear();
        assert!(spool.take_batch(500, usize::MAX).events.is_empty());
    }

    #[test]
    fn a_missing_file_is_an_empty_spool() {
        let (_dir, spool) = spool();
        let batch = spool.take_batch(500, usize::MAX);
        assert!(batch.events.is_empty());
        spool.acknowledge(&batch);
        spool.clear();
    }
}
