//! The OS half of the test fixtures: links, privilege, and special files, so a test asks
//! for a capability (`can_create_symlinks()`, `is_root()`) instead of naming an OS.

use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(&'static Path, &'static Path) -> io::Result<()> = symlink::<&'static Path, &'static Path>;
const _: fn() -> bool = is_root;
const _: fn() -> bool = can_create_symlinks;
const _: fn(&Path) -> io::Result<std::path::PathBuf> = long_path;
const _: fn() -> std::path::PathBuf = scratch_root;

/// Creates a symlink at `link` pointing at `target`, which may be relative (to `link`'s
/// folder) and may not exist. Windows keeps file and folder links apart, so there the
/// kind follows the target, and a dangling one gets a file link.
pub use imp::symlink;

/// Whether the test runs with superuser rights, which bypass the permission refusals a
/// test means to provoke, so such a test skips itself.
pub use imp::is_root;

/// Whether this process may create symlinks: always on Unix; on Windows only with
/// Developer Mode or an elevated process, so a link test skips itself without them.
pub use imp::can_create_symlinks;

/// The line a cell that skipped itself prints, which the Windows test baseline tool
/// (`scripts/windows-test-baseline`) counts: nextest has no runtime skip, so a cell
/// that returns early reads as a pass, and only this line tells the two apart.
pub const SKIP_MARKER: &str = "skipped: ";

/// `true`, after printing [`SKIP_MARKER`] and why, when this process can't create
/// symlinks, so a link cell returns at once: `if skip_without_symlinks() { return; }`.
///
/// ❗ Use it over a bare `can_create_symlinks()` check, so a box without the right
/// (Windows without Developer Mode or elevation) shows which cells it didn't run.
#[allow(
    clippy::print_stderr,
    reason = "a skip notice belongs in the test runner's captured output, which is where nextest attributes it to the cell; this module is `testing`-feature-gated and never in a shipped build"
)]
pub fn skip_without_symlinks() -> bool {
    if can_create_symlinks() {
        return false;
    }
    eprintln!("{SKIP_MARKER}this cell creates symlinks, which needs Developer Mode or an elevated shell on Windows");
    true
}

/// `path` with every 8.3 short name spelled long, the spelling a listing shows. Windows
/// only has short names (`GetLongPathNameW`); elsewhere it's `path` as is.
pub use imp::long_path;

/// The folder every test's scratch space goes in: the OS temp folder, with every 8.3 short
/// name spelled long on Windows ([`long_path`]).
///
/// Why: a Windows `TEMP` under an account name past eight characters reads
/// `C:\Users\RUNNER~1\…` (the hosted CI runner's does), while a Windows volume spells every
/// path it answers long, so a test comparing a path it built from `TEMP` with one a volume
/// reported fails on the spelling alone, hiding whatever it means to check. A cell that
/// pins how the PRODUCT reads a short name types one on purpose (the Go to path cells,
/// #1337).
///
/// `TestDir` creates under it, and on Windows a load-time hook points `tempfile`'s default
/// folder at it too, so a bare `tempfile::tempdir()` in a test binary that links this
/// module (any test build with the `testing` feature) lands there. `std::env::temp_dir()`
/// itself keeps the OS's spelling.
pub use imp::scratch_root;

/// A named pipe (FIFO) at `path`, the special file a walk must neither read nor copy.
/// Unix-only: Windows has no FIFO on disk.
#[cfg(unix)]
pub use unix::make_fifo;
