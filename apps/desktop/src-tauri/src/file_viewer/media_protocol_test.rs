//! Unit tests for the `cmdr-media://` scheme's pure helpers: token parsing and the
//! `Range` math matrix. The Tauri glue (`handle_request`) is the only un-testable part
//! and stays a thin shell over these.

use super::media_protocol::{RangeOutcome, ResolvedRange, parse_token_from_uri, resolve_range};

fn serve(start: u64, end: u64, total: u64, is_partial: bool) -> RangeOutcome {
    RangeOutcome::Serve(ResolvedRange {
        start,
        end,
        total,
        is_partial,
    })
}

// --- token parsing ---

#[test]
fn parses_token_from_path() {
    assert_eq!(parse_token_from_uri("/deadbeef"), Some("deadbeef"));
    assert_eq!(parse_token_from_uri("/deadbeef/ignored"), Some("deadbeef"));
}

/// macOS and Linux load `cmdr-media://localhost/<token>`, WebView2 `http://cmdr-media.localhost/<token>`
/// (`mediaUrl` via `convertFileSrc`). The handler reads only the path, so both carry the token.
#[test]
fn every_webview_url_form_carries_the_token() {
    for url in [
        "cmdr-media://localhost/deadbeef",
        "http://cmdr-media.localhost/deadbeef",
    ] {
        let uri: tauri::http::Uri = url.parse().expect("a media URL parses");
        assert_eq!(parse_token_from_uri(uri.path()), Some("deadbeef"), "{url}");
    }
}

#[test]
fn empty_path_has_no_token() {
    assert_eq!(parse_token_from_uri("/"), None);
    assert_eq!(parse_token_from_uri(""), None);
}

// --- no range -> full 200 ---

#[test]
fn no_range_serves_full_file_as_200() {
    assert_eq!(resolve_range(None, 1000), serve(0, 999, 1000, false));
}

#[test]
fn unknown_unit_serves_full_file() {
    // Only `bytes=` is understood; anything else degrades to the whole file, never an error.
    assert_eq!(resolve_range(Some("items=0-10"), 1000), serve(0, 999, 1000, false));
}

#[test]
fn empty_file_no_range_serves_200_zero_length() {
    let outcome = resolve_range(None, 0);
    assert_eq!(outcome, serve(0, 0, 0, false));
    if let RangeOutcome::Serve(r) = outcome {
        assert!(r.is_empty());
    }
}

// --- explicit ranges -> 206 ---

#[test]
fn closed_range_is_inclusive_206() {
    assert_eq!(resolve_range(Some("bytes=0-499"), 1000), serve(0, 499, 1000, true));
    assert_eq!(resolve_range(Some("bytes=500-999"), 1000), serve(500, 999, 1000, true));
}

#[test]
fn open_ended_range_runs_to_eof() {
    assert_eq!(resolve_range(Some("bytes=500-"), 1000), serve(500, 999, 1000, true));
}

#[test]
fn end_past_eof_is_clamped() {
    // End clamps to total-1; still a satisfiable 206.
    assert_eq!(resolve_range(Some("bytes=0-99999"), 1000), serve(0, 999, 1000, true));
    assert_eq!(
        resolve_range(Some("bytes=900-99999"), 1000),
        serve(900, 999, 1000, true)
    );
}

#[test]
fn suffix_range_serves_last_n_bytes() {
    assert_eq!(resolve_range(Some("bytes=-100"), 1000), serve(900, 999, 1000, true));
    // Suffix larger than the file: the whole file (clamped), still partial.
    assert_eq!(resolve_range(Some("bytes=-5000"), 1000), serve(0, 999, 1000, true));
}

#[test]
fn whitespace_around_spec_is_tolerated() {
    assert_eq!(resolve_range(Some("bytes= 0-499 "), 1000), serve(0, 499, 1000, true));
}

#[test]
fn first_range_of_a_list_is_used() {
    assert_eq!(
        resolve_range(Some("bytes=0-99,200-299"), 1000),
        serve(0, 99, 1000, true)
    );
}

// --- unsatisfiable -> 416 ---

#[test]
fn start_past_eof_is_unsatisfiable() {
    assert_eq!(
        resolve_range(Some("bytes=1000-1100"), 1000),
        RangeOutcome::Unsatisfiable { total: 1000 }
    );
    assert_eq!(
        resolve_range(Some("bytes=5000-"), 1000),
        RangeOutcome::Unsatisfiable { total: 1000 }
    );
}

#[test]
fn reversed_range_is_unsatisfiable() {
    assert_eq!(
        resolve_range(Some("bytes=500-100"), 1000),
        RangeOutcome::Unsatisfiable { total: 1000 }
    );
}

#[test]
fn any_range_on_empty_file_is_unsatisfiable() {
    assert_eq!(
        resolve_range(Some("bytes=0-0"), 0),
        RangeOutcome::Unsatisfiable { total: 0 }
    );
    assert_eq!(
        resolve_range(Some("bytes=-10"), 0),
        RangeOutcome::Unsatisfiable { total: 0 }
    );
}

#[test]
fn malformed_specs_degrade_to_full_file() {
    // Non-numeric -> serve the whole file rather than erroring.
    assert_eq!(resolve_range(Some("bytes=abc-def"), 1000), serve(0, 999, 1000, false));
    assert_eq!(resolve_range(Some("bytes=10-xyz"), 1000), serve(0, 999, 1000, false));
    assert_eq!(resolve_range(Some("bytes=nodash"), 1000), serve(0, 999, 1000, false));
}

// --- resolved-range arithmetic ---

#[test]
fn resolved_range_len_is_inclusive() {
    let r = ResolvedRange {
        start: 0,
        end: 499,
        total: 1000,
        is_partial: true,
    };
    assert_eq!(r.len(), 500);
    let single = ResolvedRange {
        start: 7,
        end: 7,
        total: 1000,
        is_partial: true,
    };
    assert_eq!(single.len(), 1);
}

// --- deferred tokens ---

mod deferred {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use tauri::http::StatusCode;

    use crate::file_viewer::content_kind::ViewerContentKind;
    use crate::file_viewer::media::{DeferredMedia, MediaEntry};
    use crate::file_viewer::media_protocol::{FetchFailure, fetch_deferred};

    struct Fake {
        answer: Option<MediaEntry>,
        idle: Duration,
        gave_up: AtomicBool,
    }

    impl DeferredMedia for Fake {
        fn fetch(&self) -> Option<MediaEntry> {
            if self.idle > Duration::ZERO {
                // A fetch that never finishes on its own: it waits to be told to stop.
                while !self.gave_up.load(Ordering::Relaxed) {
                    std::thread::yield_now();
                }
                return None;
            }
            self.answer.clone()
        }
        fn idle_for(&self) -> Duration {
            self.idle
        }
        fn give_up(&self) {
            self.gave_up.store(true, Ordering::Relaxed);
        }
    }

    fn fake(answer: Option<MediaEntry>, idle: Duration) -> Arc<Fake> {
        Arc::new(Fake {
            answer,
            idle,
            gave_up: AtomicBool::new(false),
        })
    }

    #[tokio::test]
    async fn a_fetched_entry_is_served() {
        let entry = MediaEntry {
            canonical_path: PathBuf::from("/tmp/a.png"),
            kind: ViewerContentKind::Image,
            mime: "image/png".to_string(),
        };
        let fetched = fetch_deferred(fake(Some(entry), Duration::ZERO), Duration::from_secs(5))
            .await
            .expect("a fetch that answers is served");
        assert_eq!(fetched.canonical_path, PathBuf::from("/tmp/a.png"));
    }

    #[tokio::test]
    async fn a_fetch_with_nothing_to_serve_is_a_404() {
        let failure = fetch_deferred(fake(None, Duration::ZERO), Duration::from_secs(5))
            .await
            .expect_err("nothing to serve");
        assert_eq!(failure, FetchFailure::NothingToServe);
        assert_eq!(failure.response().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_stalled_fetch_is_given_up_and_answers_504() {
        let source = fake(None, Duration::from_secs(60));
        let failure = fetch_deferred(Arc::clone(&source) as Arc<dyn DeferredMedia>, Duration::from_millis(1))
            .await
            .expect_err("a stalled fetch answers");
        assert_eq!(failure, FetchFailure::Stalled);
        assert_eq!(failure.response().status(), StatusCode::GATEWAY_TIMEOUT);
        assert!(source.gave_up.load(Ordering::Relaxed), "the fetch is told to stop");
    }
}
