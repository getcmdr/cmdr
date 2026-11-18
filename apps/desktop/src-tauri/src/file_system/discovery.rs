//! Whether the startup sweep (`volume-init`) has registered every volume it discovered.
//!
//! Until then the registry holds `root` alone, so an answer that depends on which ids are
//! registered (a restored tab's saved-root rebase) is an answer about an incomplete
//! registry. Such a caller waits here, bounded, or says it couldn't tell.

use std::sync::{Condvar, LazyLock, Mutex};
use std::time::Duration;

use crate::ignore_poison::IgnorePoison;

static DONE: LazyLock<(Mutex<bool>, Condvar)> = LazyLock::new(|| (Mutex::new(false), Condvar::new()));

/// The sweep finished: every discovered volume is registered.
pub(crate) fn mark_done() {
    let (done, ready) = &*DONE;
    *done.lock_ignore_poison() = true;
    ready.notify_all();
}

/// Block until the sweep has finished, or `limit` passed. `true` when it finished. Call it
/// off the IPC and main threads: a hung mount can hold the sweep for many seconds.
pub(crate) fn wait_until_done(limit: Duration) -> bool {
    let (done, ready) = &*DONE;
    let guard = done.lock_ignore_poison();
    let (guard, _) = ready
        .wait_timeout_while(guard, limit, |finished| !*finished)
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard
}
