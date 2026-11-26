//! How an SMB share listing with no credentials from the frontend signs in, from memory
//! only: when it may try guest, and which session credential it offers. Prefetch, the
//! frontend's own listing, and the sheet's guest leg ask here; a pane's share list climbs
//! `share_list_sign_in`'s ladder, which may also read the store once.

use crate::network::keychain::{self, SmbCredentials};
use crate::network::manual_servers;
use crate::network::smb_client;

/// How a listing that got no credentials signs in: guest first, unless the person set
/// an account for this host, in which case as that account with its session credential
/// (`keychain::session_credentials`: a sheet's answer or a password this session already
/// read, ❌ never a Keychain read), or not at all (`AuthRequired`). The memory-only ladder
/// of prefetch and the frontend's own listing; a pane's share list climbs
/// `network::share_list_sign_in`'s, which may read the store once.
///
/// ❗ Without the cached leg, "Sign in as…" didn't stick: the next background listing
/// had no password to offer and came back as nothing, where it used to come back guest.
pub(crate) fn account_listing_for(
    app: &tauri::AppHandle,
    hostname: &str,
    ip_address: Option<&str>,
    port: u16,
) -> (smb_client::GuestAttempt, Option<SmbCredentials>) {
    listing_ladder(
        hostname,
        ip_address,
        port,
        |server| manual_servers::typed_username(app, server),
        keychain::session_credentials,
    )
}

/// [`account_listing_for`]'s decision, over its two lookups: the account the person
/// typed for a server, and this session's cached password for a credential key.
///
/// ❗ `cached_password` is the ONLY credential source it's handed, so the ladder has no
/// way to reach the Keychain; and it's asked only once an account is typed.
pub(crate) fn listing_ladder(
    hostname: &str,
    ip_address: Option<&str>,
    port: u16,
    typed_username: impl Fn(&crate::network::server_identity::SmbServer) -> Option<String>,
    cached_password: impl Fn(&str) -> Option<SmbCredentials>,
) -> (smb_client::GuestAttempt, Option<SmbCredentials>) {
    use crate::network::server_identity::smb_server;

    let Some(username) = typed_account(hostname, ip_address, port, typed_username) else {
        return (smb_client::GuestAttempt::Try, None);
    };
    let cached = std::iter::once(hostname)
        .chain(ip_address)
        .find_map(|name| cached_password(&smb_server(name, port)))
        .filter(|creds| creds.username == username);
    (smb_client::GuestAttempt::Skip, cached)
}

/// Whether a listing of this host may try guest: not when the person typed an
/// account for it (`manual_servers::typed_username`), by either name it goes by.
pub(crate) fn guest_attempt_for(
    app: &tauri::AppHandle,
    hostname: &str,
    ip_address: Option<&str>,
    port: u16,
) -> smb_client::GuestAttempt {
    guest_attempt(hostname, ip_address, port, |server| {
        manual_servers::typed_username(app, server)
    })
}

/// [`guest_attempt_for`]'s decision, over the typed-account lookup.
fn guest_attempt(
    hostname: &str,
    ip_address: Option<&str>,
    port: u16,
    typed_username: impl Fn(&crate::network::server_identity::SmbServer) -> Option<String>,
) -> smb_client::GuestAttempt {
    match typed_account(hostname, ip_address, port, typed_username) {
        Some(_) => smb_client::GuestAttempt::Skip,
        None => smb_client::GuestAttempt::Try,
    }
}

/// The account the person typed for this host, by its hostname first, then its IP.
pub(crate) fn typed_account(
    hostname: &str,
    ip_address: Option<&str>,
    port: u16,
    typed_username: impl Fn(&crate::network::server_identity::SmbServer) -> Option<String>,
) -> Option<String> {
    use crate::network::server_identity::SmbServer;

    typed_username(&SmbServer::new(hostname, port))
        .or_else(|| ip_address.and_then(|ip| typed_username(&SmbServer::new(ip, port))))
}

#[cfg(test)]
#[path = "smb_account_ladder_test.rs"]
mod tests;
