//! The cache, the budgets, and the timeouts, against the real OS runner (CFNetwork on macOS,
//! WinHTTP on Windows) and a PAC file served on loopback.

use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::test_server::{SCRIPT, pac_server, pac_server_after, url};
use super::*;

/// What the OS makes of [`SCRIPT`]'s `PROXY proxy.test:3128; DIRECT`.
fn via_proxy() -> Vec<Entry> {
    vec![
        Entry::Http {
            host: "proxy.test".into(),
            port: 3128,
            credentials: None,
        },
        Entry::Direct,
    ]
}

#[test]
fn a_pac_file_routes_each_host_its_own_way() {
    let pac = pac_server(Some(SCRIPT));
    let evaluator = OsPac::new(ANSWER_BUDGET);
    let source = PacSource::Url(pac);
    assert_eq!(
        evaluator.evaluate(&source, &url("https://example.com")),
        Some(via_proxy())
    );
    assert_eq!(
        evaluator.evaluate(&source, &url("https://other.test")),
        Some(vec![Entry::Direct])
    );
}

#[test]
fn an_answer_is_cached_per_host() {
    let pac = pac_server(Some(SCRIPT));
    let evaluator = OsPac::new(ANSWER_BUDGET);
    let source = PacSource::Url(pac);
    // Judged by the evaluations run, ❌ never by the server's hits: one evaluation can take
    // CFNetwork more than one fetch (three, on macOS 27.0), and a late one can land after the
    // answer is back, which raced the old count in CI.
    for _ in 0..3 {
        assert_eq!(
            evaluator.evaluate(&source, &url("https://example.com")),
            Some(via_proxy())
        );
    }
    assert_eq!(evaluator.evaluations.load(Ordering::SeqCst), 1);
    // Per host: another one runs the PAC for itself.
    assert_eq!(
        evaluator.evaluate(&source, &url("https://other.test")),
        Some(vec![Entry::Direct])
    );
    assert_eq!(evaluator.evaluations.load(Ordering::SeqCst), 2);
}

#[test]
fn an_unreachable_pac_fails_without_waiting_out_the_timeout() {
    let closed = TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("an address");
    let evaluator = OsPac::new(TIMEOUT);
    let started = Instant::now();
    let answer = evaluator.evaluate(
        &PacSource::Url(format!("http://{closed}/proxy.pac")),
        &url("https://example.com"),
    );
    assert_eq!(answer, None);
    assert!(started.elapsed() < TIMEOUT, "took {:?}", started.elapsed());
}

#[test]
fn a_pac_host_that_never_answers_is_given_up_on() {
    let pac = pac_server(None);
    let evaluator = OsPac::new(Duration::from_millis(300));
    let started = Instant::now();
    assert_eq!(
        evaluator.evaluate(&PacSource::Url(pac), &url("https://example.com")),
        None
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(300) && took < Duration::from_secs(2),
        "took {took:?}"
    );
}

/// A PAC host slower than [`SHORT_BUDGET`], but well inside a warm-up's.
const SLOW_PAC: Duration = Duration::from_millis(600);
const SHORT_BUDGET: Duration = Duration::from_millis(300);

#[test]
fn a_warm_up_waits_past_the_request_budget() {
    let pac = pac_server_after(Some(SCRIPT), SLOW_PAC);
    let evaluator = OsPac::new(SHORT_BUDGET);
    let started = Instant::now();
    assert_eq!(
        evaluator
            .with_budget(ANSWER_BUDGET)
            .evaluate(&PacSource::Url(pac), &url("https://example.com")),
        Some(via_proxy())
    );
    assert!(started.elapsed() > SHORT_BUDGET, "took {:?}", started.elapsed());
}

#[test]
fn a_request_during_the_warm_up_joins_it() {
    let pac = pac_server_after(Some(SCRIPT), SLOW_PAC);
    let evaluator = Arc::new(OsPac::new(SHORT_BUDGET));
    let source = PacSource::Url(pac);
    let warming = {
        let (evaluator, source) = (evaluator.clone(), source.clone());
        std::thread::spawn(move || {
            evaluator
                .with_budget(ANSWER_BUDGET)
                .evaluate(&source, &url("https://example.com"))
        })
    };
    // The warm-up holds the evaluation turn from the moment it counts itself.
    while evaluator.evaluations.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }
    // On its own, this request's budget would run out on the slow PAC and go direct.
    assert_eq!(
        evaluator.evaluate(&source, &url("https://example.com")),
        Some(via_proxy())
    );
    assert_eq!(warming.join().expect("the warm-up thread"), Some(via_proxy()));
    assert_eq!(evaluator.evaluations.load(Ordering::SeqCst), 1);
}
