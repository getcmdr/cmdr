//! Windows: no drive eject yet (the drives task in `docs/specs/windows-port-plan.md`: drive
//! letters, the device-arrival watcher, and `CM_Request_Device_EjectW` for a removable drive).
//! Nothing reads as ejectable, so a local drive's eject refuses as `NotEjectable` before any tool
//! runs, and a tool run (an SMB share's unmount) answers that no tool could start, which the
//! eject reports as `Unexpected`. Device providers (MTP, ADB) and remote sessions (SFTP, WebDAV)
//! never reach this seam, so their ejects work as on every OS.

use super::super::unmount_tool::{ToolOutcome, UnmountVerb};

/// Cmdr makes no OS SMB mount on Windows yet (`WNetAddConnection2W` is the SMB mounts task), so a
/// disconnect drops the direct session, as on Linux.
pub(crate) const SMB_DISCONNECT_UNMOUNTS: bool = false;

pub(crate) fn tool_command(_verb: UnmountVerb) -> &'static str {
    "unmount (no tool on Windows yet)"
}

pub(crate) fn run_tool_blocking(_verb: UnmountVerb, _mount_path: &str) -> ToolOutcome {
    ToolOutcome::of(Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows has no unmount tool wired up yet",
    )))
}

pub(crate) fn is_ejectable(_mount_path: &str) -> bool {
    false
}
