//! Taking down an SMB mount that may be DEAD: a normal unmount first, and a force
//! unmount only when that one didn't answer.
//!
//! The one caller is "Eject and save" on an SMB host's address move
//! (`server_move_smb.rs`): a NAS that got a new IP leaves its old-address mounts dead,
//! and a normal unmount of a dead SMB mount hangs.
//!
//! ❗ **Force only on no answer, ❌ never on a refusal.** A refused unmount
//! ([`EjectError::UnmountRefused`]) means something has files open there: the mount is
//! LIVE, and forcing it could lose someone's unsaved work in another app. A dead mount
//! loses nothing to a force. Every other answer stops too: a stalled pre-unmount step
//! (`NotResponding`) never reached the unmount, so forcing past it would skip the index
//! stop the wedge guard needs. `volume/DETAILS.md` § "Eject" has the why.

use super::{EjectError, EjectOutcome, Teardown, UnmountVerb, in_flight, run_teardown};

/// How a mount went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakenDown {
    /// The normal unmount did it (or the mount was already gone).
    Unmounted,
    /// The normal unmount didn't answer, so a force unmount did it.
    Forced,
}

/// What the normal unmount's answer leaves to do.
#[derive(Debug)]
enum Next {
    Done,
    Force,
    Stop(EjectError),
}

/// Reads the normal unmount's answer. Pure, so the escalation rule is pinned without
/// a mount.
fn after_normal(answer: Result<(), EjectError>) -> Next {
    match answer {
        Ok(()) => Next::Done,
        // No answer within the tool's timeout: what a dead mount does.
        Err(EjectError::TimedOut) => Next::Force,
        Err(error) => Next::Stop(error),
    }
}

/// Runs `normal`, and `force` only when [`after_normal`] says so. Both are parameters
/// so a test drives the escalation without `diskutil`.
async fn normal_then_force<N, NF, F, FF>(normal: N, force: F) -> Result<TakenDown, EjectError>
where
    N: FnOnce() -> NF,
    NF: Future<Output = Result<(), EjectError>>,
    F: FnOnce() -> FF,
    FF: Future<Output = Result<(), EjectError>>,
{
    match after_normal(normal().await) {
        Next::Done => Ok(TakenDown::Unmounted),
        Next::Force => force().await.map(|()| TakenDown::Forced),
        Next::Stop(error) => Err(error),
    }
}

/// Takes down the registered SMB volume `volume_id`: its normal [`super::eject`] (the
/// busy gate, the index stop, the retries on a transient hold), then a force unmount of
/// its root when that didn't answer.
///
/// ❗ The force runs inside its own eject flight (`in_flight`), so the switcher shows
/// the volume ejecting and a click there joins it. It skips the index stop: the normal
/// eject only reaches the tool, and so its timeout, after the stop let go.
pub async fn take_down_smb_volume(volume_id: &str) -> Result<TakenDown, EjectError> {
    normal_then_force(
        || async { super::eject(volume_id).await.map(|_| ()) },
        || async {
            let owned = volume_id.to_string();
            in_flight::join_or_start(volume_id, move || force_volume(owned))
                .await
                .map(|_| ())
        },
    )
    .await
}

/// Takes down the SMB mount at `mount_path` that no volume is registered under (a
/// mount Finder or another app made, which the registry never took): a normal unmount,
/// then a force one when that didn't answer.
pub async fn take_down_smb_mount(mount_path: &str) -> Result<TakenDown, EjectError> {
    normal_then_force(
        || run_teardown(mount_path, Teardown::volume_tool(UnmountVerb::Unmount, mount_path)),
        || run_teardown(mount_path, Teardown::volume_tool(UnmountVerb::ForceUnmount, mount_path)),
    )
    .await
}

/// The force unmount of a registered volume's root, as an eject flight.
async fn force_volume(volume_id: String) -> Result<EjectOutcome, EjectError> {
    let Some(volume) = crate::file_system::volume::manager::get_volume_manager().get(&volume_id) else {
        // The unmount the timeout didn't cancel landed meanwhile: the goal is met.
        return Ok(EjectOutcome::AlreadyGone);
    };
    let mount_path = volume.root().to_string_lossy().to_string();
    run_teardown(
        &volume_id,
        Teardown::volume_tool(UnmountVerb::ForceUnmount, &mount_path),
    )
    .await?;
    Ok(EjectOutcome::Unmounted)
}

#[cfg(test)]
mod tests {
    use super::super::holders::HolderScan;
    use super::*;
    use std::cell::Cell;

    fn refused() -> EjectError {
        EjectError::UnmountRefused {
            holders: HolderScan::not_scanned(),
            detail: "diskutil unmount: dissented by PID 4242 (Preview)".to_string(),
        }
    }

    /// Drives the escalation with `normal`'s answer and a force that succeeds,
    /// answering the result and whether the force ran.
    async fn escalate(normal: Result<(), EjectError>) -> (Result<TakenDown, EjectError>, bool) {
        let forced = Cell::new(false);
        let result = normal_then_force(
            || async { normal },
            || async {
                forced.set(true);
                Ok(())
            },
        )
        .await;
        (result, forced.get())
    }

    #[tokio::test]
    async fn a_normal_unmount_that_went_through_never_forces() {
        let (result, forced) = escalate(Ok(())).await;
        assert_eq!(result.ok(), Some(TakenDown::Unmounted));
        assert!(!forced);
    }

    /// ❗ A dead mount doesn't answer, and forcing it loses nothing.
    #[tokio::test]
    async fn a_normal_unmount_that_never_answered_escalates_to_a_force() {
        let (result, forced) = escalate(Err(EjectError::TimedOut)).await;
        assert_eq!(result.ok(), Some(TakenDown::Forced));
        assert!(forced);
    }

    /// ❗ **A refusal is a LIVE mount someone is using: ❌ never forced.** Its answer
    /// goes back as-is, so the sheet can say who's holding it.
    #[tokio::test]
    async fn a_refused_unmount_is_never_forced_and_answers_the_refusal() {
        let (result, forced) = escalate(Err(refused())).await;
        assert!(!forced, "forcing a live mount could lose someone's unsaved work");
        assert!(
            matches!(result, Err(EjectError::UnmountRefused { ref detail, .. }) if detail.contains("4242")),
            "got {result:?}"
        );
    }

    /// A pre-unmount step that stalled never reached the unmount, so forcing past it
    /// would skip the index stop.
    #[tokio::test]
    async fn a_stalled_step_before_the_unmount_is_never_forced() {
        let (result, forced) = escalate(Err(EjectError::NotResponding {
            step: super::super::EjectStep::IndexStop,
        }))
        .await;
        assert!(!forced);
        assert!(
            matches!(result, Err(EjectError::NotResponding { .. })),
            "got {result:?}"
        );
    }

    #[tokio::test]
    async fn a_busy_volume_is_never_forced() {
        let (result, forced) = escalate(Err(EjectError::Busy)).await;
        assert!(!forced);
        assert!(matches!(result, Err(EjectError::Busy)), "got {result:?}");
    }

    /// A force that doesn't go through either answers its own refusal.
    #[tokio::test]
    async fn a_force_that_fails_answers_its_own_error() {
        let result = normal_then_force(
            || async { Err(EjectError::TimedOut) },
            || async {
                Err(EjectError::Unexpected {
                    detail: "diskutil unmount force: couldn't start".to_string(),
                })
            },
        )
        .await;
        assert!(matches!(result, Err(EjectError::Unexpected { .. })), "got {result:?}");
    }
}
