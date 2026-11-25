//! Which failed space queries reach the log at WARN, and which drop to DEBUG.

use super::*;
use smb2::types::Command;
use smb2::types::status::NtStatus;

fn refused() -> Result<(), smb2::Error> {
    Err(smb2::Error::Protocol {
        status: NtStatus::INVALID_PARAMETER,
        command: Command::Create,
    })
}

fn lost() -> Result<(), smb2::Error> {
    Err(smb2::Error::Disconnected)
}

/// A share that refuses every poll says so once, then quietly: the 5 s poller
/// would otherwise put the same WARN in the log ~720 times an hour.
#[test]
fn a_share_that_keeps_refusing_warns_once_then_logs_at_debug() {
    let refusals = SpaceRefusals::new();
    assert_eq!(refusals.level_for(&refused()), Level::Warn);
    assert_eq!(refusals.level_for(&refused()), Level::Debug);
    assert_eq!(refusals.level_for(&refused()), Level::Debug);
}

/// A share that answered again and then refuses once more is news: it warns.
#[test]
fn an_answer_in_between_rearms_the_warning() {
    let refusals = SpaceRefusals::new();
    assert_eq!(refusals.level_for(&refused()), Level::Warn);
    assert_eq!(refusals.level_for(&Ok(())), Level::Warn);
    assert_eq!(refusals.level_for(&refused()), Level::Warn);
}

/// Only the server's own refusal goes quiet. A failure that isn't the server
/// answering "no" (a dropped connection, a timeout) keeps its WARN every time,
/// and doesn't use up the refusal's one warning.
#[test]
fn a_failure_that_is_not_a_refusal_always_warns() {
    let refusals = SpaceRefusals::new();
    assert_eq!(refusals.level_for(&lost()), Level::Warn);
    assert_eq!(refusals.level_for(&lost()), Level::Warn);
    assert_eq!(refusals.level_for(&refused()), Level::Warn);
    assert_eq!(refusals.level_for(&refused()), Level::Debug);
}
