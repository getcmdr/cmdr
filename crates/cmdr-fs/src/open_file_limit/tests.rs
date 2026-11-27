use super::*;

/// `RLIM_INFINITY` on macOS and Linux: a hard limit that names no cap.
const UNLIMITED: u64 = u64::MAX;

#[test]
fn a_dock_launched_macos_app_rises_to_the_target() {
    assert_eq!(soft_limit_target(256, UNLIMITED, Some(245_760)), TARGET);
}

#[test]
fn an_unlimited_hard_limit_is_held_to_macos_per_process_ceiling() {
    assert_eq!(soft_limit_target(256, UNLIMITED, Some(4_096)), 4_096);
}

#[test]
fn a_linux_session_rises_to_the_target_under_its_high_hard_limit() {
    assert_eq!(soft_limit_target(1_024, 524_288, None), TARGET);
}

#[test]
fn a_low_hard_limit_caps_the_raise() {
    assert_eq!(soft_limit_target(256, 2_048, None), 2_048);
    assert_eq!(soft_limit_target(256, 2_048, Some(245_760)), 2_048);
}

#[test]
fn a_soft_limit_already_above_the_target_is_never_lowered() {
    assert_eq!(soft_limit_target(65_536, UNLIMITED, Some(245_760)), 65_536);
    // Above the per-process ceiling too: the process keeps what it has.
    assert_eq!(soft_limit_target(8_192, 8_192, Some(4_096)), 8_192);
}

#[test]
fn a_soft_limit_at_the_hard_limit_stays() {
    assert_eq!(soft_limit_target(512, 512, None), 512);
}

/// The real process: after `raise`, the soft limit is at or above what it was, and at or above
/// the target wherever the hard limit allows it. nextest runs each test in its own process, so
/// the raise reaches no other test.
#[cfg(unix)]
#[test]
fn raising_the_real_limit_never_lowers_it() {
    fn soft_and_hard() -> (u64, u64) {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limit` is a valid, writable `rlimit` the call fills in.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) }, 0);
        (limit.rlim_cur, limit.rlim_max)
    }

    let (before, hard) = soft_and_hard();
    let outcome = raise();
    let (after, _) = soft_and_hard();

    assert!(after >= before, "{outcome:?}: {before} became {after}");
    assert!(!matches!(outcome, OpenFileLimit::Refused { .. }), "{outcome:?}");
    if hard >= TARGET && before < TARGET {
        assert_eq!(
            outcome,
            OpenFileLimit::Raised {
                from: before,
                to: after
            }
        );
    }
}
