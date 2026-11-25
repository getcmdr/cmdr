//! Linux: `umount` for every verb, and `/sys/block/*/removable` through the location list.

use super::super::unmount_tool::{ToolOutcome, UnmountVerb};

/// The OS-level unmount of a GVFS or kernel SMB mount isn't wired up (as
/// `network::os_mount::unmount_smb_shares_from_host`), so a disconnect drops the direct session.
pub(crate) const SMB_DISCONNECT_UNMOUNTS: bool = false;

pub(crate) fn tool_command(verb: UnmountVerb) -> &'static str {
    match verb {
        UnmountVerb::Eject | UnmountVerb::Unmount => "umount",
        UnmountVerb::ForceUnmount => "umount -f",
    }
}

/// `umount` covers the SMB and removable cases; the physical-drive power-down `diskutil eject`
/// does is rare on Linux dev machines. `-f` is the kernel's force for an unreachable network
/// mount (CIFS).
pub(crate) fn run_tool_blocking(verb: UnmountVerb, mount_path: &str) -> ToolOutcome {
    let mut command = std::process::Command::new("umount");
    if verb == UnmountVerb::ForceUnmount {
        command.arg("-f");
    }
    ToolOutcome::of(command.arg(mount_path).output())
}

/// An ejectable volume's teardown: the volume alone, as for a mount no disk backs (no per-disk
/// flight on Linux).
pub(in crate::file_system::volume::eject) async fn eject_ejectable(
    volume_id: &str,
    mount_path: &str,
) -> Result<super::super::EjectOutcome, super::super::EjectError> {
    super::super::eject_one_volume(volume_id, mount_path, UnmountVerb::Eject).await?;
    Ok(super::super::EjectOutcome::Unmounted)
}

pub(crate) fn is_ejectable(mount_path: &str) -> bool {
    crate::volumes::list_locations()
        .into_iter()
        .find(|v| v.path == mount_path)
        .is_some_and(|v| v.is_ejectable)
}
