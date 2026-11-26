//! Whether an SMB share listing may try guest, and the account the person typed for a
//! host, from memory only. The sheet's listing asks here; a pane's share list climbs
//! `share_list_sign_in`'s ladder, which reads the typed account here too.

use crate::network::manual_servers;
use crate::network::smb_client;

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
