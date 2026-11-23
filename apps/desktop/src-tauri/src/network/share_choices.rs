//! A choice someone made about one SMB share, keyed by server + share: how a stored
//! choice names its share ([`ShareRef`]), how it's matched ([`names_share`]), and the
//! "Show “..” in the top folder" override rule ([`ParentRowOverride`]).
//!
//! ❗ A leaf: it imports no other `network` module but the identity helpers. Both the
//! store that persists these (`known_shares`) and the module that answers the UI about
//! them (`smb_share_parent`) depend on it, so neither has to depend on the other's
//! side. `smb_share_parent` living here once welded `known_shares`, `smb_share_parent`,
//! and `smb_direct_switch` into a `module-cycles` tangle.

use crate::network::NetworkHost;
use serde::{Deserialize, Serialize};

/// One share on one server, named the way the mount reported it. Matched with
/// [`names_share`], ❌ never compared as a key; the lists holding it stay private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareRef {
    pub(crate) server_name: String,
    pub(crate) share_name: String,
}

impl ShareRef {
    fn new(server_name: &str, share_name: &str) -> Self {
        Self {
            server_name: server_name.to_string(),
            share_name: share_name.to_string(),
        }
    }
}

/// Case- and NFC-folds one half of a share's name, the fold `known_shares::share_key` applies.
pub(crate) fn fold_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    name.nfc().flat_map(char::to_lowercase).collect()
}

/// Whether `entry` names the share `share` on the server any of `server_names` names.
///
/// The server half asks [`same_machine`](crate::network::server_identity::same_machine)
/// rather than comparing keys, because `statfs` echoes whichever name form each mount
/// used: one NAS is `192.168.50.10` on one mount and `Testnas._smb._tcp.local` on the
/// next, and a choice the other spelling can't see looks like the switch resetting
/// itself.
pub(crate) fn names_share(entry: &ShareRef, server_names: &[&str], share: &str, hosts: &[NetworkHost]) -> bool {
    use crate::network::server_identity::same_machine;

    fold_name(&entry.share_name) == fold_name(share)
        && server_names
            .iter()
            .any(|server| same_machine(&entry.server_name, server, hosts))
}

/// One share's own "Show “..”" choice, stored only while it differs from the default
/// (`smb_share_parent` has the whole rule).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParentRowOverride {
    #[serde(flatten)]
    share: ShareRef,
    show: bool,
}

/// The share's override in `overrides`, under any name its server goes by.
pub(crate) fn parent_row_override(
    overrides: &[ParentRowOverride],
    server: &str,
    share: &str,
    hosts: &[NetworkHost],
) -> Option<bool> {
    overrides
        .iter()
        .find(|entry| names_share(&entry.share, &[server], share, hosts))
        .map(|entry| entry.show)
}

/// Records the share's choice: every entry naming the share goes first (so a share
/// chosen under two spellings keeps one entry), and a new one is filed only when
/// `show` differs from `default_show`.
pub(crate) fn choose_parent_row(
    overrides: &mut Vec<ParentRowOverride>,
    server: &str,
    share: &str,
    show: bool,
    default_show: bool,
    hosts: &[NetworkHost],
) {
    overrides.retain(|entry| !names_share(&entry.share, &[server], share, hosts));
    if show != default_show {
        overrides.push(ParentRowOverride {
            share: ShareRef::new(server, share),
            show,
        });
    }
}

/// Copies the share's override from the server's old address to its new one, when the
/// new one has none yet: a moved server's share keeps its `..` choice
/// (`known_shares::move_host_rows_in`, which carries the direct-connection opt-outs the
/// same way). The old entry stays, since it names a machine another server may share.
pub(crate) fn carry_parent_row(
    overrides: &mut Vec<ParentRowOverride>,
    from_server: &str,
    to_server: &str,
    share: &str,
    hosts: &[NetworkHost],
) {
    let Some(show) = parent_row_override(overrides, from_server, share, hosts) else {
        return;
    };
    if parent_row_override(overrides, to_server, share, hosts).is_none() {
        overrides.push(ParentRowOverride {
            share: ShareRef::new(to_server, share),
            show,
        });
    }
}
