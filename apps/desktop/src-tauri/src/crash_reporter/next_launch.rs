//! Reading the previous session's crash evidence and assembling a report from it.
//!
//! The counterpart to the two capture paths in `mod.rs` and `signal_handler.rs`, and the split is
//! the one the rest of this module keeps making: capture runs in a COMPROMISED context (a panic
//! hook that must not panic, a signal handler that must stay async-signal-safe), while everything
//! here runs at the next launch in an ordinary process with the full stdlib.
//!
//! That's why every field a crash report can't gather at crash time is attached here: the system
//! snapshot, the diagnostics id, macOS's own crash report, and the app's settled fate. When adding
//! a field, the question is which side of that line it belongs on, and anything that reads the
//! filesystem or allocates belongs on this one.

use crate::timestamps;
use jiff::Timestamp;
use std::path::Path;

use super::{AppFate, CRASH_LOOP_THRESHOLD_SECS, CrashReport, read_crash_report, write_crash_report};

/// Turn whatever the previous session left behind into a report the frontend can offer.
///
/// Three outcomes: a JSON report from the panic hook is topped up with the fields the hook couldn't
/// gather and left in place; a raw signal-crash file is read and converted to one; and a report
/// already delivered in-session is deleted rather than offered twice.
pub(super) fn process_pending_crash(crash_json_path: &Path, raw_crash_path: &Path) {
    process_evidence(crash_json_path, raw_crash_path);
    // Read (or found to belong to no crash) by now, so it can't attach itself to a later one.
    #[cfg(target_os = "macos")]
    let _ = std::fs::remove_file(objc_exception_path(raw_crash_path));
}

fn process_evidence(crash_json_path: &Path, raw_crash_path: &Path) {
    super::pending_delivery::restore_unfinished_sends(crash_json_path);
    let records = raw_crash_records(raw_crash_path);
    let slot_taken = match read_crash_report(crash_json_path) {
        Some(mut report) => {
            finish_panic_report(crash_json_path, &mut report);
            // `finish_panic_report` drops a report already delivered in-session, which frees the slot.
            crash_json_path.exists()
        }
        None => false,
    };

    // One pending slot: the oldest record fills it if it's free, and every later one waits for a
    // later launch rather than being lost under the report already pending.
    let mut slot_taken = slot_taken;
    for record in records {
        if std::fs::metadata(&record).map_or(true, |m| m.len() == 0) {
            // A clean session's empty file, or one that's gone already.
            let _ = std::fs::remove_file(&record);
        } else if slot_taken {
            keep_for_a_later_launch(&record, raw_crash_path);
        } else {
            super::platform::convert_raw_crash(crash_json_path, &record);
            let _ = std::fs::remove_file(&record);
            // An unreadable record wrote nothing, so the next one may still take the slot.
            slot_taken = crash_json_path.exists();
        }
    }
}

/// Every raw crash file beside `raw_crash_path` that no running process still holds, oldest first:
/// the fixed `crash-report.raw`, a Windows per-process `crash-report.<pid>-<ms>.raw`, and a record
/// an earlier launch kept aside (`crash-report.kept-<ms>.raw`).
fn raw_crash_records(raw_crash_path: &Path) -> Vec<std::path::PathBuf> {
    let Some(dir) = raw_crash_path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut records: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("crash-report.") && name.ends_with(".raw")
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && !super::platform::raw_crash_file_in_use(path))
        .map(|path| (crash_time_of(&path), path))
        .collect();
    records.sort();
    records.into_iter().map(|(_, path)| path).collect()
}

/// Moves a record off the fixed name, which this session's capture is about to truncate, so it
/// survives until a launch with a free slot. A record under any other name is already safe.
fn keep_for_a_later_launch(record: &Path, raw_crash_path: &Path) {
    if record != raw_crash_path {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let kept = raw_crash_path.with_file_name(format!("crash-report.kept-{stamp}.raw"));
    match std::fs::rename(record, &kept) {
        Ok(()) => log::info!(
            "Crash reporter: a crash report is still pending, so a newer native crash waits for a later launch"
        ),
        Err(e) => log::warn!("Crash reporter: couldn't keep a newer native crash for a later launch: {e}"),
    }
}

/// Top up the panic hook's report with what it couldn't know, rewriting only if something changed.
fn finish_panic_report(crash_json_path: &Path, report: &mut CrashReport) {
    // Already delivered in-session, so the user has heard about this panic once. Saying it again on
    // the next launch spends trust and adds nothing. Dropped here rather than hidden in the
    // frontend, so a report nobody will be offered can't linger on disk.
    if report.reported_in_session {
        log::info!("Crash reporter: pending report already went out in-session, discarding");
        let _ = std::fs::remove_file(crash_json_path);
        return;
    }

    if is_crash_loop(&report.timestamp) {
        report.possible_crash_loop = true;
    }
    // The panic hook can't know the app's fate, so it writes `Unconfirmed` and `survival.rs`
    // upgrades it from a thread that can only run while the process is alive. Reaching a LATER
    // LAUNCH still unconfirmed is therefore the proof that no such upgrade happened: the app went
    // down with the panic. Resolving it here, at the one moment the evidence is conclusive, is what
    // lets the dialog pick its opening sentence from a settled value.
    if report.app_fate == AppFate::Unconfirmed {
        report.app_fate = AppFate::Ended;
    }
    // The panic hook couldn't gather the snapshot (compromised context), so attach the stable form
    // now. Only when missing, so we don't rewrite on every launch the report lingers.
    if report.system_snapshot.is_none()
        && let Some(dir) = crash_json_path.parent()
    {
        report.system_snapshot = Some(crate::diagnostics_snapshot::SystemSnapshot::collect_stable(dir));
    }
    // A panic that UNWINDS leaves macOS nothing to report, so this usually misses and costs one
    // directory scan. It hits for the aborting kind ("panic in a function that cannot unwind"),
    // which is exactly the class where our backtrace stops at the tao/wry event loop and the
    // symbolicated stack says what the app was actually doing.
    if report.os_exception.is_none()
        && report.os_frames.is_empty()
        && let Some(crash_time) = parse_timestamp(&report.timestamp)
    {
        attach_os_crash_report(report, crash_time);
    }
    report.prepare_for_delivery();
    if let Err(e) = write_crash_report(crash_json_path, report) {
        log::warn!("Crash reporter: couldn't update pending crash report: {e}");
    }
}

/// Read the async-signal-safe handler's raw file and write a real report in its place.
#[cfg(unix)]
pub(super) fn convert_raw_signal_crash(crash_json_path: &Path, raw_crash_path: &Path) {
    use super::{signal_handler, symbolicate};

    // The raw file is written BY THE HANDLER, mid-crash, so its mtime is when the app actually
    // died. Read it before anything else touches the file. Everything downstream wants that moment
    // rather than now: it's what matches macOS's own report to this crash, and what a later log
    // bundle scopes itself around.
    let crash_time = crash_time_of(raw_crash_path);

    let Some((signal, addresses, image_base, crash_app_version)) = signal_handler::read_raw_crash(raw_crash_path)
    else {
        return;
    };

    let current_version = env!("CARGO_PKG_VERSION");
    let backtrace_frames = if crash_app_version == current_version {
        symbolicate::symbolicate_addresses(&addresses)
    } else {
        log::info!(
            "Crash reporter: version mismatch (crash={crash_app_version}, \
             current={current_version}), sending raw addresses"
        );
        addresses.iter().map(|a| format!("0x{a:016x}")).collect()
    };

    let mut report = native_crash_report(crash_json_path, crash_time, signal_name(signal), crash_app_version);
    // A signal carries no payload, but an uncaught Objective-C exception's SIGABRT leaves its
    // `<name>: <reason>` behind (`objc_exception.rs`). Redacted and capped with the rest by
    // `prepare_for_delivery` below.
    report.panic_message = objc_exception_line(raw_crash_path, crash_time);
    report.backtrace_frames = backtrace_frames;
    // The base recorded BY THE CRASHED PROCESS, never this one's: ASLR gives the relaunched
    // process a different slide, which would make every offset wrong. `0` means that build
    // couldn't resolve it (non-macOS Unix).
    report.image_base = (image_base != 0).then(|| format!("0x{image_base:x}"));

    // The payoff for the signal path. `backtrace_frames` above are bare addresses; this is the same
    // stack with names on it, system frames included.
    // The report is written unconditionally on the next line either way.
    attach_os_crash_report(&mut report, crash_time);

    report.prepare_for_delivery();

    if let Err(e) = write_crash_report(crash_json_path, &report) {
        log::warn!("Crash reporter: couldn't write symbolicated crash report: {e}");
    }
}

/// Read the Windows exception filter's record (`exception_filter.rs`) and write a real report in
/// its place. Compiled on every host for its tests; only Windows writes the record.
#[cfg(any(windows, test))]
pub(super) fn convert_raw_exception_crash(crash_json_path: &Path, raw_crash_path: &Path) {
    // Read first, for the same reason as on the signal path: the mtime is when the app died.
    let crash_time = crash_time_of(raw_crash_path);
    let Ok(data) = std::fs::read(raw_crash_path) else {
        return;
    };
    let Some(record) = super::platform::exception_record::parse(&data) else {
        // Every session leaves an empty file (the filter opens it at install), so only a
        // non-empty one that doesn't parse is worth a line.
        if !data.is_empty() {
            log::info!("Crash reporter: raw exception record unreadable, discarding");
        }
        return;
    };

    let mut report = native_crash_report(crash_json_path, crash_time, record.signal(), record.app_version.clone());
    report.backtrace_frames = record.raw_frames();
    // Recorded by the crashed process, like the signal path's. The frames below don't need it
    // (each is relative to its own module), but it keeps the field meaning one thing everywhere.
    let image_base = record.header.image_base;
    report.image_base = (image_base != 0).then(|| format!("0x{image_base:x}"));
    report.uptime_secs = record.header.uptime_ms as f64 / 1000.0;
    // Windows' own exception record plays the part macOS's `.ips` plays: the reason in one line,
    // and the faulting thread's stack with a name on every frame an image holds.
    report.os_exception = Some(record.exception_line());
    report.os_frames = record.module_frames();

    report.prepare_for_delivery();

    if let Err(e) = write_crash_report(crash_json_path, &report) {
        log::warn!("Crash reporter: couldn't write the native crash report: {e}");
    }
}

/// The raw file's mtime: the moment the handler wrote it, so the moment the app died. `now` only as
/// a fallback, which keeps the timestamp populated on a filesystem that won't answer.
fn crash_time_of(raw_crash_path: &Path) -> std::time::SystemTime {
    std::fs::metadata(raw_crash_path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now())
}

/// What every native crash report holds before its OS's evidence goes on: the fields the next
/// launch knows for any of them. The caller adds the stack and whatever the capture recorded.
fn native_crash_report(
    crash_json_path: &Path,
    crash_time: std::time::SystemTime,
    signal: String,
    app_version: String,
) -> CrashReport {
    use super::{CACHED_SETTINGS, CRASH_FILE_VERSION, CRASH_SHORT_ID_PREFIX, current_build_mode};

    // When the app DIED, taken from the raw file the handler wrote, never the moment this next
    // launch got round to assembling the report. The gap between the two is however long the
    // machine sat closed, and everything that reads this field (the macOS-report match, the
    // crash-loop check, a log bundle's window) wants the crash.
    let timestamp = to_iso8601(crash_time);
    // A native crash within seconds of this launch is the app dying on start, again: the frontend
    // asks rather than auto-sending a report per relaunch, as it already did for a panic.
    let possible_crash_loop = is_crash_loop(&timestamp);
    CrashReport {
        version: CRASH_FILE_VERSION,
        timestamp,
        signal: Some(signal),
        panic_message: None,
        backtrace_frames: Vec::new(),
        thread_name: None,
        thread_count: 0,
        app_version,
        os_version: crate::platform::os_version(),
        os: crate::platform::os_name(),
        arch: std::env::consts::ARCH.to_string(),
        uptime_secs: 0.0, // Unknown unless the capture recorded it
        active_settings: CACHED_SETTINGS.get().cloned().unwrap_or_default(),
        possible_crash_loop,
        // No ambiguity on this path: a native crash is unrecoverable, the handler hands it on to
        // the OS, and we're reading the evidence from the NEXT launch. The app is definitively gone.
        app_fate: AppFate::Ended,
        // A native crash is never delivered in-session: the process died in the handler.
        reported_in_session: false,
        build_mode: Some(current_build_mode().to_string()),
        short_id: Some(crate::short_id::generate(CRASH_SHORT_ID_PREFIX)),
        // The handler couldn't touch the diag id (no alloc/lock). We attach it HERE, where the full
        // stdlib is available. `email` stays `None` (send-time field, set by the dialog).
        diag_id: crate::install_id::diagnostics_id(),
        email: None,
        // The data dir is the crash file's parent; the snapshot reads only index sizes and capacity.
        system_snapshot: crash_json_path
            .parent()
            .map(crate::diagnostics_snapshot::SystemSnapshot::collect_stable),
        image_base: None,
        os_exception: None,
        os_frames: Vec::new(),
    }
}

/// The uncaught-exception sidecar sits beside the raw crash file in the data dir.
#[cfg(target_os = "macos")]
fn objc_exception_path(raw_crash_path: &Path) -> std::path::PathBuf {
    raw_crash_path.with_file_name(super::objc_exception::FILE_NAME)
}

#[cfg(target_os = "macos")]
fn objc_exception_line(raw_crash_path: &Path, crash_time: std::time::SystemTime) -> Option<String> {
    let line = super::objc_exception::line_near(&objc_exception_path(raw_crash_path), crash_time)?;
    Some(format!("Uncaught Objective-C exception {line}"))
}

/// Only macOS has Objective-C exceptions to record.
#[cfg(all(unix, not(target_os = "macos")))]
fn objc_exception_line(_raw_crash_path: &Path, _crash_time: std::time::SystemTime) -> Option<String> {
    None
}

/// Attach macOS's own view of this crash, if it wrote one.
///
/// Best-effort by design: a miss is normal (the report may not be written yet when someone
/// relaunches quickly, the directory may be unreadable, or the crash may be a panic that unwound
/// and produced no OS report at all), and the crash report is still worth sending without it.
#[cfg(target_os = "macos")]
fn attach_os_crash_report(report: &mut CrashReport, crash_time: std::time::SystemTime) {
    let Some(process_name) = current_process_name() else {
        return;
    };
    let Some(extracted) = super::os_crash_report::extract_near(&process_name, crash_time) else {
        log::debug!("Crash reporter: no matching macOS crash report for this crash");
        return;
    };
    log::info!(
        "Crash reporter: attached macOS crash report ({} symbolicated frames)",
        extracted.frames.len()
    );
    report.os_exception = extracted.exception;
    report.os_frames = extracted.frames;
}

/// `.ips` files are macOS's, so everywhere else there is nothing to attach and the report ships
/// with its own backtrace, exactly as it did before any of this existed. ❗ The whole
/// `os_crash_report` module is gated too: leaving its parser compiled-but-unreachable here is what
/// made the Linux build fail on dead code.
#[cfg(not(target_os = "macos"))]
fn attach_os_crash_report(_report: &mut CrashReport, _crash_time: std::time::SystemTime) {}

/// The name macOS files our crash reports under: the executable's own file stem (`Cmdr` in a
/// shipped build). Taken from `current_exe` rather than hardcoded so a dev build, whose binary is
/// named differently, still finds its own reports.
#[cfg(target_os = "macos")]
fn current_process_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// A [`SystemTime`](std::time::SystemTime) as the same ISO 8601 string the capture paths write, so
/// every crash-file timestamp has one shape whatever produced it. A time no timestamp can hold
/// (past year 9999, which a local file's mtime never reaches) reads as now, like an mtime the
/// filesystem won't give.
pub(super) fn to_iso8601(time: std::time::SystemTime) -> String {
    Timestamp::try_from(time).map_or_else(|_| timestamps::now_rfc3339(), timestamps::rfc3339)
}

/// The inverse, for a timestamp already on disk. `None` for a value that isn't ISO 8601, which
/// callers treat as "can't place this crash in time" rather than as an error.
#[expect(clippy::unnecessary_fallible_conversions, reason = "fallible from jiff 1.0")]
pub(super) fn parse_timestamp(timestamp: &str) -> Option<std::time::SystemTime> {
    std::time::SystemTime::try_from(timestamps::parse_rfc3339(timestamp)?).ok()
}

#[cfg(unix)]
fn signal_name(sig: i32) -> String {
    match sig {
        libc::SIGSEGV => "SIGSEGV".to_string(),
        libc::SIGBUS => "SIGBUS".to_string(),
        libc::SIGABRT => "SIGABRT".to_string(),
        other => format!("signal {other}"),
    }
}

/// Whether the crash timestamp says this launch is following one that died within seconds, which
/// makes the frontend ask instead of auto-sending. A malformed timestamp answers `false`: claiming
/// a crash loop we can't prove would suppress a legitimate auto-send.
pub(super) fn is_crash_loop(crash_timestamp: &str) -> bool {
    let Some(crash_time) = timestamps::parse_rfc3339(crash_timestamp) else {
        return false;
    };
    // Whole seconds, truncated toward zero; a crash stamped in the future is negative.
    let elapsed_secs = Timestamp::now().duration_since(crash_time).as_secs();
    u64::try_from(elapsed_secs).is_ok_and(|secs| secs < CRASH_LOOP_THRESHOLD_SECS)
}
