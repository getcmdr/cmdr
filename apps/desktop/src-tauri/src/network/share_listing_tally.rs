//! How many times this session asked a server for its share list, per address: the
//! evidence #324's pins read ("opening the Servers list lists nobody's shares").
//!
//! [`smb_client::list_shares`](super::smb_client::list_shares) is the one door every share
//! listing goes through (a pane's share list, its sign-in ladder, the Servers view's prefetch,
//! the sign-in sheet's attempt), so it notes each ask here: `asked` for every call, `dialed`
//! for the ones the share cache didn't answer. Each ask counts under its hostname AND its
//! resolved IP (lowercased), so a spec finds it by whichever address it knows.
//!
//! Memory only, bounded by the number of servers the session talks to, and compiled into
//! tests and E2E builds only. The E2E reads it through `share_listing_asks` /
//! `reset_share_listing_asks` (`commands/e2e.rs`).

use crate::ignore_poison::IgnorePoison;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// One address's asks since the last reset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareListingAsks {
    /// Every `list_shares` call naming this address, cache hits included.
    pub asked: u64,
    /// The ones that went to the network: the cache had no answer.
    pub dialed: u64,
}

static TALLY: LazyLock<Mutex<HashMap<String, ShareListingAsks>>> = LazyLock::new(Default::default);

/// The addresses an ask names, lowercased and deduplicated (a host listed by its IP names it twice).
fn keys(hostname: &str, ip_address: Option<&str>) -> Vec<String> {
    let mut keys = vec![hostname.to_lowercase()];
    if let Some(ip) = ip_address.map(str::to_lowercase)
        && !keys.contains(&ip)
    {
        keys.push(ip);
    }
    keys
}

/// Notes one share-list ask; `dialed` says the cache didn't answer it.
pub(super) fn note(hostname: &str, ip_address: Option<&str>, dialed: bool) {
    let mut tally = TALLY.lock_ignore_poison();
    for key in keys(hostname, ip_address) {
        let asks = tally.entry(key).or_default();
        asks.asked += 1;
        if dialed {
            asks.dialed += 1;
        }
    }
}

/// The asks naming `address` (a hostname or an IP, any case) since the last [`reset`]; zeros
/// for one nobody asked about. A lookup, ❌ not the whole map: a server-keyed map never
/// crosses IPC (`network/CLAUDE.md`).
pub fn asks_for(address: &str) -> ShareListingAsks {
    TALLY
        .lock_ignore_poison()
        .get(&address.to_lowercase())
        .copied()
        .unwrap_or_default()
}

/// Starts counting from zero.
pub fn reset() {
    TALLY.lock_ignore_poison().clear();
}

#[cfg(test)]
#[path = "share_listing_tally_test.rs"]
mod tests;
