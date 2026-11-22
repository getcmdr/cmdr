use super::{Probed, write_verdict};
use cmdr_fs::volume::{UnwritableReason, WriteAccess};

const READ_ONLY: WriteAccess = WriteAccess::Unwritable {
    reason: UnwritableReason::ReadOnlyFilesystem,
};

fn local(read_only_volume: bool, may_add_file: Option<bool>) -> Probed {
    Probed {
        read_only_volume,
        may_add_file,
        on_a_share: false,
    }
}

#[test]
fn a_read_only_volume_wins_over_whatever_the_dacl_says() {
    for dacl in [Some(true), Some(false), None] {
        assert_eq!(write_verdict(local(true, dacl)), READ_ONLY, "{dacl:?}");
        let shared = Probed {
            on_a_share: true,
            ..local(true, dacl)
        };
        assert_eq!(write_verdict(shared), READ_ONLY, "{dacl:?} on a share");
    }
}

#[test]
fn the_dacl_decides_on_a_writable_local_volume() {
    assert_eq!(write_verdict(local(false, Some(true))), WriteAccess::Writable);
    assert_eq!(
        write_verdict(local(false, Some(false))),
        WriteAccess::Unwritable {
            reason: UnwritableReason::NoPermission
        }
    );
}

#[test]
fn a_dacl_nobody_could_read_is_unknown_never_a_guess() {
    assert_eq!(write_verdict(local(false, None)), WriteAccess::Unknown);
}

/// A share's DACL names the server's accounts, so a local `AccessCheck` refusal there must
/// never block a copy the server would take; its grant still counts.
#[test]
fn a_share_dacl_refusal_is_unknown_and_its_grant_is_writable() {
    let on_share = |may_add_file| Probed {
        on_a_share: true,
        ..local(false, may_add_file)
    };
    assert_eq!(write_verdict(on_share(Some(false))), WriteAccess::Unknown);
    assert_eq!(write_verdict(on_share(None)), WriteAccess::Unknown);
    assert_eq!(write_verdict(on_share(Some(true))), WriteAccess::Writable);
}
