//! The share list's sign-in ladder and the mount's, over a fake dial and a counting
//! store behind the real one-read memo.

use std::future::ready;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cmdr_smb::{AuthMode, ShareInfo, ShareListError, ShareListResult};

use super::{Ladder, ShareListAccount, mount_signed_in, sign_in_and_list};
use crate::network::keychain::SmbCredentials;
use crate::network::keychain_session::ReadOnce;
use crate::network::os_mount::MountError;
use crate::network::smb_client::GuestAttempt;

fn creds(username: &str) -> SmbCredentials {
    SmbCredentials {
        username: username.to_string(),
        password: "12345678".to_string(),
    }
}

fn listed(from_cache: bool) -> ShareListResult {
    ShareListResult {
        shares: vec![ShareInfo {
            name: "public".to_string(),
            is_disk: true,
            comment: None,
        }],
        auth_mode: AuthMode::GuestAllowed,
        from_cache,
    }
}

fn auth_required() -> ShareListError {
    ShareListError::AuthRequired {
        message: "guest refused".to_string(),
    }
}

/// One session's store: the real one-read memo over a counting read, and the memory a
/// hit leaves behind (the keychain's cache).
struct Session {
    memo: ReadOnce,
    stored: Option<SmbCredentials>,
    cached: Mutex<Option<SmbCredentials>>,
    typed: Option<SmbCredentials>,
    reads: AtomicUsize,
}

impl Session {
    fn new(stored: Option<SmbCredentials>) -> Self {
        Self {
            memo: ReadOnce::default(),
            stored,
            cached: Mutex::new(None),
            typed: None,
            reads: AtomicUsize::new(0),
        }
    }

    fn with_typed(mut self, typed: SmbCredentials) -> Self {
        self.typed = Some(typed);
        self
    }

    fn known(&self) -> Option<SmbCredentials> {
        self.typed
            .clone()
            .or_else(|| self.cached.lock().expect("test lock").clone())
    }

    fn stored_once(&self) -> std::future::Ready<Option<SmbCredentials>> {
        let answer = self.memo.read(
            "smb://testnas",
            || self.known(),
            || {
                self.reads.fetch_add(1, Ordering::SeqCst);
                let found = self.stored.clone();
                *self.cached.lock().expect("test lock") = found.clone();
                found
            },
        );
        ready(answer)
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

/// A server that lets a guest in when `guest_ok`, and an account in when it offers
/// `testuser`'s password. Records every dial.
struct Server {
    guest_ok: bool,
    dials: Mutex<Vec<(Option<String>, GuestAttempt)>>,
}

impl Server {
    fn new(guest_ok: bool) -> Self {
        Self {
            guest_ok,
            dials: Mutex::new(Vec::new()),
        }
    }

    fn dial(
        &self,
        credentials: Option<SmbCredentials>,
        guest: GuestAttempt,
    ) -> std::future::Ready<Result<ShareListResult, ShareListError>> {
        let user = credentials.as_ref().map(|c| c.username.clone());
        self.dials.lock().expect("test lock").push((user.clone(), guest));
        ready(match (user.as_deref(), guest) {
            (Some("testuser"), _) => Ok(listed(false)),
            (Some(_), _) => Err(ShareListError::AuthFailed {
                message: "rejected".to_string(),
            }),
            (None, GuestAttempt::Try) if self.guest_ok => Ok(listed(false)),
            (None, _) => Err(auth_required()),
        })
    }

    fn dials(&self) -> Vec<(Option<String>, GuestAttempt)> {
        self.dials.lock().expect("test lock").clone()
    }
}

async fn list(session: &Session, server: &Server, typed_username: Option<&str>) -> super::Listed {
    sign_in_and_list(
        Ladder {
            typed_username: typed_username.map(str::to_string),
            known: || session.known(),
            stored_once: || session.stored_once(),
        },
        |credentials, guest| server.dial(credentials, guest),
    )
    .await
}

/// With a stored password the typed account lists, and a second listing (⌘R) reads the
/// store no second time.
#[tokio::test]
async fn a_typed_account_lists_with_its_stored_password_read_once() {
    let session = Session::new(Some(creds("testuser")));
    let server = Server::new(false);

    let first = list(&session, &server, Some("testuser")).await;
    let second = list(&session, &server, Some("testuser")).await;

    assert!(first.result.is_ok() && second.result.is_ok());
    assert_eq!(
        first.account,
        Some(ShareListAccount::User {
            username: "testuser".to_string()
        })
    );
    assert_eq!(session.reads(), 1, "the store was read once");
    assert_eq!(
        server.dials(),
        vec![
            (Some("testuser".to_string()), GuestAttempt::Skip),
            (Some("testuser".to_string()), GuestAttempt::Skip)
        ]
    );
}

/// Nothing stored: the typed account answers `AuthRequired` (the pane's sign-in), and a
/// second listing doesn't ask the store again.
#[tokio::test]
async fn a_typed_account_with_nothing_stored_asks_for_a_sign_in_once() {
    let session = Session::new(None);
    let server = Server::new(true);

    let first = list(&session, &server, Some("testuser")).await;
    let second = list(&session, &server, Some("testuser")).await;

    assert!(matches!(first.result, Err(ShareListError::AuthRequired { .. })));
    assert!(matches!(second.result, Err(ShareListError::AuthRequired { .. })));
    assert_eq!(session.reads(), 1, "a miss is remembered");
    assert!(
        server.dials().iter().all(|(_, guest)| *guest == GuestAttempt::Skip),
        "❌ never as guest for a typed account"
    );
}

/// A sheet's answer (the session credential) signs in with no store read.
#[tokio::test]
async fn a_typed_account_uses_the_session_credential_first() {
    let session = Session::new(Some(creds("testuser"))).with_typed(creds("testuser"));
    let server = Server::new(false);

    let listed = list(&session, &server, Some("testuser")).await;

    assert!(listed.result.is_ok());
    assert_eq!(session.reads(), 0);
}

/// Another account's password never signs a typed account in.
#[tokio::test]
async fn a_stored_password_for_another_account_is_not_offered() {
    let session = Session::new(Some(creds("someone_else")));
    let server = Server::new(false);

    let listed = list(&session, &server, Some("testuser")).await;

    assert!(matches!(listed.result, Err(ShareListError::AuthRequired { .. })));
    assert_eq!(server.dials(), vec![(None, GuestAttempt::Skip)]);
}

/// A server that lets guests in costs no store read.
#[tokio::test]
async fn a_guest_listing_reads_nothing() {
    let session = Session::new(Some(creds("testuser")));
    let server = Server::new(true);

    let listed = list(&session, &server, None).await;

    assert!(listed.result.is_ok());
    assert_eq!(listed.account, Some(ShareListAccount::Guest));
    assert_eq!(session.reads(), 0);
}

/// A cached list doesn't say who listed it.
#[tokio::test]
async fn a_cached_guest_list_names_no_account() {
    let session = Session::new(None);
    let listed = sign_in_and_list(
        Ladder {
            typed_username: None,
            known: || session.known(),
            stored_once: || session.stored_once(),
        },
        |_, _| ready(Ok(listed(true))),
    )
    .await;

    assert!(listed.result.is_ok());
    assert_eq!(listed.account, None);
}

/// A guest turned away falls to the stored password, read once.
#[tokio::test]
async fn a_refused_guest_falls_to_the_stored_password() {
    let session = Session::new(Some(creds("testuser")));
    let server = Server::new(false);

    let listed = list(&session, &server, None).await;

    assert!(listed.result.is_ok());
    assert_eq!(
        listed.account,
        Some(ShareListAccount::User {
            username: "testuser".to_string()
        })
    );
    assert_eq!(session.reads(), 1);
}

/// A server that can't be reached is no reason to read the store.
#[tokio::test]
async fn an_unreachable_server_reads_nothing() {
    let session = Session::new(Some(creds("testuser")));
    let listed = sign_in_and_list(
        Ladder {
            typed_username: None,
            known: || session.known(),
            stored_once: || session.stored_once(),
        },
        |_, _| {
            ready(Err(ShareListError::HostUnreachable {
                message: "no route".to_string(),
            }))
        },
    )
    .await;

    assert!(matches!(listed.result, Err(ShareListError::HostUnreachable { .. })));
    assert_eq!(session.reads(), 0);
}

// ── The mount ───────────────────────────────────────────────────────

/// A share that takes a guest when `guest_ok`, else `testuser`. Records each mount's account.
struct Share {
    guest_ok: bool,
    mounts: Mutex<Vec<Option<String>>>,
}

impl Share {
    fn new(guest_ok: bool) -> Self {
        Self {
            guest_ok,
            mounts: Mutex::new(Vec::new()),
        }
    }

    fn mount(&self, credentials: Option<SmbCredentials>) -> std::future::Ready<Result<&'static str, MountError>> {
        let user = credentials.map(|c| c.username);
        self.mounts.lock().expect("test lock").push(user.clone());
        ready(match user.as_deref() {
            Some("testuser") => Ok("/Volumes/public"),
            Some(_) => Err(MountError::AuthFailed {
                server: "testnas".to_string(),
            }),
            None if self.guest_ok => Ok("/Volumes/public"),
            None => Err(MountError::AuthRequired {
                server: "testnas".to_string(),
                share: "public".to_string(),
            }),
        })
    }

    fn mounts(&self) -> Vec<Option<String>> {
        self.mounts.lock().expect("test lock").clone()
    }
}

async fn mount(session: &Session, share: &Share, typed_account: bool) -> Result<Option<String>, MountError> {
    mount_signed_in(
        typed_account,
        || session.known(),
        || session.stored_once(),
        |c| share.mount(c),
    )
    .await
    .map(|(_, used)| used.map(|c| c.username))
}

/// After a typed sign-in (remembered or not), a mount with no credentials goes through
/// as that account, with no store read.
#[tokio::test]
async fn a_mount_after_a_typed_sign_in_uses_the_session_credential() {
    let session = Session::new(None).with_typed(creds("testuser"));
    let share = Share::new(false);

    assert_eq!(
        mount(&session, &share, true).await.ok(),
        Some(Some("testuser".to_string()))
    );
    assert_eq!(session.reads(), 0);
    assert_eq!(share.mounts(), vec![Some("testuser".to_string())]);
}

/// The listing already read the store: the mount reads nothing more (hit or miss).
#[tokio::test]
async fn a_mount_after_the_listing_read_the_store_reads_nothing_more() {
    let hit = Session::new(Some(creds("testuser")));
    let server = Server::new(false);
    list(&hit, &server, Some("testuser")).await;
    assert_eq!(
        mount(&hit, &Share::new(false), true).await.ok(),
        Some(Some("testuser".to_string()))
    );
    assert_eq!(hit.reads(), 1, "one read between the listing and the mount");

    let miss = Session::new(None);
    list(&miss, &server, Some("testuser")).await;
    let share = Share::new(false);
    assert!(matches!(
        mount(&miss, &share, true).await,
        Err(MountError::AuthRequired { .. })
    ));
    assert_eq!(miss.reads(), 1, "a miss is remembered for the mount too");
    assert_eq!(share.mounts(), vec![None], "one mount, no retry with nothing to offer");
}

/// A guest share mounts as guest with no store read.
#[tokio::test]
async fn a_guest_mount_reads_nothing() {
    let session = Session::new(Some(creds("testuser")));
    let share = Share::new(true);

    assert_eq!(mount(&session, &share, false).await.ok(), Some(None));
    assert_eq!(session.reads(), 0);
}

/// A guest the share turns away costs the one store read and one more mount.
#[tokio::test]
async fn a_refused_guest_mount_tries_the_stored_password_once() {
    let session = Session::new(Some(creds("testuser")));
    let share = Share::new(false);

    assert_eq!(
        mount(&session, &share, false).await.ok(),
        Some(Some("testuser".to_string()))
    );
    assert_eq!(session.reads(), 1);
    assert_eq!(share.mounts(), vec![None, Some("testuser".to_string())]);
}
