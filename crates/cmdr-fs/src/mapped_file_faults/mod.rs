//! Surviving a read of a memory-mapped file whose disk went away.
//!
//! `gix` maps a repository's pack files, and Apple's ImageIO, QuickLook, and PDFKit map the
//! files they read. When the volume under such a mapping goes away (a pulled USB cable, a forced
//! eject, a network volume dropping), the next read of a page that isn't resident raises SIGBUS,
//! and SIGBUS kills the process. `recover` is what a SIGBUS handler calls first: when the fault
//! is a READ of a read-only, file-backed page that belongs to no loaded image, it maps anonymous
//! zeros over that one page, moves [`generation`], and the faulting load re-executes and reads
//! zeros. Every other fault is left alone, for the crash reporter.
//!
//! **Why zeros are sound.** Nothing unwinds and no Rust frame is skipped: the instruction that
//! faulted runs again and sees valid memory. A decoder reading zeros sees corrupt data and says
//! so through its own error path. What a caller must do is distrust any answer computed while
//! [`generation`] moved, which is the caller's job (`crates/cmdr-git/src/fault_guard.rs`), since
//! only it knows what an answer is.
//!
//! **What is never recovered**, each a fault where zeros would be worse than the crash:
//!
//! - A page of a loaded image (our binary, a dylib, a framework, the dyld shared cache): a code
//!   page of zeros executes as garbage. macOS keeps the image ranges in a table filled from
//!   dyld's add-image callback at [`prepare`] time; Linux recognizes an image by the same file
//!   also being mapped executable.
//! - A writable mapping: a write landing in an anonymous page would look committed and never
//!   reach the file, which is silent data loss (SQLite's `-shm` is mapped this way).
//! - An executable or anonymous page, and any address no file-backed region covers.
//!
//! **Async-signal-safe.** `recover` allocates nothing, takes no lock, and formats nothing. It
//! makes three kinds of call: atomic loads, `mmap` (a raw syscall), and, on macOS,
//! `mach_vm_region_recurse` (a Mach trap through the thread's MIG reply port, the call crash
//! reporters make from their own handlers) or, on Linux, `open` / `read` / `close` of
//! `/proc/self/maps`, all three on POSIX's async-signal-safe list.
//!
//! The why and the evidence: `DETAILS.md` § "Mapped-file faults".

use std::sync::atomic::{AtomicU64, Ordering};

mod image_ranges;
mod platform;
mod proc_maps;

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
mod real_image;

/// The SIGBUS entry point a Unix signal handler calls first (`platform/unix.rs`). Windows
/// recovers nothing yet (`platform/windows.rs`).
#[cfg(unix)]
pub use platform::recover;

/// How many pages `recover` has mapped zeros over since the process started.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// How many mapped-file pages have been recovered since the process started.
///
/// Read it before and after an operation that reads through a mapping: if it moved, some page
/// read in between may have been zeros, so the answer can't be trusted. The counter is
/// process-wide, because the fault can land on a thread the operation doesn't own (`gix` runs its
/// status walk on threads it spawns itself).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// Readies `recover`: reads the page size and, on macOS, starts tracking every loaded image's
/// address range. Idempotent. Call it in normal context, before installing a handler that calls
/// `recover`; until it ran, `recover` recovers nothing. A no-op where nothing recovers (Windows).
pub fn prepare() {
    platform::prepare();
}

/// Moves [`generation`] as one recovered page would, so a consumer's cell can drive what follows a
/// recovery without a disk image. ❌ Never called outside a test.
#[cfg(any(test, feature = "testing"))]
pub fn simulate_recovery() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// A fault handler for a test process that has no crash reporter: on Unix it calls `recover` and,
/// for a fault it can't own, restores the default action and returns, so the load re-executes and
/// the process dies of the signal as it would have. Installs nothing on Windows.
#[cfg(any(test, feature = "testing"))]
pub fn install_test_handler() {
    platform::install_test_handler();
}
