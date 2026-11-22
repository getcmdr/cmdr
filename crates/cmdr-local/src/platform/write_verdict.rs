//! What the Windows write-access probe's two answers mean together
//! (`local_windows/write_access.rs`): the volume's `FILE_READ_ONLY_VOLUME` flag, and an open of
//! the folder asking for `FILE_ADD_FILE`. Pure, compiled into every OS's tests.

use cmdr_fs::volume::{UnwritableReason, WriteAccess};

/// `ERROR_ACCESS_DENIED`: the folder's DACL refuses this user.
const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_WRITE_PROTECT`: write-protected media.
const ERROR_WRITE_PROTECT: i32 = 19;

/// The verdict for a folder on a volume that is (`read_only_volume`) or isn't read-only, whose
/// `FILE_ADD_FILE` open answered `open` (`Err` carries its Win32 code, `None` for a refusal
/// before any call). Anything else (a sharing violation, a dead share) is `Unknown`, ❌ never
/// a guess: the write itself decides.
pub(crate) fn write_verdict(read_only_volume: bool, open: Result<(), Option<i32>>) -> WriteAccess {
    let read_only = WriteAccess::Unwritable {
        reason: UnwritableReason::ReadOnlyFilesystem,
    };
    if read_only_volume {
        return read_only;
    }
    match open {
        Ok(()) => WriteAccess::Writable,
        Err(Some(ERROR_WRITE_PROTECT)) => read_only,
        Err(Some(ERROR_ACCESS_DENIED)) => WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission,
        },
        Err(_) => WriteAccess::Unknown,
    }
}

#[cfg(test)]
#[path = "write_verdict_test.rs"]
mod tests;
