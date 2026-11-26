//! Tests for when a share listing tries guest.

use super::*;
use crate::network::server_identity::SmbServer;

const PORT: u16 = 10481;

/// A typed-account store holding `username` for `host` only.
fn typed_for(host: &'static str, username: &'static str) -> impl Fn(&SmbServer) -> Option<String> {
    move |server| (*server == SmbServer::new(host, PORT)).then(|| username.to_string())
}

fn nothing_typed(_: &SmbServer) -> Option<String> {
    None
}

#[test]
fn the_sheets_listing_tries_guest_only_when_no_account_is_typed() {
    assert_eq!(
        guest_attempt("testnas.local", Some("192.0.2.10"), PORT, nothing_typed),
        smb_client::GuestAttempt::Try
    );
    assert_eq!(
        guest_attempt(
            "testnas.local",
            Some("192.0.2.10"),
            PORT,
            typed_for("192.0.2.10", "test_user_1")
        ),
        smb_client::GuestAttempt::Skip
    );
}
