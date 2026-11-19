//! WPAD's discovery state, one per network. Discovery (DHCP and DNS lookups, then fetching the
//! script it names) can take seconds, or hang on a server that accepts and never answers, so it
//! never runs on a request's thread: the first ask on a network starts it in the background, and
//! every ask waits for its answer at most a bounded time, without holding any lock another host's
//! request needs. Pure, so every OS's tests run it; the Windows arm (`windows.rs`) supplies the
//! discovery and the network number.
#![cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    allow(dead_code, reason = "Windows' WPAD; on macOS and Linux only its tests use it")
)]

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a network's "no WPAD here" stands before an ask reruns discovery, in the background.
/// A WPAD server that was down when the network came up is found within this.
pub(crate) const RETRY_AFTER: Duration = Duration::from_secs(60);

/// One network's discovery: the PAC URL it found, or that it found none.
pub(crate) struct Wpad {
    state: Mutex<State>,
    landed: Condvar,
    retry_after: Duration,
}

struct State {
    /// The network the rest is about.
    network: u64,
    /// A discovery for this network is running.
    running: bool,
    /// What the last discovery on this network found (`None` inside: no usable WPAD), and when.
    /// `None` until one lands.
    answer: Option<(Option<String>, Instant)>,
}

impl State {
    const fn fresh(network: u64) -> Self {
        State {
            network,
            running: false,
            answer: None,
        }
    }
}

impl Wpad {
    pub(crate) const fn new(retry_after: Duration) -> Self {
        Wpad {
            state: Mutex::new(State::fresh(0)),
            landed: Condvar::new(),
            retry_after,
        }
    }

    /// The PAC URL WPAD found on `network`, or `None` (none there, or not found within `wait`).
    /// The first ask on a network calls `start`, which must start a discovery that ends in
    /// [`Wpad::finish`] for that network (on another thread, or at once when it can't start).
    pub(crate) fn pac_url(&self, network: u64, wait: Duration, start: impl FnOnce()) -> Option<String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.network != network {
            if network < state.network {
                // An ask about a network the machine has left: its answer would be stale.
                return None;
            }
            *state = State::fresh(network);
        }
        if let Some((answer, at)) = &state.answer {
            let answer = answer.clone();
            if answer.is_none() && !state.running && at.elapsed() >= self.retry_after {
                // Rerun in the background; this ask goes on without WPAD, as it would have.
                state.running = true;
                drop(state);
                start();
            }
            return answer;
        }
        if !state.running {
            state.running = true;
            drop(state);
            start();
            state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        }
        let (state, _) = self
            .landed
            .wait_timeout_while(state, wait, |state| state.network == network && state.answer.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        if state.network != network {
            return None;
        }
        state.answer.as_ref().and_then(|(answer, _)| answer.clone())
    }

    /// A discovery for `network` found `answer`. Dropped when the machine has moved on to another
    /// network since it started.
    pub(crate) fn finish(&self, network: u64, answer: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.network != network {
            return;
        }
        state.answer = Some((answer, Instant::now()));
        state.running = false;
        drop(state);
        self.landed.notify_all();
    }
}

#[cfg(test)]
#[path = "wpad_test.rs"]
mod tests;
