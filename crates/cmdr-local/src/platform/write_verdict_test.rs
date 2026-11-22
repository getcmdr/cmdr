use super::write_verdict;
use cmdr_fs::volume::{UnwritableReason, WriteAccess};

const READ_ONLY: WriteAccess = WriteAccess::Unwritable {
    reason: UnwritableReason::ReadOnlyFilesystem,
};

#[test]
fn a_read_only_volume_wins_over_whatever_the_open_said() {
    assert_eq!(write_verdict(true, Ok(())), READ_ONLY);
    assert_eq!(write_verdict(true, Err(Some(5))), READ_ONLY);
}

#[test]
fn the_open_decides_on_a_writable_volume() {
    assert_eq!(write_verdict(false, Ok(())), WriteAccess::Writable);
    assert_eq!(
        write_verdict(false, Err(Some(5))),
        WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission
        }
    );
    assert_eq!(write_verdict(false, Err(Some(19))), READ_ONLY, "write-protected media");
}

#[test]
fn anything_else_is_unknown_never_a_guess() {
    for open in [Err(Some(32)), Err(Some(53)), Err(None)] {
        assert_eq!(write_verdict(false, open), WriteAccess::Unknown, "{open:?}");
    }
}
