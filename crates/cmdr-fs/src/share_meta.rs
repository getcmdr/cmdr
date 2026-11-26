//! What a server's share row says about its share, as data rather than as a
//! sentence.
//!
//! A share list (`smb://<server>` on the `network` volume) lists each share as a
//! folder. The server's own description of a share rides on
//! [`FileEntry::share_meta`](crate::entry::FileEntry::share_meta) as [`ShareMeta`],
//! and the frontend shows it in the Ext track, the way it shows a restic snapshot's
//! host ([`ResticSnapshotMeta`](crate::restic_meta::ResticSnapshotMeta)).
//!
//! A share the person saved (an SMB share Cmdr mounted, `known_shares`) also carries
//! [`SavedShareMeta`]: its place, how live it is, its account, and its pin, so the row
//! draws a status dot and offers Pin and Forget share.
//!
//! ❌ Never add a field that carries a sentence of Cmdr's own. The description is the
//! server's text, shown as the server set it.

use serde::{Deserialize, Serialize};

use crate::server_meta::{ServerStatus, ShareListAccount};

/// The facts a server publishes about one of its shares, for the share's row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareMeta {
    /// The server's own description of the share (SMB's share comment), or `None`
    /// when it set none.
    pub description: Option<String>,
    /// The person's saved place for this share, or `None` for a share they never saved.
    pub saved: Option<SavedShareMeta>,
}

/// What a saved share's row knows about its place, from memory and the mount table only
/// (❌ never the Keychain, ❌ never a dial).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedShareMeta {
    /// The saved place's volume id: what Enter, Pin, and Forget share act on.
    pub volume_id: String,
    /// [`ServerStatus::Connected`] while the share is mounted (Cmdr's mount or the OS's),
    /// else [`ServerStatus::Saved`].
    pub status: ServerStatus,
    /// The account the live mount signed in as while it's connected, else the one its
    /// next mount uses; `None` when nothing known says.
    pub account: Option<ShareListAccount>,
    /// Whether the place is pinned to the switcher.
    pub pinned: bool,
}
