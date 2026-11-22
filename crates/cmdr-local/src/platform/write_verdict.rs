//! What the Windows write-access probe's two answers mean together
//! (`local_windows/write_access.rs`): the volume's `FILE_READ_ONLY_VOLUME` flag, and whether the
//! folder's DACL grants this user `FILE_ADD_FILE`. Pure, compiled into every OS's tests.

use cmdr_fs::volume::{UnwritableReason, WriteAccess};

/// The verdict for a folder on a volume that is (`read_only_volume`) or isn't read-only, whose
/// DACL grants this user `FILE_ADD_FILE` (`Some(true)`), refuses it (`Some(false)`), or couldn't
/// be read (`None`, a dead share or no right to read the DACL): `Unknown`, ❌ never a guess, so
/// the write itself decides.
pub(crate) fn write_verdict(read_only_volume: bool, may_add_file: Option<bool>) -> WriteAccess {
    if read_only_volume {
        return WriteAccess::Unwritable {
            reason: UnwritableReason::ReadOnlyFilesystem,
        };
    }
    match may_add_file {
        Some(true) => WriteAccess::Writable,
        Some(false) => WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission,
        },
        None => WriteAccess::Unknown,
    }
}

#[cfg(test)]
#[path = "write_verdict_test.rs"]
mod tests;
