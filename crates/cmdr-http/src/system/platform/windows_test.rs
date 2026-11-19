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

/// What launch pays for the system layer on this machine's real settings, in a fresh process:
/// how long `warm_up_pac` holds the caller, then the first routing of five hosts at once (the
/// app's launch burst: license, update check, telemetry, and two more), then a warm one. A
/// measurement, not a check: run it by hand with `--ignored --nocapture` and compare commits.
#[test]
#[ignore = "a measurement for the builder, run by hand"]
fn measure_launch_routing() {
    let started = Instant::now();
    crate::warm_up_pac("https://api.getcmdr.com");
    let warm_up_call = started.elapsed();
    let hosts = [
        "api.getcmdr.com",
        "getcmdr.com",
        "example.com",
        "example.org",
        "example.net",
    ];
    let burst = Instant::now();
    let each: Vec<_> = hosts
        .iter()
        .map(|host| {
            let url = Url::parse(&format!("https://{host}")).expect("a test URL");
            std::thread::spawn(move || {
                let asked = Instant::now();
                let route = route(&url);
                (asked.elapsed(), route)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|thread| thread.join().expect("a routing thread"))
        .collect();
    let burst_took = burst.elapsed();
    let warm = Instant::now();
    let _ = route(&Url::parse("https://example.com").expect("a test URL"));
    println!(
        "settings: {:?}\nwarm_up_pac call: {warm_up_call:?}\nburst of {}: {burst_took:?}, each {:?}\nwarm route: {:?}",
        current_settings(),
        hosts.len(),
        each.iter()
            .map(|(took, route)| format!("{took:?} {route:?}"))
            .collect::<Vec<_>>(),
        warm.elapsed()
    );
}
