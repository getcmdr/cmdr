//! Where `..` leads from a folder whose `..` isn't the folder above it in its volume: a
//! server's top folder, which leads up to where the person found that server.
//!
//! - A server's share list (`smb://<server>` on the `network` volume) → the Servers list
//!   (`smb://`), on that server's row. Always shown.
//! - An SMB share's top folder → its server's share list, on the share (#1238). The share's
//!   own switch decides (`smb_share_parent.rs`).
//! - A one-place server's place (SFTP, WebDAV, NFS), and an S3 account's root → the Servers
//!   list, on the server's row.
//! - An S3 bucket opened as its own place → its account's root (the bucket list), on the
//!   bucket, when that root is saved; else the Servers list, on the account's row (Enter
//!   there saves the root).
//!
//! The last two follow `network.showServerParentRow`. Every answer is read off the
//! Servers list's own rows (`servers_listing.rs`: a row holds its places), ❌ never a
//! scheme sniffed off a path, so a new protocol gets its `..` by being listed there.
//! Memory, the stores, and the mount table only: it runs on a listing's way out.

use serde::{Deserialize, Serialize};

use crate::network::servers_listing::{ServerOpens, ServerRow};
use crate::network::share_list_path::{NetworkPath, share_list_path};
use crate::network::share_list_volume::NETWORK_VOLUME_ID;
use crate::network::smb_share_parent::ShareParent;

/// The Servers list's path on the `network` volume, as the frontend spells it.
const SERVERS_LIST_PATH: &str = "smb://";

/// Where `..` leads out of a server's top folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TopFolderParent {
    /// The volume `..` opens: `network` for the Servers list and a share list, an S3
    /// account's root place for a bucket.
    pub volume_id: String,
    /// The folder it opens there, as the frontend spells it (`smb://`, `smb://testnas`,
    /// `s3://<key id>@<host>:<port>/`).
    pub path: String,
    /// The row the cursor lands on there, by its path: the server's Servers row, the
    /// bucket. `None` for a share, which lands on its name through the share list's
    /// arrival.
    pub select_path: Option<String>,
    /// Which switch shows this `..`.
    pub switch: ParentRowSwitch,
}

/// The switch that shows a [`TopFolderParent`]'s `..`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ParentRowSwitch {
    /// Always shown: a share list's `..` to the Servers list.
    Always,
    /// The SMB share's own choice over `network.showShareParentRow` (the listing's
    /// `share_parent` carries it).
    Share,
    /// `network.showServerParentRow`: a server's place, an S3 account's root, a bucket.
    Server,
}

/// Where `..` leads from `listed` on `volume_id`, or `None` where it's the folder above
/// (or nothing, at a local volume's root). `at_volume_top`: `listed` is its volume's root.
/// `share`: what `smb_share_parent::share_parent_of` said about the volume's share.
pub(crate) fn top_folder_parent_of(
    volume_id: &str,
    listed: &str,
    at_volume_top: bool,
    share: Option<&ShareParent>,
) -> Option<TopFolderParent> {
    if volume_id == NETWORK_VOLUME_ID {
        let NetworkPath::ShareList(_) = NetworkPath::of(listed) else {
            return None;
        };
        let row = crate::network::share_list_volume::servers_row_path(listed);
        return Some(share_list_parent(listed, row));
    }
    if !at_volume_top {
        return None;
    }
    if let Some(share) = share {
        return Some(share_top_parent(share));
    }
    let rows = crate::network::share_list_volume::server_rows_now()?;
    place_parent(&rows, volume_id, listed)
}

/// A share list's `..`: the Servers list, on `row` (the row that stands for the listed
/// server, by identity), else on the listed path itself.
pub(crate) fn share_list_parent(listed: &str, row: Option<String>) -> TopFolderParent {
    TopFolderParent {
        volume_id: NETWORK_VOLUME_ID.to_string(),
        path: SERVERS_LIST_PATH.to_string(),
        select_path: Some(row.unwrap_or_else(|| listed.trim_end_matches('/').to_string())),
        switch: ParentRowSwitch::Always,
    }
}

/// A share's top folder's `..`: its server's share list.
pub(crate) fn share_top_parent(share: &ShareParent) -> TopFolderParent {
    TopFolderParent {
        volume_id: NETWORK_VOLUME_ID.to_string(),
        path: share_list_path(&share.host),
        select_path: None,
        switch: ParentRowSwitch::Share,
    }
}

/// A saved place's top folder's `..`, read off the Servers list's `rows`: the row whose
/// server holds `volume_id`. A bucket of an account whose root is saved leads to that root,
/// on the bucket (`root`, the bucket's top, is the row's path in the bucket list); every
/// other place to the Servers list, on its server's row. `None` for a volume no row holds.
pub(crate) fn place_parent(rows: &[ServerRow], volume_id: &str, root: &str) -> Option<TopFolderParent> {
    let row = rows
        .iter()
        .find(|row| row.places.iter().any(|place| place == volume_id))?;
    if let ServerOpens::S3Account {
        root_volume_id,
        root_app_root: Some(account_root),
    } = &row.opens
        && root_volume_id != volume_id
    {
        return Some(TopFolderParent {
            volume_id: root_volume_id.clone(),
            path: account_root.clone(),
            select_path: Some(root.to_string()),
            switch: ParentRowSwitch::Server,
        });
    }
    Some(TopFolderParent {
        volume_id: NETWORK_VOLUME_ID.to_string(),
        path: SERVERS_LIST_PATH.to_string(),
        select_path: Some(row.path.clone()),
        switch: ParentRowSwitch::Server,
    })
}

#[cfg(test)]
#[path = "top_folder_parent_test.rs"]
mod tests;
