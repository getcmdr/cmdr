//! The SMB password macOS saved for a server (Finder's "Remember this password in my
//! keychain"), for a share list that needs a sign-in Cmdr has none of its own for.
//!
//! Two reads, with different costs:
//!
//! - **The account** ([`account_for`]): an attribute-only lookup that raises no prompt, so the
//!   sign-in sheet pre-fills it and decides whether to offer "Use the password macOS saved".
//! - **The password** ([`read_once`]): a data read, which raises macOS's consent dialog
//!   (`secrets/system_keychain_smb.rs`). ❗ It takes a [`PersonAsked`], which only an IPC command
//!   a person's action sent can mint (`commands::network`), so a background thread, prefetch,
//!   upgrade sweep, or launch path has none to pass: a surprise Keychain prompt there would be
//!   awkward. ❗ At most once per server per run, a denial included (`SYSTEM_READS`), the rule
//!   Cmdr's own store keeps (`keychain_session.rs`).
//!
//! The listing that uses it is [`list_with`]: a typed account only ever signs in as itself, and
//! a listing that went through files the account as the server's session credential, so the
//! re-list and the mounts after it sign in with it and ask nothing.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, RwLock};

use cmdr_smb::{ShareListError, ShareListResult};
use serde::Serialize;

pub use crate::commands::network::PersonAsked;
use crate::ignore_poison::RwLockIgnorePoison as _;
use crate::network::NetworkHost;
use crate::network::keychain::SmbCredentials;
use crate::network::keychain_session::ReadOnce;

mod platform;
use platform::{candidates, saved_account, saved_password};

/// The one consent-gated read each server gets this run, hit or miss.
static SYSTEM_READS: LazyLock<ReadOnce> = LazyLock::new(ReadOnce::default);

/// What a read found, by server key, so a retry (the server didn't answer) asks nothing again.
static HITS: LazyLock<RwLock<HashMap<String, SmbCredentials>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

/// What the sign-in sheet may offer for a server: the account macOS saved, and whether the
/// password behind it may still be tried this run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemSavedAccount {
    /// The account macOS saved the password under. Pre-fills the sheet.
    pub username: String,
    /// ❗ `false` once this run asked macOS and got nothing usable (no password, a denial, or
    /// one the server turned down): the button would do nothing, so it isn't offered.
    pub can_try: bool,
}

/// The key one server's read is memoized under: its credential key (port kept off 445).
fn memo_key(host: &NetworkHost) -> String {
    crate::network::server_identity::credential_key(&host.name)
}

/// The account macOS saved for `host`, attribute-only (❌ no prompt), and whether its password
/// may still be tried this run. `None` off macOS, or when macOS saved nothing for it.
pub(crate) async fn account_for(host: &NetworkHost) -> Option<SystemSavedAccount> {
    let candidates = candidates(host);
    let username = tokio::task::spawn_blocking(move || saved_account(&candidates))
        .await
        .ok()
        .flatten()?;
    let key = memo_key(host);
    let can_try = !SYSTEM_READS.is_spent(&key) || HITS.read_ignore_poison().contains_key(&key);
    Some(SystemSavedAccount { username, can_try })
}

/// The password macOS saved for `host`, read at most once per server per run (a denial or a
/// miss included) on the blocking pool, waiting up to a minute on the consent dialog. `asked`
/// is the proof a person's action sent this; nothing else can make one.
pub(crate) async fn read_once(_asked: &PersonAsked, host: &NetworkHost) -> Option<SmbCredentials> {
    let key = memo_key(host);
    let candidates = candidates(host);
    cmdr_fs::deadline::blocking_with_timeout(READ_LIMIT, None, move || {
        SYSTEM_READS.read(
            &key,
            || HITS.read_ignore_poison().get(&key).cloned(),
            || {
                let found = saved_password(&candidates)?;
                HITS.write_ignore_poison().insert(key.clone(), found.clone());
                Some(found)
            },
        )
    })
    .await
}

/// Forgets a password the server turned down, so the sheet stops offering it.
pub(crate) fn drop_rejected(host: &NetworkHost) {
    HITS.write_ignore_poison().remove(&memo_key(host));
}

/// How long the consent dialog may wait on a person, as Cmdr's own store read does.
const READ_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

/// Lists with the password macOS saved: `Ok(None)` when there's nothing to try (none saved, a
/// denial, already asked this run, or saved for another account than the `typed_username` the
/// person set for the server). `account` is the prompt-free lookup, asked before the read so a
/// typed account never costs a prompt for someone else's password.
pub(crate) async fn list_with<A, R, RF, D, DF>(
    typed_username: Option<&str>,
    account: A,
    read: R,
    dial: D,
) -> Result<Option<ShareListResult>, ShareListError>
where
    A: FnOnce() -> Option<String>,
    R: FnOnce() -> RF,
    RF: Future<Output = Option<SmbCredentials>>,
    D: FnOnce(SmbCredentials) -> DF,
    DF: Future<Output = Result<ShareListResult, ShareListError>>,
{
    if let Some(typed) = typed_username
        && account().as_deref() != Some(typed)
    {
        return Ok(None);
    }
    let Some(credentials) = read().await else {
        return Ok(None);
    };
    if typed_username.is_some_and(|typed| typed != credentials.username) {
        return Ok(None);
    }
    dial(credentials).await.map(Some)
}

#[cfg(test)]
mod tests;
