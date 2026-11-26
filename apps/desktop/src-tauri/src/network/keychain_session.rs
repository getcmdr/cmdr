//! What this session may sign in to an SMB server as, past the store's own cache: the
//! account a person typed on a sheet (remembered or not), and the ONE store read each
//! server gets.
//!
//! ❗ **The store is read at most once per server per session**, hit or miss: each read
//! can raise a system prompt, so a share list's ⌘R or a second mount must not ask again.
//! A hit lands in the keychain's cache (`keychain::get_credentials`), so later reads
//! answer from memory; a miss is remembered here as spent. The listing and the mount
//! share this one memo. ❌ Nothing on a background path calls [`ReadOnce::read`]: only a
//! listing or a mount a person started.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::ignore_poison::IgnorePoison;

/// The one store read per key, whatever it found.
///
/// ❗ A turn lock PER KEY spans the read, so two callers racing for one server (a listing
/// and a mount) ask once: the second waits, then answers from what the first left in
/// memory. Per key, so one server's pending prompt (up to a minute) never holds up
/// another server's first read.
#[derive(Default)]
pub(crate) struct ReadOnce {
    spent: Mutex<HashSet<String>>,
    turns: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ReadOnce {
    /// What `key` signs in as: `known` (memory only) if it has an answer, else `read`,
    /// run only the first time `key` asks.
    pub(crate) fn read<T>(
        &self,
        key: &str,
        known: impl Fn() -> Option<T>,
        read: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        // The map lock is held only to find the key's turn, never across the read.
        let turn = Arc::clone(self.turns.lock_ignore_poison().entry(key.to_string()).or_default());
        let _turn = turn.lock_ignore_poison();
        if let Some(found) = known() {
            return Some(found);
        }
        if !self.spent.lock_ignore_poison().insert(key.to_string()) {
            return None;
        }
        read()
    }

    /// Whether `key`'s one read was spent. Tests and logs only.
    #[cfg(test)]
    pub(crate) fn is_spent(&self, key: &str) -> bool {
        self.spent.lock_ignore_poison().contains(key)
    }
}

#[cfg(test)]
#[path = "keychain_session_test.rs"]
mod tests;
