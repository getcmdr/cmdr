//! macOS: `diskutil`, and NSURL's `isEjectable` through the fast per-path resolver.

use super::super::unmount_tool::{ToolOutcome, UnmountVerb};

/// `diskutil unmount` makes FSEvents fire `NSWorkspaceDidUnmount`, which runs
/// `Volume::on_unmount` and unregisters the volume.
pub(crate) const SMB_DISCONNECT_UNMOUNTS: bool = true;

pub(crate) fn tool_command(verb: UnmountVerb) -> &'static str {
    match verb {
        UnmountVerb::Eject => "diskutil eject",
        UnmountVerb::Unmount => "diskutil unmount",
        UnmountVerb::ForceUnmount => "diskutil unmount force",
    }
}

pub(crate) fn run_tool_blocking(verb: UnmountVerb, mount_path: &str) -> ToolOutcome {
    let args: &[&str] = match verb {
        UnmountVerb::Eject => &["eject"],
        UnmountVerb::Unmount => &["unmount"],
        UnmountVerb::ForceUnmount => &["unmount", "force"],
    };
    ToolOutcome::of(
        std::process::Command::new("diskutil")
            .args(args)
            .arg(mount_path)
            .output(),
    )
}

pub(crate) fn is_ejectable(mount_path: &str) -> bool {
    crate::volumes::resolve_path_volume_fast(mount_path).is_some_and(|v| v.is_ejectable)
}
