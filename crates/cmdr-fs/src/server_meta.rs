//! What a server's row on the Servers list says about its server, as data rather
//! than as a sentence.
//!
//! The Servers list (`smb://` on the `network` volume) lists one folder row per
//! server. The facts the row's Details cell, status dot, and row actions read ride on
//! [`FileEntry::server_meta`](crate::entry::FileEntry::server_meta) as [`ServerMeta`],
//! the way a share row's description rides on
//! [`ShareMeta`](crate::share_meta::ShareMeta). The app decides the rows
//! (`apps/desktop/src-tauri/src/network/servers_listing.rs`); the frontend words them.
//!
//! ❌ Never add a field that carries a sentence of Cmdr's own: the status is an enum,
//! the account is data, and the frontend owns every word.

use serde::{Deserialize, Serialize};

/// Which protocol a saved server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServerProtocol {
    /// An SMB host. ❗ The host row is never pinned; its saved shares are.
    Smb,
    /// An SFTP server, one account per entry.
    Sftp,
    /// A WebDAV server, one account per entry.
    Webdav,
    /// An S3 account (an endpoint plus an access key id), whose places are its
    /// saved buckets and, when saved, its root.
    S3,
    /// An NFS server, whose places are its saved exports.
    Nfs,
}

/// The account a server's share list was listed as, for "as `<account>`": on the
/// share list's path bar, and on the server's row in the Servers list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShareListAccount {
    /// An anonymous session.
    Guest,
    /// Signed in as this account.
    User {
        /// The account's username.
        username: String,
    },
}

/// How live a server's row is, for its status dot and the words beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    /// A live session Cmdr owns, or an SMB share from it mounted (Cmdr's mount or
    /// the OS's).
    Connected,
    /// Saved and idle: nothing in flight, and nothing is wrong.
    Saved,
    /// An SMB host saved without contact ("Add anyway", an approved Ask Cmdr card):
    /// nothing has dialed it, and nothing lists its shares until the person connects.
    NotConnectedYet,
    /// mDNS is seeing it right now.
    FoundNearby,
    /// The session dropped because a credential is what's missing.
    SignedOut,
    /// An SFTP host key is waiting for the person to look at it.
    WaitingForKey,
}

/// The facts a Servers row carries about its server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerMeta {
    /// The row's own id, which every action on the row resolves its saved entry by:
    /// the saved server's id, else the discovered host's. ❌ Never re-derived from a
    /// host, name, or address at action time.
    pub row_id: String,
    /// Which protocol the server speaks.
    pub protocol: ServerProtocol,
    /// Where the server is, as the Details cell shows it: resolved where mDNS
    /// resolved it, with the port when it isn't the protocol's own.
    pub address: String,
    /// How live the row is.
    pub status: ServerStatus,
    /// The account the row is signed in as, `None` when nothing known says. An SMB
    /// server: the account its share list signed in as, else the one it's set to be
    /// used with. A one-place server: `None` (its name says the account already).
    pub account: Option<ShareListAccount>,
    /// Whether the person saved it (`false`: a host Cmdr only found nearby).
    pub saved: bool,
    /// The volume id of the place Enter opens: a one-place server's. `None` for an
    /// SMB host (it opens its share list) and an S3 account (it opens its root).
    pub place: Option<String>,
    /// Whether that place is pinned to the switcher. Always `false` without a place.
    pub pinned: bool,
}
