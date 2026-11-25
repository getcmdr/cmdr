//! The log level for a share's failed space query.
//!
//! The space poller asks every 5 s for as long as a pane shows the share, so a
//! share whose server refuses the query (an NTSTATUS answer, not a lost session)
//! would repeat the same WARN ~720 times an hour. It warns once, then logs at
//! DEBUG until the share answers again. Classified by the typed
//! `smb2::Error::Protocol` variant, ❌ never by the status wording.

use log::Level;
use std::sync::atomic::{AtomicBool, Ordering};

/// Remembers, per share, whether its refusal of the space query was already
/// logged at WARN.
pub(super) struct SpaceRefusals {
    warned: AtomicBool,
}

impl SpaceRefusals {
    pub(super) const fn new() -> Self {
        Self {
            warned: AtomicBool::new(false),
        }
    }

    /// The level `result`'s failure logs at. A refusal warns the first time
    /// and drops to DEBUG after; any other failure always warns. An answer
    /// re-arms the warning, so a share that starts refusing again says so.
    pub(super) fn level_for<T>(&self, result: &Result<T, smb2::Error>) -> Level {
        match result {
            Ok(_) => {
                self.warned.store(false, Ordering::Relaxed);
                Level::Warn
            }
            Err(smb2::Error::Protocol { .. }) if self.warned.swap(true, Ordering::Relaxed) => Level::Debug,
            Err(_) => Level::Warn,
        }
    }
}

#[cfg(test)]
#[path = "space_refusal_test.rs"]
mod space_refusal_test;
