use super::write_verdict;
use cmdr_fs::volume::{UnwritableReason, WriteAccess};

const READ_ONLY: WriteAccess = WriteAccess::Unwritable {
    reason: UnwritableReason::ReadOnlyFilesystem,
};

#[test]
fn a_read_only_volume_wins_over_whatever_the_dacl_says() {
    for dacl in [Some(true), Some(false), None] {
        assert_eq!(write_verdict(true, dacl), READ_ONLY, "{dacl:?}");
    }
}

#[test]
fn the_dacl_decides_on_a_writable_volume() {
    assert_eq!(write_verdict(false, Some(true)), WriteAccess::Writable);
    assert_eq!(
        write_verdict(false, Some(false)),
        WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission
        }
    );
}

#[test]
fn a_dacl_nobody_could_read_is_unknown_never_a_guess() {
    assert_eq!(write_verdict(false, None), WriteAccess::Unknown);
}
