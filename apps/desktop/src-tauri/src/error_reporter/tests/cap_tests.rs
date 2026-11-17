//! The size cap, in both pipelines: what survives when a bundle is too big. The newest lines
//! always win, and the fit is measured in compressed bytes, since that's what the cap limits.

use super::{prepared, read_zip_entries, sample_manifest, test_redaction};
use crate::error_reporter::BundleManifest;
use crate::error_reporter::bundle_builder::{PreparedFile, build_bundle_streaming_to_cap, build_zip};
use crate::error_reporter::bundle_capper::cap_bundle_to_bytes;
use crate::error_reporter::cap_bundle_to_mb;
use crate::logging::dispatch::file_timestamp_at as iso;
use crate::test_support::TestDir;
use jiff::{SignedDuration, Timestamp};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::time::{Duration, SystemTime};

fn make_log_dir(name: &str) -> TestDir {
    TestDir::new(&format!("cap-{name}"))
}

#[test]
fn cap_bundle_is_no_op_when_under_cap() {
    let now = SystemTime::now();
    let mut files = BTreeMap::new();
    files.insert("cmdr.log".to_string(), prepared(vec!["short"], now));
    let bytes = build_zip(&sample_manifest(), &files, now).unwrap();
    let original_len = bytes.len();
    let original = bytes.clone();

    let capped = cap_bundle_to_mb(bytes, 10);
    assert_eq!(capped.len(), original_len);
    assert_eq!(capped, original);
}

/// Generates pseudo-random text that doesn't deflate well, so a 30 MB input stays
/// 30 MB-ish in the zip too.
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn synthetic_lines(file_idx: u32, count: u32) -> Vec<String> {
    (0u32..count)
        .map(|i| {
            let token: String = (0u32..30)
                .map(|k| {
                    let n = splitmix64(((file_idx as u64) << 40) ^ ((i as u64) << 16) ^ k as u64);
                    char::from(33u8 + ((n & 0xFF) as u8 % 90))
                })
                .collect();
            format!("INFO file={file_idx} idx={i} body={token}")
        })
        .collect()
}

/// Headline test for fix #6: pre-fix, capping a 30 MB bundle to 1 MB produced a
/// 542-byte zip with only the manifest. Post-fix: capping trims old content from the
/// head of the newest file, ships the tail under (or just over) the cap, and proves
/// the LAST input line survives.
///
/// Uses the bytes-precision [`cap_bundle_to_bytes`] entry point (the prod API
/// [`cap_bundle_to_mb`] is a wrapper for the same logic at MB granularity). The
/// behavior under test is scale-invariant — pre-fix, the bug reproduced at any input
/// size as long as the cap engaged. A 100 KB cap with a ~250 KB input exercises the
/// same trim-from-head-keep-tail logic in ~50 ms instead of the ~2 s warm / >8 s
/// contended that 200 000 synthetic lines + 1 MB cap used to cost. See the test
/// commit for the previous shape if a future regression seems to need the bigger
/// fixture.
#[test]
fn cap_bundle_keeps_newest_lines_and_manifest() {
    let now = SystemTime::now();
    let mut files: BTreeMap<String, PreparedFile> = BTreeMap::new();
    // 4 000 synthetic high-entropy lines (~60 bytes each = ~240 KB raw,
    // deflates to ~130 KB at level 1 — comfortably over the 100 KB cap below).
    let lines = synthetic_lines(0, 4_000);
    let last_line = lines.last().cloned().expect("synthetic lines must exist");
    let lines_owned: Vec<String> = lines;
    files.insert(
        "cmdr.log".to_string(),
        PreparedFile {
            lines: lines_owned,
            mtime: now,
        },
    );

    let bytes = build_zip(&sample_manifest(), &files, now).unwrap();
    assert!(
        bytes.len() > 110_000,
        "test setup needs a bigger input zip than the cap (got {} bytes)",
        bytes.len()
    );

    let cap_bytes_value = 100 * 1024;
    let capped = cap_bundle_to_bytes(bytes.clone(), cap_bytes_value);

    // Tolerance: cap may exceed by ~10% to honor the minimum-tail floor on the newest
    // file. 1.1 × cap is the documented contract.
    let upper = cap_bytes_value * 11 / 10;
    assert!(
        capped.len() <= upper,
        "capped bundle is {} bytes, expected <= {} bytes (cap {} bytes + 10% headroom)",
        capped.len(),
        upper,
        cap_bytes_value,
    );
    assert!(
        capped.len() < bytes.len(),
        "capping should have shrunk the bundle ({} -> {})",
        bytes.len(),
        capped.len()
    );

    // Manifest is preserved verbatim.
    let entries = read_zip_entries(&capped);
    let manifest_json = entries.get("manifest.json").expect("manifest must survive capping");
    let parsed: BundleManifest = serde_json::from_str(manifest_json).unwrap();
    assert_eq!(parsed.id, "ERR-AB23X");

    // `logs/cmdr.log` is present and contains the LAST line of the input, proving we
    // trimmed from the head, not the tail.
    let cmdr_log = entries
        .get("logs/cmdr.log")
        .expect("the newest log entry must survive capping");
    assert!(
        cmdr_log.lines().any(|l| l == last_line),
        "the LAST input line must survive (got first/last few: {:?} ... {:?})",
        cmdr_log.lines().take(3).collect::<Vec<_>>(),
        cmdr_log.lines().rev().take(3).collect::<Vec<_>>(),
    );
    // ...and we dropped the FIRST line (otherwise we kept everything, which would
    // mean the cap didn't actually do anything).
    assert!(
        !cmdr_log.contains("idx=0 "),
        "expected the head to be trimmed; idx=0 still present"
    );
}

/// Newer files win the budget race over older files. Even with a tight cap, the
/// newest file gets its tail in; older files may be dropped entirely.
///
/// Like the headline test, scale-invariant: pre-fix the newest-wins logic was
/// broken at any size. Uses a 100 KB cap and ~3 000 lines per file to stay under
/// the 8 s nextest cap with headroom (was 50 000 × 2 files = 5 s warm / >8 s
/// contended).
#[test]
fn cap_bundle_prefers_newer_files() {
    let now = SystemTime::now();
    let older = now - Duration::from_secs(86_400);
    let mut files: BTreeMap<String, PreparedFile> = BTreeMap::new();
    files.insert(
        "cmdr.log".to_string(),
        PreparedFile {
            lines: synthetic_lines(0, 3_000),
            mtime: now,
        },
    );
    files.insert(
        "cmdr.log.1".to_string(),
        PreparedFile {
            lines: synthetic_lines(1, 3_000),
            mtime: older,
        },
    );
    let bytes = build_zip(&sample_manifest(), &files, now).unwrap();
    let capped = cap_bundle_to_bytes(bytes, 100 * 1024);
    let entries = read_zip_entries(&capped);

    assert!(
        entries.contains_key("logs/cmdr.log"),
        "newest file must survive (got entries: {:?})",
        entries.keys().collect::<Vec<_>>()
    );
    // Manifest always present.
    assert!(entries.contains_key("manifest.json"));
}

/// The cap is on COMPRESSED bytes, so the trim measures compressed bytes too. Budgeting the
/// raw text against it kept ~1/17 of what fits on real logs: a 1 MB cap shipped 54 KB
/// (cmdr-reports#84).
#[test]
fn cap_bundle_fills_the_cap_with_compressed_bytes_not_raw_ones() {
    let now = SystemTime::now();
    let lines: Vec<String> = (0..20_000)
        .map(|i| format!("2026-10-28T10:20:26.358+01:00 DEBUG ·cmdr_smb::volume::query  SmbVolume::get_metadata: share=\"<volume:a829a3cd8863>\" idx={i}"))
        .collect();
    let last_line = lines.last().cloned().expect("lines");
    let mut files: BTreeMap<String, PreparedFile> = BTreeMap::new();
    files.insert("cmdr.log".to_string(), PreparedFile { lines, mtime: now });
    let bytes = build_zip(&sample_manifest(), &files, now).unwrap();
    let cap_bytes = 32 * 1024;
    assert!(
        bytes.len() > cap_bytes,
        "fixture must engage the cap (got {})",
        bytes.len()
    );

    let capped = cap_bundle_to_bytes(bytes, cap_bytes);
    assert!(capped.len() <= cap_bytes, "capped bundle is {} bytes", capped.len());
    let entries = read_zip_entries(&capped);
    let body = entries.get("logs/cmdr.log").expect("log entry");
    assert!(body.lines().any(|l| l == last_line), "the newest line must survive");
    assert!(
        body.len() > cap_bytes * 3,
        "kept only {} raw bytes under a {cap_bytes}-byte cap",
        body.len()
    );
}

/// Cap stops streaming: a synthetic file forces the compressed cap before the
/// timestamp boundary; the bundle is well-formed and contains some content.
/// (The test asserts on byte ceiling and zip validity rather than line-by-line
/// content, since which lines survive depends on the deflate buffer's flush timing.)
#[test]
fn streaming_stops_at_cap() {
    let dir = make_log_dir("cap");
    let log = dir.join("cmdr.log");
    let now = Timestamp::now();
    let mut f = fs::File::create(&log).unwrap();
    // 192 KiB of deterministic high-entropy text in realistically sized log lines.
    // Poor compression crosses the test's 64 KiB compressed cap without making
    // fixture size or unusually wide regex inputs dominate what this test exercises.
    let line_count = 1_024u32;
    let mut state = 0x4d59_5df4_d0f3_3173u64;
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut token = vec![0u8; 192];
    for i in 0..line_count {
        let ts = iso(now - SignedDuration::from_secs((line_count - i) as i64));
        for byte in &mut token {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let random = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
            *byte = alphabet[(random >> 58) as usize];
        }
        write!(f, "{ts} INFO body=").unwrap();
        f.write_all(&token).unwrap();
        writeln!(f).unwrap();
    }
    drop(f);

    let cutoff = now - SignedDuration::from_hours(2);
    let cap_bytes = 64 * 1024;
    let bundle = build_bundle_streaming_to_cap(
        "ERR-CAP".to_string(),
        sample_manifest(),
        vec![log.clone()],
        cutoff,
        SystemTime::now(),
        &test_redaction(),
        cap_bytes,
    )
    .unwrap();

    // Allow one extra test-cap window for the deflater's ~64 KiB flush buffer plus
    // the central directory. With enforcement removed, the whole ~144 KiB
    // compressed fixture exceeds this ceiling.
    assert!(
        bundle.zip_bytes.len() <= cap_bytes * 2,
        "expected zip <= 2x cap; got {}",
        bundle.zip_bytes.len()
    );
    // And the bundle is a valid zip with manifest + at least one log entry.
    let entries = read_zip_entries(&bundle.zip_bytes);
    assert!(entries.contains_key("manifest.json"));
    assert!(entries.keys().any(|k| k.starts_with("logs/")));
    // Streaming must have terminated before consuming everything.
    assert!(
        (bundle.total_redacted_lines as u32) < line_count,
        "expected early termination; consumed all {} lines",
        bundle.total_redacted_lines,
    );
}

/// When the cap engages, the lines dropped are the OLDEST, never the newest: the moment
/// the report is about sits at the end of the live file. Writing a file oldest-first
/// and stopping at the cap shipped its first minutes and lost the failure itself
/// (cmdr-reports#84: the bundle jumped from 09:20 to 09:30:47, around a 09:30:30 move).
#[test]
fn streaming_cap_keeps_the_newest_lines_of_every_file_it_reaches() {
    let dir = make_log_dir("cap-newest");
    let live = dir.join("cmdr.log");
    let rotated = dir.join("cmdr.log.1");
    let now = Timestamp::now();
    // Realistic, compressible log text: what a verbose SMB session writes.
    let write_lines = |path: &std::path::Path, from_secs_ago: i64, count: i64, marker: &str| {
        let mut f = fs::File::create(path).unwrap();
        for i in 0..count {
            let ts = iso(now - SignedDuration::from_secs(from_secs_ago - i));
            writeln!(
                f,
                "{ts} INFO SmbVolume::get_metadata: share=\"<volume:a829a3cd8863>\" input=\"<dir:fe2800a71878>/<dir:{i:012}>\" {marker} idx={i}"
            )
            .unwrap();
        }
    };
    write_lines(&rotated, 3_600, 3_000, "rotated");
    write_lines(&live, 400, 300, "live");
    let mut f = fs::OpenOptions::new().append(true).open(&live).unwrap();
    writeln!(
        f,
        "{} WARN move_between_volumes: source delete failed newest-marker",
        iso(now)
    )
    .unwrap();
    drop(f);

    let cap_bytes = 16 * 1024;
    let bundle = build_bundle_streaming_to_cap(
        "ERR-NEWST".to_string(),
        sample_manifest(),
        vec![live.clone(), rotated.clone()],
        now - SignedDuration::from_hours(5),
        SystemTime::now(),
        &test_redaction(),
        cap_bytes,
    )
    .unwrap();

    // Under the cap itself, so the defensive post-pass never has to trim (it budgets far
    // more coarsely and would cut deeper).
    assert!(
        bundle.zip_bytes.len() <= cap_bytes,
        "expected zip <= cap; got {}",
        bundle.zip_bytes.len()
    );
    let entries = read_zip_entries(&bundle.zip_bytes);
    let live_body = entries.get("logs/cmdr.log").expect("live entry");
    assert!(live_body.contains("newest-marker"), "the failure line must ship");
    if let Some(rotated_body) = entries.get("logs/cmdr.log.1") {
        assert!(
            rotated_body.contains(" rotated idx=2999"),
            "a rotated file that ships at all must ship its newest line"
        );
        assert!(
            !rotated_body.contains(" rotated idx=0\n"),
            "the cap must have dropped the oldest"
        );
    }
    // The budget went to real context: far more raw text than the cap's byte count.
    let raw: usize = entries
        .iter()
        .filter(|(k, _)| k.starts_with("logs/"))
        .map(|(_, v)| v.len())
        .sum();
    assert!(
        raw > cap_bytes * 3,
        "kept only {raw} raw bytes under a {cap_bytes}-byte cap"
    );
}
