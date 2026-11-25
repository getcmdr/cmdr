//! Windows: a child process crashes on purpose with the real filter installed, and the parent reads
//! what it left, then runs the next launch over it. The child is this test binary re-launched on
//! [`native_crash_child`], which is `#[ignore]`d so the ordinary run never crashes itself.

use std::path::{Path, PathBuf};

use super::exception_record::{self, ExceptionRecord};
use crate::crash_reporter::next_launch::process_pending_crash;
use crate::crash_reporter::{CRASH_FILE_NAME, RAW_CRASH_FILE_NAME, read_crash_report};

const CHILD_DIR_ENV: &str = "CMDR_TEST_NATIVE_CRASH_CHILD_DIR";
const CHILD_KIND_ENV: &str = "CMDR_TEST_NATIVE_CRASH_KIND";
const CHILD_TEST_PATH: &str = "crash_reporter::platform::exception_filter_tests::native_crash_child";
/// What a waiting child prints once its filter is in.
const READY_LINE: &str = "cmdr-test-native-crash-child-ready";

unsafe extern "C" {
    /// The C runtime's `abort()`: raises `SIGABRT`, then ends the process.
    fn abort() -> !;
    /// Turns off `abort()`'s message box and its Windows Error Reporting hand-off.
    fn _set_abort_behavior(flags: u32, mask: u32) -> u32;
}

/// The crashing half. Does nothing unless the parent launched it.
#[test]
#[ignore = "the child half of the native-crash tests below, which launch it"]
fn native_crash_child() {
    let Some(dir) = crate::native_text::env_path(CHILD_DIR_ENV) else {
        return;
    };
    let kind = std::env::var(CHILD_KIND_ENV).expect("the parent names the crash");

    use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode};
    // Nobody is there to close Windows Error Reporting's dialog or the CRT's abort box.
    // SAFETY: plain process-wide flag setters.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        _set_abort_behavior(0, 0x1 | 0x2);
    }
    super::install_native_crash_hooks(&super::session_raw_crash_path(&dir.join(RAW_CRASH_FILE_NAME)), &dir);

    match kind.as_str() {
        // A second instance still running: says so, then waits for the parent's word to crash.
        "wait_then_access_violation" => {
            use std::io::{BufRead, Write};
            let mut out = std::io::stdout();
            writeln!(out, "{READY_LINE}").expect("tell the parent");
            out.flush().expect("flush");
            let mut line = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut line)
                .expect("wait for the parent");
            // SAFETY: none: reading address 0x10 is the crash under test.
            let byte = unsafe { std::ptr::read_volatile(std::ptr::without_provenance::<u8>(0x10)) };
            std::hint::black_box(byte);
        }
        "access_violation" => {
            // SAFETY: none: reading address 0x10 is the crash under test.
            let byte = unsafe { std::ptr::read_volatile(std::ptr::without_provenance::<u8>(0x10)) };
            std::hint::black_box(byte);
        }
        "stack_overflow" => {
            std::hint::black_box(recurse(0));
        }
        // SAFETY: none: ending the process through the CRT's `abort()` is the crash under test.
        "abort" => unsafe { abort() },
        other => panic!("unknown crash kind {other}"),
    }
    unreachable!("the child crashed above");
}

#[inline(never)]
#[allow(unconditional_recursion, reason = "the stack overflow under test")]
fn recurse(depth: u64) -> u64 {
    let frame = std::hint::black_box([depth; 32]);
    recurse(depth + 1).wrapping_add(frame[0])
}

/// Launches the child to crash with `kind`, and returns the dir it left its record in.
fn crash_in_child(kind: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = std::env::current_exe().expect("current test binary");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            CHILD_TEST_PATH,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR_ENV, dir.path())
        .env(CHILD_KIND_ENV, kind)
        .output()
        .expect("re-launch the test binary");
    assert!(
        !output.status.success(),
        "the child must crash.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    dir
}

/// The raw crash files in `dir`: each process writes its own (`crash-report.<pid>-<ms>.raw`).
fn raw_records(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("list the dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name.starts_with("crash-report.") && name.ends_with(".raw")
        })
        .collect()
}

fn read_record(dir: &Path) -> ExceptionRecord {
    let records = raw_records(dir);
    assert_eq!(records.len(), 1, "one crashed process, one record: {records:?}");
    let bytes = std::fs::read(&records[0]).expect("the child left its record");
    exception_record::parse(&bytes).expect("the record is well-formed")
}

/// This test binary's file name, the module our own frames lie in.
fn our_module() -> String {
    let exe: PathBuf = std::env::current_exe().expect("current test binary");
    exe.file_name().expect("a file name").to_string_lossy().into_owned()
}

fn assert_names_files_only(record: &ExceptionRecord) {
    assert!(!record.modules.is_empty(), "the walk named the images it passed");
    for (_, name) in &record.modules {
        assert!(
            !name.contains('\\') && !name.contains('/'),
            "a file name, never a path: {name}"
        );
    }
}

/// The next launch over the child's leftovers: the report it offers, as JSON, with nothing in it
/// that names this machine's folders or its user.
fn assemble(dir: &Path) -> (crate::crash_reporter::CrashReport, String) {
    let json_path = dir.join(CRASH_FILE_NAME);
    process_pending_crash(&json_path, &dir.join(RAW_CRASH_FILE_NAME));
    assert!(raw_records(dir).is_empty(), "the record is consumed");
    let report = read_crash_report(&json_path).expect("the next launch wrote a report to offer");
    let json = std::fs::read_to_string(&json_path).expect("read the report");
    // Each string as the report holds it, not as JSON spells it: JSON doubles every backslash, so a
    // search of the raw text for a Windows path could never match.
    let value: serde_json::Value = serde_json::from_str(&json).expect("the report is JSON");
    let mut strings = Vec::new();
    collect_strings(&value, &mut strings);
    let dir_text = dir.display().to_string().to_lowercase();
    let user = std::env::var("USERNAME")
        .ok()
        .filter(|user| user.len() >= 3)
        .map(|user| user.to_lowercase());
    for text in strings {
        let text = text.to_lowercase();
        assert!(!text.contains(&dir_text), "no data-dir path: {text}");
        assert!(
            !text.contains(r"\device\") && !text.contains(r"\users\"),
            "no path: {text}"
        );
        if let Some(user) = &user {
            assert!(!text.contains(user.as_str()), "no user name: {text}");
        }
    }
    (report, json)
}

fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        serde_json::Value::Object(fields) => fields.values().for_each(|field| collect_strings(field, out)),
        _ => {}
    }
}

/// The guard test for the scan itself: a path planted in a report is found.
#[test]
fn the_report_scan_finds_a_windows_path() {
    let value = serde_json::json!({ "frames": [r"C:\Users\test_user_1\x.dll+0x10"] });
    let mut strings = Vec::new();
    collect_strings(&value, &mut strings);
    assert!(strings.iter().any(|text| text.to_lowercase().contains(r"\users\")));
}

/// Two instances, one data dir: a live one's record is never touched by the other's next launch,
/// and once it crashes, the next launch after that offers it.
#[test]
fn a_second_instance_leaves_a_live_instances_record_alone() {
    use std::io::{BufRead, Write};

    let dir = tempfile::tempdir().expect("tempdir");
    let exe = std::env::current_exe().expect("current test binary");
    let mut child = std::process::Command::new(exe)
        .args([
            "--exact",
            CHILD_TEST_PATH,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR_ENV, dir.path())
        .env(CHILD_KIND_ENV, "wait_then_access_violation")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("launch the first instance");
    let mut stdout = std::io::BufReader::new(child.stdout.take().expect("piped"));
    let mut line = String::new();
    while !line.contains(READY_LINE) {
        line.clear();
        assert!(
            stdout.read_line(&mut line).expect("read the child") > 0,
            "the child ended before it was ready"
        );
    }

    // The second instance's next-launch pass, while the first still runs.
    let json_path = dir.path().join(CRASH_FILE_NAME);
    process_pending_crash(&json_path, &dir.path().join(RAW_CRASH_FILE_NAME));
    assert_eq!(raw_records(dir.path()).len(), 1, "the live instance's record stays");
    assert!(!json_path.exists(), "and nothing is offered for it");

    let mut stdin = child.stdin.take().expect("piped");
    writeln!(stdin, "crash").expect("tell the child");
    drop(stdin);
    assert!(
        !child.wait().expect("the child ends").success(),
        "the first instance crashed"
    );

    let (report, _) = assemble(dir.path());
    assert_eq!(report.signal.as_deref(), Some("EXCEPTION_ACCESS_VIOLATION"));
}

#[test]
fn an_access_violation_leaves_a_record_the_next_launch_offers() {
    let dir = crash_in_child("access_violation");
    let record = read_record(dir.path());
    assert_eq!(record.signal(), "EXCEPTION_ACCESS_VIOLATION");
    assert_eq!(record.header.params[..2], [0, 0x10], "a read of address 0x10");
    assert_names_files_only(&record);
    let frames = record.module_frames();
    assert!(
        frames[0].starts_with(&format!("{}+0x", our_module())),
        "the walk starts at the fault, in our code: {frames:?}"
    );

    let (report, _) = assemble(dir.path());
    assert_eq!(report.os, "windows");
    assert_eq!(report.signal.as_deref(), Some("EXCEPTION_ACCESS_VIOLATION"));
    assert_eq!(report.app_fate, crate::crash_reporter::AppFate::Ended);
    assert!(
        report
            .os_exception
            .as_deref()
            .is_some_and(|line| line.contains("reading 0x0000000000000010")),
        "{:?}",
        report.os_exception
    );
    assert_eq!(report.os_frames, frames, "the frames ship as the record named them");
    assert!(report.image_base.is_some());
}

#[test]
fn a_stack_overflow_leaves_a_record_of_the_recursion() {
    let dir = crash_in_child("stack_overflow");
    let record = read_record(dir.path());
    assert_eq!(record.signal(), "EXCEPTION_STACK_OVERFLOW");
    assert_names_files_only(&record);
    let ours = format!("{}+0x", our_module());
    let in_our_code = record
        .module_frames()
        .iter()
        .filter(|frame| frame.starts_with(&ours))
        .count();
    assert!(
        in_our_code >= 10,
        "the recursion fills the walk: {:?}",
        record.module_frames()
    );

    let (report, _) = assemble(dir.path());
    assert_eq!(report.signal.as_deref(), Some("EXCEPTION_STACK_OVERFLOW"));
}

#[test]
fn an_abort_leaves_a_record_through_the_runtimes_handler() {
    let dir = crash_in_child("abort");
    let record = read_record(dir.path());
    assert_eq!(record.signal(), "SIGABRT");
    assert_names_files_only(&record);
    let ours = format!("{}+0x", our_module());
    assert!(
        record.module_frames().iter().any(|frame| frame.starts_with(&ours)),
        "the walk reaches the code that called abort(): {:?}",
        record.module_frames()
    );

    let (report, _) = assemble(dir.path());
    assert_eq!(report.signal.as_deref(), Some("SIGABRT"));
    assert_eq!(report.os_exception.as_deref(), Some("abort() called"));
}

/// The panic hook's fallback walk, in a normal process: it names our own module.
#[test]
fn the_module_walk_names_our_own_frames() {
    let frames = super::exception_filter::module_frames();
    let ours = format!("{}+0x", our_module());
    assert!(frames.iter().any(|frame| frame.starts_with(&ours)), "{frames:?}");
}
