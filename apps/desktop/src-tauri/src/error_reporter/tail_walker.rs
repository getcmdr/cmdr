//! Tail walker: read a log file from the end backwards in fixed-size chunks, yielding
//! lines newest-first. Stop the moment a leading ISO-8601 timestamp falls older than a
//! cutoff. Used by Flow A's "past five hours of log content" filter so we don't have to read
//! and redact the entire ~180 MB log dir to ship a 1 MB triage bundle.
//!
//! ## Why backwards?
//!
//! Flow A's window is "the last N hours of content," anchored on `now`. The newest
//! lines live at the END of the file. Reading forward from the start means redacting and
//! buffering everything older too, wasting CPU when 99 %+ of the file is outside the
//! window. Reading backward from the end lets us bail the instant we cross the cutoff.
//!
//! ## Multi-line entries (panic backtraces)
//!
//! Lines without a parseable leading timestamp are continuation lines of a multi-line
//! record (panic backtraces). They pass through with their in-window
//! header. Because the walker encounters continuations before their header, it remembers
//! the last complete-record boundary and rolls back those lines if that header is outside
//! the window. This prevents a dropped header from leaving an orphaned continuation leak.
//!
//! ## Long lines spanning multiple chunks
//!
//! Lines longer than `CHUNK_SIZE` (state YAML, big backtraces, ~10 KB stack frames) are
//! handled by accumulating chunks in a tail buffer until a `\n` shows up. No max line
//! length is assumed.
//!
//! ## CRLF
//!
//! The fern file chain writes LF only, but we strip a trailing `\r` defensively in case
//! a future change introduces CRLF anywhere (or someone hand-edits a log on Windows).

use jiff::Timestamp;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Read this many bytes per backward seek. Picked to be large enough that "typical" lines
/// (a few hundred bytes) come in batches of dozens, but small enough that a 100 ms seek on
/// a network mount doesn't show up as a stutter. 64 KB matches the file chain's write
/// buffer, so we usually pick up exactly one rotation's worth of buffered writes per pull.
const CHUNK_SIZE: usize = 64 * 1024;

/// Outcome of walking a single log file from the tail.
///
/// `lines` is in **forward order** (oldest kept first, newest last); we reverse before
/// returning so callers can write them straight into the zip without flipping again.
/// `hit_cutoff` is true when the walker stopped because it crossed the timestamp cutoff
/// (vs. ran out of file). Callers use this to decide whether to keep walking older
/// rotations (no, we have everything we need) or to keep going (yes, the window
/// extends past this file's age).
pub struct TailWalkResult {
    /// Lines kept, oldest-first. Newlines stripped. `\r` trim-trailed.
    pub lines: Vec<String>,
    /// True if the walker stopped at the cutoff. False if it consumed the whole file.
    pub hit_cutoff: bool,
}

/// Read `path` from the end backward. Yield lines until either (a) a leading timestamp
/// is older than `cutoff`, or (b) the file is exhausted.
///
/// Lines without a parseable leading ISO-8601 timestamp are kept as continuation context
/// without triggering a cutoff check (see the module doc).
///
/// Errors are mapped to "no lines, did not hit cutoff" so the caller can keep going. We
/// log a warning at the same call site the legacy file-by-file reader did. An empty file
/// returns `(empty lines, hit_cutoff = false)`.
pub fn walk_tail(path: &Path, cutoff: Timestamp) -> std::io::Result<TailWalkResult> {
    let mut file = File::open(path)?;
    let total_len = file.seek(SeekFrom::End(0))?;
    if total_len == 0 {
        return Ok(TailWalkResult {
            lines: Vec::new(),
            hit_cutoff: false,
        });
    }

    // Newest-first scratch list. Reversed at the end.
    let mut newest_first: Vec<String> = Vec::new();
    // End of the records whose timestamped headers have already been accepted. Any later
    // entries in `newest_first` are continuations waiting for their older header.
    let mut complete_records_end = 0;
    let mut hit_cutoff = false;

    // `pending` holds bytes we've read but haven't split into a complete line yet. It's
    // the SUFFIX of the not-yet-yielded prefix of the file: if we read backward and the
    // chunk is `b"abc\ndefgh"`, "defgh" is held until the next chunk shows where its
    // line begins (or until we hit byte 0).
    let mut pending: Vec<u8> = Vec::new();
    let mut cursor = total_len;
    let mut buf = vec![0u8; CHUNK_SIZE];

    'outer: while cursor > 0 {
        let read_len = cursor.min(CHUNK_SIZE as u64) as usize;
        cursor -= read_len as u64;
        file.seek(SeekFrom::Start(cursor))?;
        let slice = &mut buf[..read_len];
        file.read_exact(slice)?;

        // Prepend chunk to pending, building combined = chunk_bytes + pending.
        let mut combined: Vec<u8> = Vec::with_capacity(read_len + pending.len());
        combined.extend_from_slice(slice);
        combined.extend_from_slice(&pending);
        pending.clear();

        // Walk newlines from RIGHT to LEFT inside `combined`. Each `\n` separates
        // a finished line on its right from earlier text on its left. When we cross a
        // newline, we yield the right-hand line. The leftmost segment (before the
        // earliest `\n` we found in `combined`) becomes the new `pending`, since it might
        // continue earlier in the file.
        let mut end = combined.len();
        while end > 0 {
            // Find the previous '\n' in combined[..end].
            match combined[..end].iter().rposition(|b| *b == b'\n') {
                Some(nl_idx) => {
                    let line_bytes = &combined[nl_idx + 1..end];
                    end = nl_idx;
                    if !try_emit_line(
                        line_bytes,
                        cutoff,
                        &mut newest_first,
                        &mut complete_records_end,
                        &mut hit_cutoff,
                    ) {
                        break 'outer;
                    }
                }
                None => {
                    // No more newlines in this chunk. Whatever's left is the start of a
                    // line that may continue in older bytes. Park it as `pending` and
                    // pull another chunk.
                    pending.extend_from_slice(&combined[..end]);
                    end = 0;
                }
            }
        }
    }

    // Anything left in `pending` is a line that runs from byte 0 (no earlier newline
    // could exist). Emit it last: it's the OLDEST line in the file.
    if !pending.is_empty() && !hit_cutoff {
        // Doesn't matter what the function returns here; we're at file start, the loop
        // is going to exit anyway.
        let _ = try_emit_line(
            &pending,
            cutoff,
            &mut newest_first,
            &mut complete_records_end,
            &mut hit_cutoff,
        );
    }

    newest_first.reverse();
    Ok(TailWalkResult {
        lines: newest_first,
        hit_cutoff,
    })
}

/// Try to emit one line. Returns `false` to signal the outer walker should stop
/// immediately (cutoff hit on a timestamped line). Returns `true` otherwise:
/// walker keeps consuming older bytes.
///
/// `kept` is appended to (newest-first order); `hit_cutoff` is set to `true` once the
/// cutoff fires.
fn try_emit_line(
    raw: &[u8],
    cutoff: Timestamp,
    kept: &mut Vec<String>,
    complete_records_end: &mut usize,
    hit_cutoff: &mut bool,
) -> bool {
    let mut bytes = raw;
    if bytes.last() == Some(&b'\r') {
        bytes = &bytes[..bytes.len() - 1];
    }
    // Empty lines: skip silently. The most common case is the empty slice that follows
    // the file's final `\n`. Less common: a blank separator line in the middle of the
    // log. Either way, blank lines have nothing useful for triage.
    if bytes.is_empty() {
        return true;
    }
    // Logs are UTF-8 by convention; `from_utf8_lossy` preserves bytes triagers can read
    // even when a line has a stray non-UTF-8 sequence (rare but the redactor handles it
    // downstream; we don't want to drop lines for that).
    let line = String::from_utf8_lossy(bytes).into_owned();

    let line_timestamp = parse_leading_iso8601(&line);
    if let Some(line_ts) = line_timestamp
        && line_ts < cutoff
    {
        // The untimestamped lines encountered since the last accepted header belong to
        // this out-of-window record. Remove them with the header so no partial record ships.
        kept.truncate(*complete_records_end);
        *hit_cutoff = true;
        return false;
    }

    kept.push(line);
    if line_timestamp.is_some() {
        *complete_records_end = kept.len();
    }
    true
}

/// Parses an ISO-8601 stamp at the start of a log line (matches the format produced by
/// `logging::dispatch::file_timestamp`: `YYYY-MM-DDTHH:MM:SS.mmm±HH:MM`).
///
/// Returns `None` for lines that don't start with one (legacy `HH:MM:SS.mmm`-stamped lines,
/// blank lines, panic-backtrace continuation lines, redacted-payload lines, etc.). Callers
/// fall back to keeping the line in that case rather than risk a false drop.
pub fn parse_leading_iso8601(line: &str) -> Option<Timestamp> {
    // The timestamp is always 29 bytes: 23 for date+time+ms + 6 for `±HH:MM`, all ASCII.
    // `get(..29)` returns `None` (rather than panicking) when byte 29 splits a multibyte
    // char, which happens on non-timestamp lines (backtrace frames, captured output, paths
    // with accented names); those have no leading stamp, so falling through to `None` is right.
    let candidate = line.get(..29)?;
    Timestamp::strptime("%Y-%m-%dT%H:%M:%S%.3f%:z", candidate).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use jiff::SignedDuration;
    use std::io::Write;

    /// Returns the scratch dir alongside the file path: the caller has to keep
    /// the `TestDir` bound, or the directory (and the file) go away immediately.
    fn write_tmp(name: &str, body: &[u8]) -> (TestDir, std::path::PathBuf) {
        let dir = TestDir::new(&format!("tail-walker-{name}"));
        let p = dir.join("log");
        let mut f = File::create(&p).expect("create");
        f.write_all(body).expect("write");
        (dir, p)
    }

    fn iso_line(ts: Timestamp, body: &str) -> String {
        format!("{} {body}", crate::logging::dispatch::file_timestamp_at(ts))
    }

    #[test]
    fn reads_short_file_in_reverse_when_no_cutoff() {
        let now = Timestamp::now();
        let body = format!(
            "{}\n{}\n{}\n",
            iso_line(now - SignedDuration::from_secs(3), "A"),
            iso_line(now - SignedDuration::from_secs(2), "B"),
            iso_line(now - SignedDuration::from_secs(1), "C"),
        );
        let (_dir, p) = write_tmp("basic", body.as_bytes());
        let result = walk_tail(&p, now - SignedDuration::from_hours(24)).unwrap();
        assert_eq!(result.lines.len(), 3);
        assert!(result.lines[0].ends_with(" A"));
        assert!(result.lines[1].ends_with(" B"));
        assert!(result.lines[2].ends_with(" C"));
        // Reached EOF, didn't cut.
        assert!(!result.hit_cutoff);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn long_line_spanning_multiple_chunks() {
        let now = Timestamp::now();
        // 200 KB single line: three+ chunks worth of bytes for one line.
        let big_body: String = "x".repeat(200 * 1024);
        let big_line = iso_line(now - SignedDuration::from_secs(1), &big_body);
        let body = format!(
            "{}\n{}\n",
            iso_line(now - SignedDuration::from_secs(2), "short"),
            big_line,
        );
        let (_dir, p) = write_tmp("longline", body.as_bytes());
        let result = walk_tail(&p, now - SignedDuration::from_hours(24)).unwrap();
        assert_eq!(result.lines.len(), 2);
        assert!(result.lines[0].ends_with(" short"));
        assert_eq!(result.lines[1], big_line);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn no_trailing_newline() {
        let now = Timestamp::now();
        let body = format!(
            "{}\n{}",
            iso_line(now - SignedDuration::from_secs(2), "first"),
            iso_line(now - SignedDuration::from_secs(1), "last-without-newline"),
        );
        let (_dir, p) = write_tmp("no-trailing-nl", body.as_bytes());
        let result = walk_tail(&p, now - SignedDuration::from_hours(24)).unwrap();
        assert_eq!(result.lines.len(), 2);
        assert!(result.lines[1].ends_with(" last-without-newline"));
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn empty_file_returns_no_lines() {
        let (_dir, p) = write_tmp("empty", b"");
        let result = walk_tail(&p, Timestamp::now()).unwrap();
        assert!(result.lines.is_empty());
        assert!(!result.hit_cutoff);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn untimestamped_continuation_lines_pass_through() {
        // A panic-style block in the middle: header has a timestamp, frames don't.
        // The cutoff sits between "very old timestamped" and the rest.
        let now = Timestamp::now();
        let inside = now - SignedDuration::from_mins(10);
        let outside = now - SignedDuration::from_hours(2);

        let body = format!(
            "{old} OLD line\n{header} ERROR something panicked\n   stack: frame 0\n   stack: frame 1\n{newer} INFO recovered\n",
            old = iso_line(outside, "").trim_end(),
            header = iso_line(inside, "").trim_end(),
            newer = iso_line(now - SignedDuration::from_mins(1), "").trim_end(),
        );
        let (_dir, p) = write_tmp("multiline", body.as_bytes());
        let result = walk_tail(&p, now - SignedDuration::from_hours(1)).unwrap();
        // Should keep: the recovered line, the panic header + 2 frames. Drop the OLD
        // line. Order: oldest-first (header, frame 0, frame 1, recovered).
        assert!(result.hit_cutoff, "should have stopped at the cutoff");
        let joined = result.lines.join("\n");
        assert!(!joined.contains("OLD line"), "old line must be dropped: {joined}");
        assert!(joined.contains("ERROR something panicked"));
        assert!(joined.contains("frame 0"));
        assert!(joined.contains("frame 1"));
        assert!(joined.contains("recovered"));
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn cutoff_removes_an_old_records_continuations_but_keeps_a_new_backtrace() {
        let now = Timestamp::now();
        let body = format!(
            "{old}\nprivate YAML continuation\nprivate nested continuation\n{new}\n   safe frame 0\n   safe frame 1\n",
            old = iso_line(now - SignedDuration::from_hours(2), "DEBUG old record"),
            new = iso_line(now - SignedDuration::from_mins(1), "DEBUG backtrace"),
        );
        let (_dir, p) = write_tmp("record-cutoff", body.as_bytes());
        let result = walk_tail(&p, now - SignedDuration::from_hours(1)).unwrap();
        let joined = result.lines.join("\n");
        assert!(result.hit_cutoff);
        assert!(!joined.contains("private YAML"));
        assert!(!joined.contains("private nested"));
        assert!(joined.contains("DEBUG backtrace"));
        assert!(joined.contains("safe frame 0"));
        assert!(joined.contains("safe frame 1"));
    }

    #[test]
    fn window_boundary_drops_older_timestamped_lines() {
        let now = Timestamp::now();
        let cutoff = now - SignedDuration::from_mins(30);
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            iso_line(now - SignedDuration::from_hours(2), "way-old"),
            iso_line(now - SignedDuration::from_hours(1), "old"),
            iso_line(now - SignedDuration::from_mins(20), "kept-1"),
            iso_line(now - SignedDuration::from_mins(5), "kept-2"),
        );
        let (_dir, p) = write_tmp("window", body.as_bytes());
        let result = walk_tail(&p, cutoff).unwrap();
        let joined = result.lines.join("\n");
        assert!(joined.contains("kept-1"));
        assert!(joined.contains("kept-2"));
        assert!(!joined.contains("way-old"));
        assert!(!joined.contains(" old"));
        assert!(result.hit_cutoff);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn crlf_trailing_carriage_return_is_trimmed() {
        let now = Timestamp::now();
        let body = format!("{}\r\n", iso_line(now - SignedDuration::from_secs(1), "winline"));
        let (_dir, p) = write_tmp("crlf", body.as_bytes());
        let result = walk_tail(&p, now - SignedDuration::from_hours(24)).unwrap();
        assert_eq!(result.lines.len(), 1);
        assert!(!result.lines[0].ends_with('\r'));
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn multibyte_char_straddling_byte_29_returns_none_without_panic() {
        // A non-timestamp log line (a panic-backtrace frame, captured output, or a path with
        // an accented name) whose 29th byte lands inside a multibyte UTF-8 char. The function
        // slices the first 29 *bytes*, and a naive `&line[..29]` panics here with
        // `slice_error_fail` (byte 29 isn't a char boundary), crashing the Flow B bundle build.
        // It must return `None` (no leading ISO-8601 stamp) instead.
        let line = format!("{}ä trailing", "x".repeat(28));
        // 'x'*28 fills bytes 0..28; the 'ä' (2 bytes) occupies 28..30, so byte 29 splits it.
        assert!(
            line.len() >= 29 && !line.is_char_boundary(29),
            "test precondition: line is ≥29 bytes and byte 29 splits a char",
        );
        assert_eq!(parse_leading_iso8601(&line), None);
    }

    /// The log-stamp pair: the writer's bytes for a fixed instant and offset are the ones Cmdr
    /// has always written (`YYYY-MM-DDTHH:MM:SS.mmm±HH:MM`), and the parser reads such a line back to the same
    /// instant, so old log files and new ones look alike to every reader.
    #[test]
    fn the_log_stamp_keeps_its_bytes_and_reads_back_to_the_same_instant() {
        // 2026-04-24T23:18:28.218999999Z: the writer truncates to the millisecond.
        let at = Timestamp::new(1_777_072_708, 218_999_999).expect("in range");
        for (offset_secs, expected) in [
            (0, "2026-04-24T23:18:28.218+00:00"),
            (2 * 3600, "2026-04-25T01:18:28.218+02:00"),
            (-(5 * 3600 + 30 * 60), "2026-04-24T17:48:28.218-05:30"),
        ] {
            let offset = jiff::tz::Offset::from_seconds(offset_secs).expect("valid offset");
            // The writer `logging::dispatch::file_timestamp_at` hands its offset to.
            let stamp = crate::timestamps::log_file_stamp(at, offset);
            assert_eq!(stamp, expected);
            let line = format!("{stamp} INFO  · cmdr_lib::x: hello");
            let parsed = parse_leading_iso8601(&line).expect("our own stamp parses");
            assert_eq!(parsed.as_millisecond(), 1_777_072_708_218, "{line}");
        }
    }

    /// The writer at this machine's offset round-trips through the parser to the millisecond.
    #[test]
    fn file_timestamp_at_round_trips_through_the_parser() {
        for (secs, nanos, millis) in [
            (1_777_072_708, 0, 1_777_072_708_000),
            (1_777_072_708, 218_999_999, 1_777_072_708_218),
            (1_794_916_800, 1, 1_794_916_800_000),
        ] {
            let at = Timestamp::new(secs, nanos).expect("in range");
            let line = iso_line(at, "INFO x");
            assert_eq!(line.find(' '), Some(29), "{line}");
            let parsed = parse_leading_iso8601(&line).expect("parses");
            assert_eq!(parsed.as_millisecond(), millis, "{line}");
        }
    }

    /// A line stamped exactly at the cutoff stays; one a millisecond older stops the walk.
    #[test]
    fn the_cutoff_holds_to_the_millisecond() {
        let cutoff = Timestamp::new(1_777_072_708, 218_000_000).expect("in range");
        let body = format!(
            "{}\n{}\n",
            iso_line(cutoff - SignedDuration::from_millis(1), "OUTSIDE"),
            iso_line(cutoff, "EDGE"),
        );
        let (_dir, p) = write_tmp("cutoff-edge", body.as_bytes());
        let result = walk_tail(&p, cutoff).unwrap();
        assert_eq!(result.lines, [iso_line(cutoff, "EDGE")]);
        assert!(result.hit_cutoff);
    }

    /// Lines written before the current writer parse to the same instant.
    #[test]
    fn old_log_lines_parse_to_the_same_instant() {
        for (line, millis) in [
            ("2026-04-25T01:18:28.218+02:00 INFO  hello", 1_777_072_708_218),
            (
                "2026-04-24T23:18:28.218+00:00 DEBUG · cmdr_lib::x: y",
                1_777_072_708_218,
            ),
            ("2026-04-24T17:48:28.218-05:30 WARN  z", 1_777_072_708_218),
            ("2026-11-17T13:00:00.000+01:00", 1_794_916_800_000),
        ] {
            let parsed = parse_leading_iso8601(line).expect("an old line parses");
            assert_eq!(parsed.as_millisecond(), millis, "{line}");
        }
    }

    /// Anything that isn't the writer's exact 29-byte shape is "no stamp", so the line rides
    /// along with its record instead of deciding the cutoff.
    #[test]
    fn lines_without_the_exact_stamp_shape_have_no_timestamp() {
        for line in [
            "2026-04-25T01:18:28.21+02:00 INFO two fraction digits",
            "2026-04-25T01:18:28.2180+02:00 INFO four fraction digits",
            "2026-04-25T01:18:28+02:00 INFO no fraction at all here",
            "2026-04-24T23:18:28.218Z INFO a Z instead of an offset",
            "2026-04-25T01:18:28.218+0200 INFO no colon in the offset",
            "2026-04-25 01:18:28.218+02:00 INFO space separator",
            "2026-4-25T01:18:28.218+02:00 INFO one-digit month",
            "2026-04-25T01:18:28.218+02",
            "01:18:28.218 INFO legacy terminal-style stamp",
            "   0: cmdr_lib::some::backtrace_frame",
            "",
        ] {
            assert_eq!(parse_leading_iso8601(line), None, "{line:?}");
        }
    }
}
