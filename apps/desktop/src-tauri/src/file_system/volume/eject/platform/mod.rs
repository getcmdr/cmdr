//! What an eject asks the OS: the tool that unmounts, whether a volume is ejectable, and how an
//! SMB disconnect takes a share down. A platform seam (`docs/guides/platform-seams.md`); the
//! rest of `eject/` (the kinds, the index stop, the retries, the holders) is every OS's.

use super::unmount_tool::{ToolOutcome, UnmountVerb};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(UnmountVerb) -> &'static str = tool_command;
const _: fn(UnmountVerb, &str) -> ToolOutcome = run_tool_blocking;
const _: fn(&str) -> bool = is_ejectable;
const _: fn(&str) -> bool = is_network_mount;
const _: bool = SMB_DISCONNECT_UNMOUNTS;

/// The command `verb` runs, as a log line names it.
pub(super) use imp::tool_command;

/// Runs the unmount tool against `mount_path` and reports how it ended. Blocking, and unbounded:
/// `unmount_tool::run` puts it on the blocking pool under its timeout.
pub(super) use imp::run_tool_blocking;

/// Whether the volume at `mount_path` is a removable or ejectable one (a USB drive, an SD card, a
/// disk image), so an eject runs the tool's `Eject` verb. Blocking, and can hang on a dead
/// backing store: the caller bounds it (`deadlines::EJECTABILITY_CHECK_DEADLINE`).
pub(super) use imp::is_ejectable;

/// Whether the volume at `mount_path` sits on a network filesystem mount (macOS: `smbfs`, NFS,
/// AFP, WebDAV), which Finder ejects by unmounting it. macOS reports every such mount as not
/// ejectable, so without this a share macOS mounted read "This drive isn't removable".
/// Non-blocking: the mount table alone.
pub(super) use imp::is_network_mount;

/// The teardown of a volume [`is_ejectable`] said yes to, where no per-disk flight runs: Linux's
/// tool against the volume alone, Windows' drive flight. macOS runs its per-disk flight instead
/// (`disk_flight`).
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) use imp::eject_ejectable;

/// Windows' eject of a drive, the sequence every route shares: pure over its `DriveSide`, so its
/// tests run on every OS (`volume/DETAILS.md` § "Windows: the drive flight").
#[cfg(any(target_os = "windows", test))]
mod drive_flight;

/// Whether "Disconnect" on an SMB volume unmounts the OS mount under it (the unmount approver
/// then removes the volume), or only drops the direct session, leaving any OS mount to the file
/// manager that made it.
pub(super) use imp::SMB_DISCONNECT_UNMOUNTS;
