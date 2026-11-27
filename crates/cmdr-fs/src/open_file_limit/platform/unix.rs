//! The POSIX `getrlimit` / `setrlimit` raise both Unixes share; each arm passes its own ceiling.

use super::super::{OpenFileLimit, soft_limit_target};

fn last_errno() -> Option<i32> {
    std::io::Error::last_os_error().raw_os_error()
}

/// Raises the soft `RLIMIT_NOFILE` to [`soft_limit_target`] under `per_process_max`.
pub(super) fn raise_within(per_process_max: Option<u64>) -> OpenFileLimit {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid, writable `rlimit` the call fills in.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return OpenFileLimit::Refused {
            current: None,
            target: None,
            errno: last_errno(),
        };
    }
    // `rlim_t` is `u64` on every Unix Cmdr builds for; these typed bindings stop the build where
    // it isn't, so no cast can silently truncate.
    let current: u64 = limit.rlim_cur;
    let hard: u64 = limit.rlim_max;
    let target = soft_limit_target(current, hard, per_process_max);
    if target <= current {
        return OpenFileLimit::AlreadyEnough { current };
    }
    limit.rlim_cur = target;
    // SAFETY: `limit` is a valid `rlimit` whose soft value is at or under its hard one and the
    // OS's per-process ceiling, the only values the call accepts; a refusal is an errno, not UB.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
        return OpenFileLimit::Refused {
            current: Some(current),
            target: Some(target),
            errno: last_errno(),
        };
    }
    OpenFileLimit::Raised {
        from: current,
        to: target,
    }
}
