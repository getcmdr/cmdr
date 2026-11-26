//! Who a server's share list and its mounts sign in as, when the frontend sends no
//! credentials: the backend's half of "the frontend never holds a password".
//!
//! **A pane's share-list listing** ([`sign_in_and_list`]): a typed account (the one a
//! person set for the server) signs in with its session credential, else its stored
//! password, read ONCE this session, else not at all (`AuthRequired`, the pane's
//! sign-in). With no typed account it lists as guest, and only when the server turns the
//! guest away does it fall to the session credential, else the one stored read.
//!
//! **A mount with no credentials** ([`mount_signed_in`]): the session credential (the one
//! the listing signed in with, or a sheet's typed-not-remembered answer), else, for a
//! typed account, the one stored read up front; otherwise as guest, and only a guest the
//! server turns away (`AuthRequired`) costs the stored read and one more mount.
//!
//! ❗ The stored read is `keychain::read_server_credentials_once`, the ONE memo the
//! listing and the mount share, so a server is read at most once per session. ❌ Only a
//! pane listing and a person's mount reach it: prefetch and every non-pane reader stay
//! on `commands::network::account_listing_for`'s memory-only ladder.

use std::future::Future;

use cmdr_smb::{ShareListError, ShareListResult};

use crate::network::keychain::SmbCredentials;
use crate::network::os_mount::MountError;
use crate::network::share_list_path::KnownHosts;
use crate::network::smb_client::GuestAttempt;

/// The account a share list was listed as: `cmdr_fs`'s, since a Servers row's facts
/// (`cmdr_fs::server_meta::ServerMeta`) carry it too.
pub use cmdr_fs::server_meta::ShareListAccount;

/// What one pane listing came to, and the account it used, when that's known (a cached
/// list doesn't say who listed it).
pub(crate) struct Listed {
    pub result: Result<ShareListResult, ShareListError>,
    pub account: Option<ShareListAccount>,
}

/// Where a pane listing's sign-in comes from. `known` is memory only (the session
/// credential); `stored_once` is the one store read, a person's listing only.
pub(crate) struct Ladder<K, S> {
    /// The account the person set for this server, if any.
    pub typed_username: Option<String>,
    /// The session credential, from memory.
    pub known: K,
    /// The stored password, read at most once per session.
    pub stored_once: S,
}

/// Signs a pane's share-list listing in (see the module docs) and lists through `dial`,
/// which takes the credentials to offer and whether guest may be tried.
pub(crate) async fn sign_in_and_list<K, S, SF, D, DF>(ladder: Ladder<K, S>, dial: D) -> Listed
where
    K: Fn() -> Option<SmbCredentials>,
    S: Fn() -> SF,
    SF: Future<Output = Option<SmbCredentials>>,
    D: Fn(Option<SmbCredentials>, GuestAttempt) -> DF,
    DF: Future<Output = Result<ShareListResult, ShareListError>>,
{
    let Ladder {
        typed_username,
        known,
        stored_once,
    } = ladder;
    if let Some(username) = typed_username {
        // ❗ Only the typed account's own password: another account's would answer a
        // question the person didn't ask.
        let mine = |creds: &SmbCredentials| creds.username == username;
        let credentials = match known().filter(mine) {
            Some(creds) => Some(creds),
            None => stored_once().await.filter(mine),
        };
        return match credentials {
            Some(creds) => as_account(dial(Some(creds.clone()), GuestAttempt::Skip).await, &creds),
            // No password for it: the cache may still hold its listing, else
            // `AuthRequired`, the pane's sign-in.
            None => Listed {
                result: dial(None, GuestAttempt::Skip).await,
                account: None,
            },
        };
    }

    let guest = dial(None, GuestAttempt::Try).await;
    match guest {
        Ok(result) => {
            let account = (!result.from_cache).then_some(ShareListAccount::Guest);
            Listed {
                result: Ok(result),
                account,
            }
        }
        Err(refusal) if refusal.wants_sign_in() => {
            let credentials = match known() {
                Some(creds) => Some(creds),
                None => stored_once().await,
            };
            match credentials {
                Some(creds) => as_account(dial(Some(creds.clone()), GuestAttempt::Try).await, &creds),
                None => Listed {
                    result: Err(refusal),
                    account: None,
                },
            }
        }
        Err(other) => Listed {
            result: Err(other),
            account: None,
        },
    }
}

fn as_account(result: Result<ShareListResult, ShareListError>, creds: &SmbCredentials) -> Listed {
    Listed {
        account: result.is_ok().then(|| ShareListAccount::User {
            username: creds.username.clone(),
        }),
        result,
    }
}

/// Mounts with no credentials from the frontend (see the module docs), answering the
/// mount and the credentials it went through with (`None`: as guest), which the direct
/// connection and the saved share take next.
pub(crate) async fn mount_signed_in<T, K, S, SF, M, MF>(
    typed_account: bool,
    known: K,
    stored_once: S,
    mount: M,
) -> Result<(T, Option<SmbCredentials>), MountError>
where
    K: Fn() -> Option<SmbCredentials>,
    S: Fn() -> SF,
    SF: Future<Output = Option<SmbCredentials>>,
    M: Fn(Option<SmbCredentials>) -> MF,
    MF: Future<Output = Result<T, MountError>>,
{
    let credentials = match known() {
        Some(creds) => Some(creds),
        None if typed_account => stored_once().await,
        None => None,
    };
    match mount(credentials.clone()).await {
        Ok(mounted) => Ok((mounted, credentials)),
        // A guest the server turned away: the stored password, if this session hasn't
        // read it yet (the memo answers `None` without a second read if it has).
        Err(refusal @ MountError::AuthRequired { .. }) if credentials.is_none() => match stored_once().await {
            Some(creds) => mount(Some(creds.clone())).await.map(|mounted| (mounted, Some(creds))),
            None => Err(refusal),
        },
        Err(other) => Err(other),
    }
}

/// Mounts with the account a person just typed on the sheet, and files it as the server's
/// session credential under `names` ([`session_names`]) when the mount went through, or
/// when it stopped short of the account being judged (the server out of reach, the share
/// missing): the pane's Try again then mounts with no credentials, and the backend signs
/// in as it, so ❗ the frontend never keeps the password. ❌ Not when the server judged
/// the account (a refusal another credential answers) or the person called it off.
pub(crate) async fn mount_with_sheet_answer<T, M, MF>(
    offered: SmbCredentials,
    names: &[String],
    mount: M,
) -> Result<T, MountError>
where
    M: FnOnce(SmbCredentials) -> MF,
    MF: Future<Output = Result<T, MountError>>,
{
    let outcome = mount(offered.clone()).await;
    if sheet_answer_stands(&outcome) {
        for name in names {
            crate::network::keychain::remember_for_session(name, offered.clone());
        }
    }
    outcome
}

/// Whether a sheet's answer is still the account to sign in as after this mount. Every
/// variant spelled out, so a new refusal decides here.
fn sheet_answer_stands<T>(outcome: &Result<T, MountError>) -> bool {
    match outcome {
        Ok(_) => true,
        Err(
            MountError::AuthRequired { .. }
            | MountError::AuthFailed { .. }
            | MountError::PermissionDenied { .. }
            | MountError::Cancelled { .. },
        ) => false,
        Err(
            MountError::HostUnreachable { .. }
            | MountError::Timeout { .. }
            | MountError::SignInHelperStuck { .. }
            | MountError::ShareNotFound { .. }
            | MountError::UnsupportedProtocol { .. }
            | MountError::MountRefused { .. }
            | MountError::MountMissing { .. }
            | MountError::GvfsMissing
            | MountError::Unexpected { .. },
        ) => true,
    }
}

/// How long the one stored read may wait: a system prompt waits on a person. Past it the
/// listing or the mount goes on without the password (the sheet asks), and the read
/// still lands in memory for the next one.
const STORE_READ_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

/// [`crate::network::keychain::read_server_credentials_once`] on a blocking task, under
/// the name the frontend files a server's password by (its discovery `name`).
async fn stored_once(server: String) -> Option<SmbCredentials> {
    cmdr_fs::deadline::blocking_with_timeout(STORE_READ_LIMIT, None, move || {
        crate::network::keychain::read_server_credentials_once(&server)
    })
    .await
}

/// Every name a server's session credential is filed and looked up under: `name`, the one
/// the person knows it by (the discovery `name`, which a sheet saves under), then each
/// address a dial reached it by, ❗ on ITS port, or a server on 445 of the same machine
/// would sign in as this account.
///
/// ❗ The ONE spelling of that set: a sheet's listing and mount file under it, the pane's
/// listing and a mount with no credentials look up under it, so the two sides agree.
pub(crate) fn session_names<'a>(name: &str, dialed: impl IntoIterator<Item = &'a str>, port: u16) -> Vec<String> {
    let mut names = vec![name.to_string()];
    for address in dialed {
        let spelled = crate::network::server_identity::smb_server(address, port);
        if !names.contains(&spelled) {
            names.push(spelled);
        }
    }
    names
}

/// [`session_names`] for a mount of `server` (the address it dials) known as `host_name`
/// (the discovery name). ❗ With no `host_name`, the address on ITS port, ❌ never bare.
pub(crate) fn mount_session_names(server: &str, host_name: Option<&str>, port: u16) -> Vec<String> {
    let known_as = host_name
        .map(str::to_string)
        .unwrap_or_else(|| crate::network::server_identity::smb_server(server, port));
    session_names(&known_as, [server], port)
}

/// The names a sheet's answer for the share list listed as `host_id` is filed under: the
/// host that share list resolved ([`KnownHosts::by_id`], whichever list it came from: a
/// saved or a not-fresh host too), by [`session_names`]. A host no list knows any more
/// keeps the addresses the sheet dialed.
pub(crate) fn names_for_sheet_answer(
    hosts: &KnownHosts,
    host_id: &str,
    hostname: &str,
    ip: Option<&str>,
    port: u16,
) -> Vec<String> {
    let dialed = std::iter::once(hostname).chain(ip);
    match hosts.by_id(host_id) {
        Some(host) => session_names(&host.name, dialed, port),
        None => dialed
            .map(|address| crate::network::server_identity::smb_server(address, port))
            .collect(),
    }
}

/// The session credential under any name the server goes by ([`session_names`]).
fn known_under(names: &[String]) -> Option<SmbCredentials> {
    names
        .iter()
        .find_map(|name| crate::network::keychain::session_credentials(name))
}

/// A pane's listing of `host`'s share list: [`sign_in_and_list`] over the app's account
/// store, the session memo, and `smb_client::list_shares` (its cache, its default timeout
/// and TTL), with a refused account logged for the sign-in diagnostics.
pub(crate) async fn list_for_pane(app: &tauri::AppHandle, host: &crate::network::NetworkHost) -> Listed {
    use crate::network::smb_sign_in_diagnostics::{CredentialSource, log_sign_in_refusal};

    let hostname = host
        .hostname
        .clone()
        .unwrap_or_else(|| crate::network::service_name_to_hostname(&host.name));
    let ip = host.ip_address.as_deref();
    let names = session_names(&host.name, std::iter::once(hostname.as_str()).chain(ip), host.port);
    let typed_username = crate::network::smb_account_ladder::typed_account(&hostname, ip, host.port, |server| {
        crate::network::manual_servers::typed_username(app, server)
    });
    let ladder = Ladder {
        typed_username,
        known: || known_under(&names),
        stored_once: || stored_once(host.name.clone()),
    };
    sign_in_and_list(ladder, |credentials, guest| {
        let hostname = hostname.clone();
        async move {
            let offered = credentials.as_ref().map(|c| (c.username.as_str(), c.password.as_str()));
            let result =
                crate::network::smb_client::list_shares(&host.id, &hostname, ip, host.port, offered, guest, None, None)
                    .await;
            // The password is this session's copy of a saved entry or a sheet's answer.
            log_sign_in_refusal(&hostname, host.port, offered, CredentialSource::Saved, &result);
            result
        }
    })
    .await
}

/// Mounts `share` on `server` for a person who sent no credentials (see the module
/// docs), under the server's discovery name `host_name` (else `server`), on `port`.
pub(crate) async fn mount_with_session<T, M, MF>(
    app: &tauri::AppHandle,
    server: &str,
    host_name: Option<&str>,
    port: u16,
    mount: M,
) -> Result<(T, Option<SmbCredentials>), MountError>
where
    M: Fn(Option<SmbCredentials>) -> MF,
    MF: Future<Output = Result<T, MountError>>,
{
    use crate::network::server_identity::SmbServer;

    let name = host_name.unwrap_or(server).to_string();
    let names = mount_session_names(server, host_name, port);
    let typed_username = |identity: &SmbServer| crate::network::manual_servers::typed_username(app, identity);
    // `server` is where the mount dials (a hostname or an IP); the discovery name
    // spells its port off 445.
    let typed_account = crate::network::smb_account_ladder::typed_account(server, None, port, typed_username).is_some()
        || host_name.is_some_and(|name| typed_username(&SmbServer::from_name(name)).is_some());
    mount_signed_in(
        typed_account,
        || known_under(&names),
        || stored_once(name.clone()),
        mount,
    )
    .await
}

#[cfg(test)]
#[path = "share_list_sign_in_test.rs"]
mod tests;
