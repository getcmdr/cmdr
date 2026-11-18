//! Keeps the name and reason of an uncaught Objective-C exception for the crash report.
//!
//! An Objective-C exception that nothing catches ends in `abort()`, which our signal handler sees
//! as a bare SIGABRT. Neither our raw file nor macOS's `.ips` says WHY: macOS 27 writes only
//! `"asi": {"libsystem_c.dylib": ["abort() called"]}` (verified on macOS 27.0 with a throwing
//! `NSFilePromiseProvider`, 2026-10-13). CRASH-K35S2 shipped exactly that, and the one line that
//! would have named the bad input was lost.
//!
//! `_objc_terminate` calls the uncaught-exception handler before it aborts, so that's where we
//! read it. The handler runs on the throwing thread in an ordinary (not signal) context, so it may
//! allocate and write a file. It writes `<name>: <reason>` to a sidecar next to the raw crash
//! file, chains to whatever handler was installed before ours, and returns so the abort and the
//! SIGABRT handler run as before. The next launch folds the line into the report's
//! `panic_message`, where `prepare_for_delivery` redacts and caps it like any panic payload: the
//! reason is free text from AppKit and can quote a filename or path.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, SystemTime};

use objc2_foundation::{
    NSException, NSGetUncaughtExceptionHandler, NSSetUncaughtExceptionHandler, NSUncaughtExceptionHandler,
};

/// The sidecar, beside `crash-report.raw` in the data dir.
pub(super) const FILE_NAME: &str = "crash-report.objc-exception.txt";

/// Longest line the handler writes. AppKit's reasons are a sentence; the cap only keeps a
/// pathological one from producing a big file at the worst possible moment.
const MAX_LINE_BYTES: usize = 4_096;

/// How far the sidecar's mtime may sit from the raw crash file's and still be the same crash. The
/// two are written milliseconds apart (handler, then `abort`, then the SIGABRT handler).
const MATCH_WINDOW: Duration = Duration::from_secs(60);

static SIDECAR_PATH: OnceLock<PathBuf> = OnceLock::new();
static PREVIOUS_HANDLER: AtomicPtr<NSUncaughtExceptionHandler> = AtomicPtr::new(std::ptr::null_mut());

/// The signature `NSSetUncaughtExceptionHandler` takes (Foundation types it as `void *`).
type Handler = unsafe extern "C-unwind" fn(*mut NSException);

/// Install the handler, writing to `sidecar`. Call once, after the previous session's sidecar was
/// read: the next uncaught exception overwrites it.
pub(super) fn install(sidecar: &Path) {
    if SIDECAR_PATH.set(sidecar.to_path_buf()).is_err() {
        return;
    }
    let previous = NSGetUncaughtExceptionHandler();
    PREVIOUS_HANDLER.store(previous, Ordering::SeqCst);
    let ours: Handler = on_uncaught_exception;
    // SAFETY: Foundation stores the pointer and calls it as `void (*)(NSException *)`, which is
    // exactly `Handler`'s shape; the function is a `'static` item, so the pointer never dangles.
    unsafe { NSSetUncaughtExceptionHandler(ours as *mut NSUncaughtExceptionHandler) };
}

/// Foundation's uncaught-exception callback. Records the line, then hands the exception to the
/// handler that was installed before ours, so nothing it did is lost.
unsafe extern "C-unwind" fn on_uncaught_exception(exception: *mut NSException) {
    // A panic must not unwind out of here into `_objc_terminate`. The record is best-effort: the
    // process is about to abort either way.
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `_objc_terminate` passes the live exception object being thrown, or nil.
        if let (Some(exception), Some(sidecar)) = (unsafe { exception.as_ref() }, SIDECAR_PATH.get()) {
            record(exception, sidecar);
        }
    }));
    let previous = PREVIOUS_HANDLER.load(Ordering::SeqCst);
    if !previous.is_null() {
        // SAFETY: `previous` came from `NSGetUncaughtExceptionHandler`, so it's a handler
        // Foundation would have called with this same argument had ours not been installed.
        let previous: Handler = unsafe { std::mem::transmute::<*mut NSUncaughtExceptionHandler, Handler>(previous) };
        // SAFETY: as above; `exception` is passed through untouched.
        unsafe { previous(exception) };
    }
}

/// The `<name>: <reason>` line for `exception`, capped at [`MAX_LINE_BYTES`] on a char boundary.
fn exception_line(exception: &NSException) -> String {
    let mut line = exception.name().to_string();
    if let Some(reason) = exception.reason() {
        line.push_str(": ");
        line.push_str(&reason.to_string());
    }
    if line.len() > MAX_LINE_BYTES {
        let mut cut = MAX_LINE_BYTES;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
    }
    line
}

/// Write `exception`'s line to `sidecar`, replacing what was there.
fn record(exception: &NSException, sidecar: &Path) {
    let _ = std::fs::write(sidecar, exception_line(exception));
}

/// The previous session's exception line, if its sidecar belongs to the crash at `crash_time`.
/// A sidecar from some other moment would pin the wrong reason on this crash, which is worse
/// than none.
pub(super) fn line_near(sidecar: &Path, crash_time: SystemTime) -> Option<String> {
    let written = std::fs::metadata(sidecar).and_then(|m| m.modified()).ok()?;
    let distance = written
        .duration_since(crash_time)
        .or_else(|_| crash_time.duration_since(written))
        .unwrap_or(Duration::MAX);
    if distance > MATCH_WINDOW {
        return None;
    }
    let line = std::fs::read_to_string(sidecar).ok()?;
    (!line.trim().is_empty()).then_some(line)
}

#[cfg(test)]
#[path = "objc_exception_tests.rs"]
mod tests;
