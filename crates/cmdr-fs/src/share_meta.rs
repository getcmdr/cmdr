//! What a server's share row says about its share, as data rather than as a
//! sentence.
//!
//! A share list (`smb://<server>` on the `network` volume) lists each share as a
//! folder. The server's own description of a share rides on
//! [`FileEntry::share_meta`](crate::entry::FileEntry::share_meta) as [`ShareMeta`],
//! and the frontend shows it in the Ext track, the way it shows a restic snapshot's
//! host ([`ResticSnapshotMeta`](crate::restic_meta::ResticSnapshotMeta)).
//!
//! ❌ Never add a field that carries a sentence of Cmdr's own. The description is the
//! server's text, shown as the server set it.

use serde::{Deserialize, Serialize};

/// The facts a server publishes about one of its shares, for the share's row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareMeta {
    /// The server's own description of the share (SMB's share comment), or `None`
    /// when it set none.
    pub description: Option<String>,
}
