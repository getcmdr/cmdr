//! The unlock commands over a scripted locked volume: each typed outcome comes
//! back as the volume answered it, a cancel or a timeout keeps nothing, and a
//! path that leads into nothing locked is a typed refusal.

use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::IgnorePoison;
use crate::file_system::volume::manager::get_volume_manager;
use crate::file_system::volume::{
    InMemoryVolume, ListingProgress, PasswordForgotten, PasswordLocked, UnlockOutcome, Volume, VolumeError,
};
use cmdr_fs::entry::FileEntry;

use super::{
    UnlockError, cancel_container_unlock, forget_container_password, forget_saved_container_password,
    in_flight_unlocks, unlock_container, unlock_within,
};

/// How the stub answers an unlock.
#[derive(Clone, Copy)]
enum Script {
    /// Answers at once with this outcome.
    Answer(UnlockOutcome),
    /// Never answers: a repo open on a NAS that went quiet.
    Hang,
    /// The container couldn't be asked.
    Gone,
}

/// A locked container that keeps what a real one would: the password on a
/// non-wrong outcome, and what it was asked to file in the credential store.
struct LockedStub {
    inner: InMemoryVolume,
    script: Script,
    kept: Mutex<Option<String>>,
    filed: Mutex<Option<(String, bool)>>,
    /// Fired the moment the unlock answers: a Cancel that lands just after.
    cancel_on_answer: Option<tokio_util::sync::CancellationToken>,
}

impl LockedStub {
    fn new(script: Script) -> Self {
        Self {
            inner: InMemoryVolume::new("Locked"),
            script,
            kept: Mutex::new(None),
            filed: Mutex::new(None),
            cancel_on_answer: None,
        }
    }

    fn kept(&self) -> Option<String> {
        self.kept.lock_ignore_poison().clone()
    }

    fn filed(&self) -> Option<(String, bool)> {
        self.filed.lock_ignore_poison().clone()
    }
}

impl PasswordLocked for LockedStub {
    fn unlock<'a>(
        &'a self,
        password: String,
    ) -> Pin<Box<dyn Future<Output = Result<UnlockOutcome, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            match self.script {
                Script::Answer(outcome) => {
                    if outcome != UnlockOutcome::WrongPassword {
                        *self.kept.lock_ignore_poison() = Some(password);
                    }
                    if let Some(cancel) = &self.cancel_on_answer {
                        cancel.cancel();
                    }
                    Ok(outcome)
                }
                Script::Hang => {
                    std::future::pending::<()>().await;
                    unreachable!("a pending future never resolves")
                }
                Script::Gone => Err(VolumeError::DeviceDisconnected("the NAS left".into())),
            }
        })
    }

    fn remember_password<'a>(
        &'a self,
        password: String,
        remember: bool,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        *self.filed.lock_ignore_poison() = Some((password, remember));
        Box::pin(async {})
    }

    fn forget_password(&self) {
        *self.kept.lock_ignore_poison() = None;
    }

    /// Forgets everywhere, and answers that the store kept its copy, so a test
    /// sees the volume's answer come back untouched.
    fn forget_saved_password<'a>(&'a self) -> Pin<Box<dyn Future<Output = PasswordForgotten> + Send + 'a>> {
        Box::pin(async move {
            *self.kept.lock_ignore_poison() = None;
            *self.filed.lock_ignore_poison() = None;
            PasswordForgotten::StoreKeptIt
        })
    }
}

impl Volume for LockedStub {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn root(&self) -> &AppPath {
        self.inner.root()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_password_locked(&self) -> Option<&dyn PasswordLocked> {
        Some(self)
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.inner.list_directory(path, on_progress)
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        self.inner.get_metadata(path)
    }
}

/// Registers `volume` under a fresh id and hands the id back. Unregister when done.
fn register(volume: Arc<dyn Volume>) -> String {
    let id = format!("unlock-test-{}", uuid::Uuid::new_v4());
    get_volume_manager().register(&id, volume);
    id
}

async fn unlock(id: &str, password: &str, remember: bool) -> Result<UnlockOutcome, UnlockError> {
    unlock_container(
        id.to_string(),
        "/repo".to_string(),
        password.to_string(),
        remember,
        uuid::Uuid::new_v4().to_string(),
    )
    .await
}

#[tokio::test]
async fn each_outcome_comes_back_as_the_container_answered() {
    for outcome in [
        UnlockOutcome::Stored,
        UnlockOutcome::Unlocked,
        UnlockOutcome::WrongPassword,
    ] {
        let stub = Arc::new(LockedStub::new(Script::Answer(outcome)));
        let id = register(stub.clone());
        let answer = unlock(&id, "hunter2", true).await;
        get_volume_manager().unregister(&id);

        assert!(matches!(answer, Ok(answered) if answered == outcome), "{answer:?}");
        let kept = stub.kept();
        if outcome == UnlockOutcome::WrongPassword {
            assert_eq!(kept, None, "a wrong password keeps nothing");
        } else {
            assert_eq!(kept, Some("hunter2".to_string()), "for {outcome:?}");
        }
        let filed = (outcome == UnlockOutcome::Unlocked).then(|| ("hunter2".to_string(), true));
        assert_eq!(stub.filed(), filed, "only a verified unlock is filed, for {outcome:?}");
    }
}

#[tokio::test]
async fn remember_reaches_the_container_as_given() {
    let stub = Arc::new(LockedStub::new(Script::Answer(UnlockOutcome::Unlocked)));
    let id = register(stub.clone());
    let answer = unlock(&id, "hunter2", false).await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Ok(UnlockOutcome::Unlocked)), "{answer:?}");
    assert_eq!(stub.filed(), Some(("hunter2".to_string(), false)));
}

#[tokio::test]
async fn a_path_into_nothing_locked_is_a_typed_refusal() {
    let id = register(Arc::new(InMemoryVolume::new("Plain")));
    let answer = unlock(&id, "hunter2", true).await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::NotLocked)), "{answer:?}");
}

#[tokio::test]
async fn an_unregistered_volume_is_a_typed_refusal() {
    let answer = unlock("unlock-test-nobody", "hunter2", true).await;
    assert!(matches!(answer, Err(UnlockError::VolumeNotFound { .. })), "{answer:?}");
}

#[tokio::test]
async fn a_container_that_cannot_be_asked_says_why_without_the_password() {
    let id = register(Arc::new(LockedStub::new(Script::Gone)));
    let answer = unlock(&id, "hunter2", true).await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::Volume { .. })), "{answer:?}");
    assert!(!format!("{answer:?}").contains("hunter2"));
}

#[tokio::test(start_paused = true)]
async fn an_unlock_past_its_budget_times_out_and_keeps_nothing() {
    let stub = Arc::new(LockedStub::new(Script::Hang));
    let id = register(stub.clone());
    let cancel = tokio_util::sync::CancellationToken::new();
    let answer = unlock_within(
        &id,
        Path::new("/repo"),
        "hunter2".to_string(),
        true,
        &cancel,
        Duration::from_secs(30),
    )
    .await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::TimedOut)), "{answer:?}");
    assert_eq!(stub.kept(), None);
    assert_eq!(stub.filed(), None);
}

#[tokio::test]
async fn a_cancelled_unlock_answers_cancelled_and_keeps_nothing() {
    let stub = Arc::new(LockedStub::new(Script::Hang));
    let id = register(stub.clone());
    let cancel = tokio_util::sync::CancellationToken::new();

    let unlocking = unlock_within(
        &id,
        Path::new("/repo"),
        "hunter2".to_string(),
        true,
        &cancel,
        Duration::from_secs(30),
    );
    let (answer, ()) = tokio::join!(unlocking, async { cancel.cancel() });
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::Cancelled)), "{answer:?}");
    assert_eq!(stub.kept(), None, "a cancelled unlock caches nothing and saves nothing");
    assert_eq!(stub.filed(), None);
}

/// ❗ A Cancel that lands the moment the unlock answered still wins: the person
/// saw "cancelled", so the container forgets the password and nothing is filed
/// in the Keychain (a store write can't be taken back).
#[tokio::test]
async fn a_cancel_right_after_the_answer_forgets_and_files_nothing() {
    let cancel = tokio_util::sync::CancellationToken::new();
    let stub = Arc::new(LockedStub {
        cancel_on_answer: Some(cancel.clone()),
        ..LockedStub::new(Script::Answer(UnlockOutcome::Unlocked))
    });
    let id = register(stub.clone());

    let answer = unlock_within(
        &id,
        Path::new("/repo"),
        "hunter2".to_string(),
        true,
        &cancel,
        Duration::from_secs(30),
    )
    .await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::Cancelled)), "{answer:?}");
    assert_eq!(stub.kept(), None, "the late cancel forgets the password");
    assert_eq!(stub.filed(), None, "and files nothing");
}

#[tokio::test]
async fn forgetting_drops_what_the_container_kept() {
    let stub = Arc::new(LockedStub::new(Script::Answer(UnlockOutcome::Stored)));
    let id = register(stub.clone());
    unlock(&id, "hunter2", false).await.unwrap();
    let answer = forget_container_password(id.clone(), "/repo".to_string()).await;
    get_volume_manager().unregister(&id);

    assert!(answer.is_ok(), "{answer:?}");
    assert_eq!(stub.kept(), None);
}

#[tokio::test]
async fn forgetting_the_saved_password_drops_it_everywhere_and_says_what_the_store_did() {
    let stub = Arc::new(LockedStub::new(Script::Answer(UnlockOutcome::Unlocked)));
    let id = register(stub.clone());
    unlock(&id, "hunter2", true).await.unwrap();
    let answer = forget_saved_container_password(id.clone(), "/repo".to_string()).await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Ok(PasswordForgotten::StoreKeptIt)), "{answer:?}");
    assert_eq!(stub.kept(), None);
    assert_eq!(stub.filed(), None);
}

#[tokio::test]
async fn forgetting_a_saved_password_where_nothing_is_locked_is_a_typed_refusal() {
    let id = register(Arc::new(InMemoryVolume::new("Plain")));
    let answer = forget_saved_container_password(id.clone(), "/repo".to_string()).await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::NotLocked)), "{answer:?}");
}

#[tokio::test]
async fn a_cancel_by_id_drops_the_unlock_and_keeps_nothing() {
    let stub = Arc::new(LockedStub::new(Script::Hang));
    let id = register(stub.clone());
    let unlock_id = uuid::Uuid::new_v4().to_string();

    let unlocking = unlock_container(
        id.clone(),
        "/repo".to_string(),
        "hunter2".to_string(),
        true,
        unlock_id.clone(),
    );
    let cancelling = async {
        // Let the unlock register and reach the container first.
        tokio::task::yield_now().await;
        cancel_container_unlock(unlock_id.clone()).await;
    };
    let (answer, ()) = tokio::join!(unlocking, cancelling);
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::Cancelled)), "{answer:?}");
    assert_eq!(stub.kept(), None, "a cancelled unlock caches nothing and saves nothing");
    assert_eq!(stub.filed(), None);
    assert!(
        !in_flight_unlocks().contains(&unlock_id),
        "a finished unlock leaves no entry"
    );
}

#[tokio::test]
async fn a_cancel_that_overtakes_its_unlock_still_drops_it() {
    // Two IPC calls aren't ordered: the dialog's Cancel can reach the backend
    // before the unlock it cancels has registered.
    let stub = Arc::new(LockedStub::new(Script::Hang));
    let id = register(stub.clone());
    let unlock_id = uuid::Uuid::new_v4().to_string();

    cancel_container_unlock(unlock_id.clone()).await;
    let answer = unlock_container(
        id.clone(),
        "/repo".to_string(),
        "hunter2".to_string(),
        true,
        unlock_id.clone(),
    )
    .await;
    get_volume_manager().unregister(&id);

    assert!(matches!(answer, Err(UnlockError::Cancelled)), "{answer:?}");
    assert_eq!(stub.kept(), None);
    assert!(!in_flight_unlocks().contains(&unlock_id));
}

#[tokio::test]
async fn cancels_for_unlocks_that_never_come_stay_bounded() {
    // A cancel that lands after its unlock finished leaves a marker nothing
    // will claim. However many pile up, the registry stays small.
    for _ in 0..1_000 {
        cancel_container_unlock(uuid::Uuid::new_v4().to_string()).await;
    }
    let left = in_flight_unlocks().len();
    assert!(left <= 64, "the registry kept {left}");
}
