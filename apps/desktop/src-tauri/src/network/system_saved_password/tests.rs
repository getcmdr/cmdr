//! `list_with` over a fake account lookup, a counting read behind the real one-read memo, and a
//! fake dial. ❌ Nothing here reaches the real Keychain.

use std::future::ready;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cmdr_smb::{AuthMode, ShareListError, ShareListResult};

use super::list_with;
use crate::network::keychain::SmbCredentials;
use crate::network::keychain_session::ReadOnce;

fn saved(username: &str) -> SmbCredentials {
    SmbCredentials {
        username: username.to_string(),
        password: "12345678".to_string(),
    }
}

fn listed() -> ShareListResult {
    ShareListResult {
        shares: Vec::new(),
        auth_mode: AuthMode::CredsRequired,
        from_cache: false,
    }
}

/// What macOS saved, behind the memo the real read uses, counting every consent-gated read.
struct MacosKeychain {
    memo: ReadOnce,
    saved: Option<SmbCredentials>,
    reads: AtomicUsize,
}

impl MacosKeychain {
    fn new(saved: Option<SmbCredentials>) -> Self {
        Self {
            memo: ReadOnce::default(),
            saved,
            reads: AtomicUsize::new(0),
        }
    }

    fn account(&self) -> Option<String> {
        self.saved.as_ref().map(|c| c.username.clone())
    }

    fn read(&self) -> std::future::Ready<Option<SmbCredentials>> {
        ready(self.memo.read(
            "smb://testnas.local",
            || None,
            || {
                self.reads.fetch_add(1, Ordering::SeqCst);
                self.saved.clone()
            },
        ))
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

async fn list(
    keychain: &MacosKeychain,
    typed_username: Option<&str>,
    dialed: &Mutex<Vec<String>>,
) -> Result<Option<ShareListResult>, ShareListError> {
    list_with(
        typed_username,
        || keychain.account(),
        || keychain.read(),
        |credentials| {
            dialed.lock().expect("test lock").push(credentials.username);
            ready(Ok(listed()))
        },
    )
    .await
}

/// The saved account lists, and asking again this run raises no second consent dialog.
#[tokio::test]
async fn the_saved_password_lists_and_is_asked_for_once() {
    let keychain = MacosKeychain::new(Some(saved("test_user_1")));
    let dialed = Mutex::new(Vec::new());

    let first = list(&keychain, None, &dialed).await;
    let second = list(&keychain, None, &dialed).await;

    assert!(matches!(first, Ok(Some(_))));
    assert!(matches!(second, Ok(None)), "the one ask is spent");
    assert_eq!(keychain.reads(), 1);
    assert_eq!(*dialed.lock().expect("test lock"), vec!["test_user_1".to_string()]);
}

/// Nothing saved (or a denial, which reads the same) dials nothing, and isn't asked again.
#[tokio::test]
async fn nothing_saved_dials_nothing_and_asks_once() {
    let keychain = MacosKeychain::new(None);
    let dialed = Mutex::new(Vec::new());

    assert!(matches!(list(&keychain, None, &dialed).await, Ok(None)));
    assert!(matches!(list(&keychain, None, &dialed).await, Ok(None)));

    assert_eq!(keychain.reads(), 1, "a miss is remembered");
    assert!(dialed.lock().expect("test lock").is_empty());
}

/// A server the person set an account for signs in only as that account: a password macOS
/// saved for someone else costs no consent dialog at all.
#[tokio::test]
async fn another_accounts_saved_password_is_never_read() {
    let keychain = MacosKeychain::new(Some(saved("test_user_2")));
    let dialed = Mutex::new(Vec::new());

    let answer = list(&keychain, Some("test_user_1"), &dialed).await;

    assert!(matches!(answer, Ok(None)));
    assert_eq!(keychain.reads(), 0);
    assert!(dialed.lock().expect("test lock").is_empty());
}

/// The typed account's own saved password signs it in.
#[tokio::test]
async fn the_typed_accounts_own_saved_password_lists() {
    let keychain = MacosKeychain::new(Some(saved("test_user_1")));
    let dialed = Mutex::new(Vec::new());

    assert!(matches!(
        list(&keychain, Some("test_user_1"), &dialed).await,
        Ok(Some(_))
    ));
    assert_eq!(keychain.reads(), 1);
}

/// The server's refusal comes back as is, for the sheet to word.
#[tokio::test]
async fn a_refused_saved_password_is_the_servers_answer() {
    let keychain = MacosKeychain::new(Some(saved("test_user_1")));

    let answer = list_with(
        None,
        || keychain.account(),
        || keychain.read(),
        |_| {
            ready(Err(ShareListError::AuthFailed {
                message: "rejected".to_string(),
            }))
        },
    )
    .await;

    assert!(matches!(answer, Err(ShareListError::AuthFailed { .. })));
}
