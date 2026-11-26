//! Where a server draft sits among what's saved: whether an add is saved already, which row it
//! joins, the store entry it saves, and whether its name is another server's.
//!
//! Pure over a [`SavedServers`] snapshot. `propose_server` asks before it stages a card, and the
//! approval asks again before it saves, because a server saved in between (by the user, or by
//! another approved card) can make either answer change.

use super::saved::{SavedRow, SavedServers};
use crate::agent::store::proposals::ServerDraft;
use crate::agent::types::DraftProtocol;
use crate::saved_servers::ServerTarget;
use crate::saved_servers::target_ids::place_id;

/// SMB's port, the add form's default.
pub(crate) const SMB_PORT: u16 = 445;
/// SFTP's port, the add form's default.
pub(crate) const SFTP_PORT: u16 = 22;

/// Why an add can't be saved beside what's saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AddClash {
    /// A bucket under an S3 account nothing saved has.
    UnknownAccount { id: String },
    /// The address is saved already, as the place with this id: an edit of it is the way.
    AlreadySaved { server_id: String },
    /// No store could key it: a WebDAV URL no dial could open, or an NFS add with no identity.
    Unkeyable,
}

/// An add that's clear of what's saved: what it saves, and where that lands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedAdd {
    /// The row it joins (an S3 account, an NFS server), whose name it may share.
    pub own_row: Option<String>,
    /// The store entry an add saves, with the add form's defaults for every switch the draft
    /// can't carry, the SSH agent's aside ([`target_of`]). `None` for SMB, which saves a host (`manual_servers`), not a target.
    pub target: Option<ServerTarget>,
    /// The id the saved place will have; `None` for SMB, whose share learns one at its first
    /// mount.
    pub place_id: Option<String>,
}

impl SavedServers {
    /// Where `draft`, an add, lands: refused when its address is saved already, and for S3
    /// when its account isn't. By the stores' own id funnel (`place_id`), ❌ never by comparing
    /// a typed address: the stores normalize (a WebDAV URL gains its trailing slash).
    pub(crate) fn place_add(&self, draft: &ServerDraft) -> Result<PlacedAdd, AddClash> {
        if draft.protocol == DraftProtocol::Smb {
            // An SMB host is its address AND port (`saved_servers/DETAILS.md` § SMB hosts).
            let (host, port) = (
                draft.host.as_deref().unwrap_or_default(),
                draft.port.unwrap_or(SMB_PORT),
            );
            if let Some(row) = self.rows.iter().find(|row| {
                row.smb_host.as_ref().is_some_and(|(saved_host, saved_port)| {
                    saved_host.eq_ignore_ascii_case(host) && *saved_port == port
                })
            }) {
                return Err(AddClash::AlreadySaved {
                    server_id: row
                        .places
                        .first()
                        .map_or_else(|| row.id.clone(), |place| place.volume_id.clone()),
                });
            }
            return Ok(PlacedAdd {
                own_row: None,
                target: None,
                place_id: None,
            });
        }
        let (target, own_row) = match draft.protocol {
            DraftProtocol::S3 => {
                let account = draft.account_id.clone().unwrap_or_default();
                let Some((provider, access_key_id)) = self.row(&account).and_then(|row| row.s3_account.clone()) else {
                    return Err(AddClash::UnknownAccount { id: account });
                };
                let target = ServerTarget::S3 {
                    display_name: text(&draft.display_name),
                    provider,
                    access_key_id,
                    bucket: draft.bucket.clone(),
                    auto_reconnect: true,
                };
                (target, Some(account))
            }
            DraftProtocol::Nfs => {
                let server = cmdr_nfs::NfsServer::new(draft.host.as_deref().unwrap_or_default(), draft.port);
                let own_row = crate::network::nfs_known_places::server_id(&server);
                (target_of(draft).ok_or(AddClash::Unkeyable)?, Some(own_row))
            }
            _ => (target_of(draft).ok_or(AddClash::Unkeyable)?, None),
        };
        let id = place_id(target.clone()).ok_or(AddClash::Unkeyable)?;
        if self.place(&id).is_some() {
            return Err(AddClash::AlreadySaved { server_id: id });
        }
        Ok(PlacedAdd {
            own_row,
            target: Some(target),
            place_id: Some(id),
        })
    }

    /// The saved server other than `own_row` that already wears `name`, folded the way every
    /// name check folds (`fold_name`), so an attacker host can't be called what the NAS is
    /// called.
    pub(crate) fn name_holder(&self, name: &str, own_row: Option<&str>) -> Option<&SavedRow> {
        let folded = cmdr_fs::name_fold::fold_name(name.trim());
        self.rows
            .iter()
            .find(|row| Some(row.id.as_str()) != own_row && cmdr_fs::name_fold::fold_name(row.name.trim()) == folded)
    }

    /// Whether any saved server, of any protocol, is at the host `draft` names. What the card
    /// marks "a new address" when it isn't. An S3 draft names a saved account, never a host.
    pub(crate) fn knows_host_of(&self, draft: &ServerDraft) -> bool {
        let Some(host) = host_of(draft) else {
            return draft.protocol == DraftProtocol::S3;
        };
        self.rows.iter().any(|row| {
            row.smb_host
                .as_ref()
                .is_some_and(|(saved, _)| saved.eq_ignore_ascii_case(&host))
                || row
                    .places
                    .iter()
                    .any(|place| host_of(&place.draft).is_some_and(|saved| saved.eq_ignore_ascii_case(&host)))
        })
    }
}

/// The host a draft dials: its `host`, or its URL's.
fn host_of(draft: &ServerDraft) -> Option<String> {
    match (&draft.host, &draft.url) {
        (Some(host), _) => Some(host.trim().to_string()),
        (None, Some(url)) => url::Url::parse(url.trim())
            .ok()
            .and_then(|url| url.host_str().map(str::to_string)),
        (None, None) => None,
    }
}

fn text(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

/// The store entry an SFTP, WebDAV, or NFS add saves: the draft, plus the add form's defaults
/// for the switches it can't carry ("Reconnect automatically" on, no key file), except the SSH
/// agent, which stays OFF: on, the first connect would offer every key in the user's agent to
/// a host the AI picked. The user turns it on in the edit server sheet. `None` for an NFS draft with no
/// identity, which the form never saves, and for the two protocols keyed elsewhere (SMB by
/// host, S3 by account).
fn target_of(draft: &ServerDraft) -> Option<ServerTarget> {
    let target = match draft.protocol {
        DraftProtocol::Sftp => ServerTarget::Sftp {
            display_name: text(&draft.display_name),
            host: text(&draft.host),
            port: draft.port.unwrap_or(SFTP_PORT),
            username: text(&draft.username),
            remote_root: text(&draft.remote_root),
            start_folder: draft.start_folder.clone(),
            key_file: None,
            use_agent: false,
            auto_reconnect: true,
        },
        DraftProtocol::Webdav => ServerTarget::Webdav {
            display_name: text(&draft.display_name),
            url: text(&draft.url),
            username: text(&draft.username),
            remote_root: text(&draft.remote_root),
            start_folder: draft.start_folder.clone(),
            auto_reconnect: true,
        },
        DraftProtocol::Nfs => ServerTarget::Nfs {
            display_name: text(&draft.display_name),
            host: text(&draft.host),
            port: draft.port,
            export: text(&draft.export),
            uid: draft.uid?,
            gid: draft.gid?,
            other_gids: Vec::new(),
            start_folder: draft.start_folder.clone(),
            auto_reconnect: true,
        },
        DraftProtocol::Smb | DraftProtocol::S3 => return None,
    };
    Some(target)
}
