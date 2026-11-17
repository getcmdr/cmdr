//! Only our own crates' DEBUG and TRACE reach a report, in both ZIP pipelines. The verdict is
//! the provenance tag the file chain writes (`logging/provenance.rs`); continuation lines
//! follow their header, and anything without a parseable header drops.

use super::{read_zip_entries, sample_manifest, test_redaction};
use crate::error_reporter::BundleScope;
use crate::error_reporter::bundle_builder::{build_bundle_streaming, load_and_filter_log_file};
use crate::logging::dispatch::file_timestamp_at as timestamp;
use crate::test_support::TestDir;
use jiff::{SignedDuration, Timestamp};
use std::time::SystemTime;

/// One log file with every provenance shape, written a minute ago.
fn mixed_log(now: Timestamp) -> String {
    let t = timestamp(now - SignedDuration::from_mins(1));
    [
        "orphan continuation before any header".to_string(),
        format!("{t} DEBUG ·copy  ours-short-target"),
        format!("{t} DEBUG copy  foreign-same-target"),
        format!("{t} DEBUG mdns_sd::service_daemon  foreign multi-line record"),
        "foreign continuation line".to_string(),
        format!(
            "{t} DEBUG smb2::client::tree  tree: exclusive delete of Photos/2026/Summer (Lake House)/x.jpg refused"
        ),
        format!("{t} DEBUG ·FE:viewer  ours-fe"),
        format!("{t} DEBUG indexing::writer  pre-upgrade-debug"),
        format!("{t} WARN  copy  pre-upgrade-warn"),
        format!(
            "{t} INFO  smb2::client::durable  durable: Photos/Summer (Lake House)/x.jpg reclaimed on the new session"
        ),
        format!("{t} ERROR ·copy  ours-error"),
        "ours continuation".to_string(),
        format!("{t} NEW unparseable header"),
        "unparseable continuation".to_string(),
    ]
    .join("\n")
        + "\n"
}

fn assert_only_shippable_lines(body: &str) {
    for kept in [
        "ours-short-target",
        "ours-fe",
        "pre-upgrade-warn",
        "ours-error",
        "ours continuation",
    ] {
        assert!(body.contains(kept), "{kept:?} was dropped:\n{body}");
    }
    for dropped in [
        "orphan continuation",
        "foreign-same-target",
        "foreign multi-line",
        "foreign continuation",
        "exclusive delete",
        "Lake House",
        "pre-upgrade-debug",
        "durable",
        "unparseable",
    ] {
        assert!(!body.contains(dropped), "{dropped:?} shipped:\n{body}");
    }
}

#[test]
fn the_streaming_pipeline_ships_only_our_debug() {
    let dir = TestDir::new("provenance-streaming");
    let log = dir.join("cmdr.log");
    let now = Timestamp::now();
    std::fs::write(&log, mixed_log(now)).expect("write log");

    let bundle = build_bundle_streaming(
        "ERR-PROV1".to_string(),
        sample_manifest(),
        vec![log],
        now - SignedDuration::from_hours(1),
        SystemTime::now(),
        &test_redaction(),
    )
    .expect("build_bundle_streaming");

    let entries = read_zip_entries(&bundle.zip_bytes);
    assert_only_shippable_lines(entries.get("logs/cmdr.log").expect("log entry"));
    assert_eq!(bundle.total_redacted_lines, 5, "the count covers shipped lines only");
}

/// `smb2` keys and quotes every path it logs, so its lines carry our tag and ship, DEBUG
/// included, each path tokenized. An untagged `durable` record comes from a build whose `smb2`
/// printed the path in prose, so it still stays out.
#[test]
fn tagged_smb2_lines_ship_with_their_paths_redacted() {
    let dir = TestDir::new("provenance-smb2");
    let log = dir.join("cmdr.log");
    let now = Timestamp::now();
    let t = timestamp(now - SignedDuration::from_mins(1));
    let lines = [
        format!(
            "{t} DEBUG ·smb2::client::tree  tree: exclusive delete refused (Some(SHARING_VIOLATION)), deleting shared path=\"Photos/2026/Summer (Lake House)/x.jpg.cmdr-tmp-0f6c2a4e-1b7d-4c55-9a43-6d0e8f1b2c3d\""
        ),
        format!(
            "{t} INFO  ·smb2::client::durable  durable: reclaimed on the new session; the transfer resumes rather than restarting path=\"Photos/Summer (Lake House)/y.jpg\""
        ),
        format!(
            "{t} INFO  smb2::client::durable  durable: Photos/Old Trip (Pre-upgrade)/z.jpg reclaimed on the new session"
        ),
    ];
    std::fs::write(&log, lines.join("\n") + "\n").expect("write log");

    let bundle = build_bundle_streaming(
        "ERR-PROV2".to_string(),
        sample_manifest(),
        vec![log],
        now - SignedDuration::from_hours(1),
        SystemTime::now(),
        &test_redaction(),
    )
    .expect("build_bundle_streaming");

    let entries = read_zip_entries(&bundle.zip_bytes);
    let body = entries.get("logs/cmdr.log").expect("log entry");
    for kept in [
        "tree: exclusive delete refused (Some(SHARING_VIOLATION)), deleting shared path=",
        "durable: reclaimed on the new session; the transfer resumes rather than restarting path=",
    ] {
        assert!(body.contains(kept), "{kept:?} was dropped:\n{body}");
    }
    for leaked in [
        "Summer",
        "Lake House",
        "x.jpg",
        "y.jpg",
        "Old Trip",
        "Pre-upgrade",
        "z.jpg",
    ] {
        assert!(!body.contains(leaked), "{leaked:?} shipped:\n{body}");
    }
}

#[test]
fn the_window_pipeline_ships_only_our_debug() {
    let dir = TestDir::new("provenance-window");
    let log = dir.join("cmdr.log");
    let now = Timestamp::now();
    std::fs::write(&log, mixed_log(now)).expect("write log");

    let (lines, _) = load_and_filter_log_file(
        &log,
        BundleScope::Window { first_error_at: now },
        now,
        SystemTime::now(),
        &test_redaction(),
    )
    .expect("file should be loaded");

    assert_only_shippable_lines(&lines.join("\n"));
}
