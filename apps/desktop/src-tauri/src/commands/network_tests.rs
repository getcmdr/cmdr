//! Tests for the share listing's account ladder: when a listing tries guest, and
//! which password a listing with no credentials signs in with.

use super::*;
use crate::network::server_identity::SmbServer;
use std::cell::Cell;

const PORT: u16 = 10481;

fn creds(username: &str) -> SmbCredentials {
    SmbCredentials {
        username: username.to_string(),
        password: "testpass".to_string(),
    }
}

/// A typed-account store holding `username` for `host` only.
fn typed_for(host: &'static str, username: &'static str) -> impl Fn(&SmbServer) -> Option<String> {
    move |server| (*server == SmbServer::new(host, PORT)).then(|| username.to_string())
}

fn nothing_typed(_: &SmbServer) -> Option<String> {
    None
}

#[test]
fn no_typed_account_lists_as_guest_first_and_never_asks_for_a_password() {
    let asked = Cell::new(false);
    let (guest, account) = listing_ladder("testnas.local", Some("192.0.2.10"), PORT, nothing_typed, |_| {
        asked.set(true);
        Some(creds("test_user_1"))
    });
    assert_eq!(guest, smb_client::GuestAttempt::Try);
    assert!(account.is_none());
    assert!(
        !asked.get(),
        "a host with no typed account must not look for a password"
    );
}

#[test]
fn a_typed_account_skips_guest_and_signs_in_with_the_session_cached_password() {
    let (guest, account) = listing_ladder(
        "testnas.local",
        None,
        PORT,
        typed_for("testnas.local", "test_user_1"),
        |key| (key == "testnas.local:10481").then(|| creds("test_user_1")),
    );
    assert_eq!(guest, smb_client::GuestAttempt::Skip);
    assert_eq!(account.map(|c| c.username).as_deref(), Some("test_user_1"));
}

#[test]
fn a_typed_account_with_no_cached_password_skips_guest_and_offers_nothing() {
    // `AuthRequired` follows, which the frontend takes to the Keychain and the sheet.
    let (guest, account) = listing_ladder(
        "testnas.local",
        None,
        PORT,
        typed_for("testnas.local", "test_user_1"),
        |_| None,
    );
    assert_eq!(guest, smb_client::GuestAttempt::Skip);
    assert!(account.is_none());
}

#[test]
fn a_cached_password_for_another_account_is_not_used_and_guest_stays_off() {
    let (guest, account) = listing_ladder(
        "testnas.local",
        None,
        PORT,
        typed_for("testnas.local", "test_user_1"),
        |_| Some(creds("test_user_2")),
    );
    assert_eq!(guest, smb_client::GuestAttempt::Skip);
    assert!(account.is_none());
}

#[test]
fn an_account_typed_against_the_ip_counts_and_its_password_is_found_by_ip() {
    let (guest, account) = listing_ladder(
        "testnas.local",
        Some("192.0.2.10"),
        PORT,
        typed_for("192.0.2.10", "test_user_1"),
        |key| (key == "192.0.2.10:10481").then(|| creds("test_user_1")),
    );
    assert_eq!(guest, smb_client::GuestAttempt::Skip);
    assert_eq!(account.map(|c| c.username).as_deref(), Some("test_user_1"));
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
