use super::*;

// The tally is process-wide. nextest runs each test in its own process; each test still names
// its own hosts, so only the reset test could disturb a sibling under a shared-process runner.

#[test]
fn an_ask_counts_under_its_hostname_and_its_ip_in_any_case() {
    note("TallyNas-1.local", Some("10.0.0.41"), true);

    let expected = ShareListingAsks { asked: 1, dialed: 1 };
    assert_eq!(asks_for("tallynas-1.local"), expected);
    assert_eq!(asks_for("TALLYNAS-1.LOCAL"), expected);
    assert_eq!(asks_for("10.0.0.41"), expected);
}

#[test]
fn a_cache_answer_is_asked_but_not_dialed() {
    note("tallynas-2", None, true);
    note("tallynas-2", None, false);

    assert_eq!(asks_for("tallynas-2"), ShareListingAsks { asked: 2, dialed: 1 });
}

#[test]
fn a_host_listed_by_its_ip_counts_once() {
    note("10.0.0.43", Some("10.0.0.43"), true);

    assert_eq!(asks_for("10.0.0.43"), ShareListingAsks { asked: 1, dialed: 1 });
}

#[test]
fn a_server_nobody_asked_about_reads_zero_and_a_reset_forgets_every_ask() {
    assert_eq!(asks_for("tallynas-never"), ShareListingAsks::default());

    note("tallynas-4", None, true);
    reset();
    assert_eq!(asks_for("tallynas-4"), ShareListingAsks::default());
}
