//! What the Windows write-access probe's answers mean together
//! (`local_windows/write_access.rs`): the volume's `FILE_READ_ONLY_VOLUME` flag, whether the
//! folder's DACL grants this user `FILE_ADD_FILE`, and whether the folder sits on a share.
//! Pure, compiled into every OS's tests.

use cmdr_fs::volume::{UnwritableReason, WriteAccess};

/// What the probe read of one folder.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Probed {
    /// The volume says it's read-only.
    pub read_only_volume: bool,
    /// The DACL grants this user `FILE_ADD_FILE` (`Some(true)`), refuses it (`Some(false)`), or
    /// couldn't be read (`None`, a dead share or no right to read the DACL).
    pub may_add_file: Option<bool>,
    /// The folder is on a share (a UNC path or a mapped drive).
    pub on_a_share: bool,
}

/// The verdict for a probed folder. An unreadable DACL is `Unknown`, ❌ never a guess, so the
/// write itself decides. ❗ On a share a DACL refusal is `Unknown` too: the server's DACL names
/// the server's accounts (a NAS's own users, Samba's `S-1-22-*`), which a local token's
/// `AccessCheck` never matches, so it would refuse a folder the server lets this user write.
/// A grant still counts: a wrong "writable" only costs a later typed refusal.
pub(crate) fn write_verdict(probed: Probed) -> WriteAccess {
    if probed.read_only_volume {
        return WriteAccess::Unwritable {
            reason: UnwritableReason::ReadOnlyFilesystem,
        };
    }
    match probed.may_add_file {
        Some(true) => WriteAccess::Writable,
        Some(false) if !probed.on_a_share => WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission,
        },
        Some(false) | None => WriteAccess::Unknown,
    }
}

#[cfg(test)]
#[path = "write_verdict_test.rs"]
mod tests;
