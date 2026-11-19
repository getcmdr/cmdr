//! The real Windows calls. They read this machine's settings and never change them: the routing
//! rules themselves are `ie_settings_test.rs`'s, and the PAC runner's are `crate::pac`'s.

use std::time::Instant;

use super::*;

/// Reading the settings works for the signed-in user, whatever they hold.
#[test]
fn this_users_internet_settings_read() {
    let settings = current_settings().expect("WinHttpGetIEProxyConfigForCurrentUser answers");
    // A builder's settings are its own; only their shape is the subject.
    let _ = settings;
}

/// reqwest asks once per new connection, so the read must stay cheap.
#[test]
fn reading_the_settings_is_cheap_enough_to_do_per_connection() {
    let _ = current_settings();
    let started = Instant::now();
    for _ in 0..100 {
        let _ = current_settings();
    }
    let each = started.elapsed() / 100;
    assert!(each.as_millis() < 5, "{each:?} a read");
}
