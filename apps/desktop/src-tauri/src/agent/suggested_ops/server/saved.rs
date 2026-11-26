//! The saved servers a server proposal is checked against, as plain facts: read once per check
//! through `saved_servers` (the Servers list's own saved listing and lookups), in memory, ❌ never the network.
//!
//! Each saved place becomes a [`ServerDraft`], the spine's own shape, so what a proposal stores
//! as an edit's `previous` is exactly what this read, and the checks compare like with like.
//!
//! Here, below both, because the same checks run twice: when `propose_server` stages a card, and
//! again when the user approves it (`DETAILS.md` § Approving a server). One copy, so the two can't
//! disagree about what "saved already" or "that name is taken" means.

use crate::agent::store::proposals::ServerDraft;
use crate::agent::types::DraftProtocol;
use crate::network::s3_known_places::{self, S3ProviderChoice};
use crate::network::{nfs_known_places, saved_server_fields};
use crate::saved_servers::smb_hosts::SmbHostGroup;
use crate::saved_servers::{SavedEntry, SavedServer, ServerNameSource, ServerProtocol};

/// Every saved server, one per saved-listing row.
#[derive(Debug, Clone, Default)]
pub(crate) struct SavedServers {
    pub(crate) rows: Vec<SavedRow>,
}

/// One saved-listing row: an SFTP or WebDAV account, an S3 account, an NFS server, or an SMB host.
#[derive(Debug, Clone)]
pub(crate) struct SavedRow {
    /// The row's id in the servers listing.
    pub id: String,
    /// What the Servers list calls it, a stand-in (an address) included: what a new name mustn't wear.
    pub name: String,
    pub protocol: DraftProtocol,
    /// The places under it, each by the id `list_volumes` shows.
    pub places: Vec<SavedPlaceDraft>,
    /// SMB: the host's address and port, its identity. Present even with no share saved.
    pub smb_host: Option<(String, u16)>,
    /// S3: the endpoint and key a new bucket place under this account is keyed by.
    pub s3_account: Option<(S3ProviderChoice, String)>,
}

/// One saved place, as a draft of its editable fields and its address.
#[derive(Debug, Clone)]
pub(crate) struct SavedPlaceDraft {
    pub volume_id: String,
    pub draft: ServerDraft,
}

impl SavedServers {
    /// What's saved now, through the Servers list's own reads.
    pub(crate) fn now<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Self {
        let (rows, smb_hosts) = crate::saved_servers::saved_now(app);
        Self::from_listing(rows, smb_hosts, crate::saved_servers::saved_by_id)
    }

    /// The facts over a listing, its SMB host groups, and a lookup of the saved entry behind a
    /// place id (`saved_by_id` in the app; a fixture in tests).
    pub(crate) fn from_listing(
        rows: Vec<SavedServer>,
        smb_hosts: Vec<SmbHostGroup>,
        entry_of: impl Fn(&str) -> Option<SavedEntry>,
    ) -> Self {
        let mut saved: Vec<SavedRow> = smb_hosts.into_iter().map(smb_row).collect();
        for row in rows.into_iter().filter(|row| row.protocol != ServerProtocol::Smb) {
            let mut facts = SavedRow {
                id: row.id,
                name: row.display_name,
                protocol: protocol_of(row.protocol),
                places: Vec::new(),
                smb_host: None,
                s3_account: None,
            };
            for place in row.places {
                let Some(entry) = entry_of(&place.volume_id) else {
                    continue;
                };
                if let SavedEntry::S3(s3) = &entry {
                    facts.s3_account = Some((s3.provider.clone(), s3.access_key_id.clone()));
                }
                if let Some(draft) = draft_of(entry) {
                    facts.places.push(SavedPlaceDraft {
                        volume_id: place.volume_id,
                        draft,
                    });
                }
            }
            saved.push(facts);
        }
        Self { rows: saved }
    }

    /// The row and place a saved place's id names.
    pub(crate) fn place(&self, volume_id: &str) -> Option<(&SavedRow, &SavedPlaceDraft)> {
        self.rows.iter().find_map(|row| {
            row.places
                .iter()
                .find(|place| place.volume_id == volume_id)
                .map(|place| (row, place))
        })
    }

    /// The row with this id.
    pub(crate) fn row(&self, id: &str) -> Option<&SavedRow> {
        self.rows.iter().find(|row| row.id == id)
    }
}

fn protocol_of(protocol: ServerProtocol) -> DraftProtocol {
    match protocol {
        ServerProtocol::Smb => DraftProtocol::Smb,
        ServerProtocol::Sftp => DraftProtocol::Sftp,
        ServerProtocol::Webdav => DraftProtocol::Webdav,
        ServerProtocol::S3 => DraftProtocol::S3,
        ServerProtocol::Nfs => DraftProtocol::Nfs,
    }
}

/// A name a person chose, or `None` for a stand-in: a draft carries only what a person can edit.
fn named(name: &str) -> Option<String> {
    saved_server_fields::is_named(name).then(|| name.trim().to_string())
}

/// An SMB host: its shares are its places, and an edit of any of them is an edit of the host.
fn smb_row(group: SmbHostGroup) -> SavedRow {
    let draft = ServerDraft {
        display_name: (group.server.name_source == ServerNameSource::User).then(|| group.server.display_name.clone()),
        host: Some(group.host.clone()),
        port: Some(group.port),
        username: group.server.username.clone(),
        shares_affected: Some(u32::try_from(group.server.places.len()).unwrap_or(u32::MAX)),
        ..ServerDraft::new(DraftProtocol::Smb)
    };
    SavedRow {
        places: group
            .server
            .places
            .iter()
            .map(|place| SavedPlaceDraft {
                volume_id: place.volume_id.clone(),
                draft: draft.clone(),
            })
            .collect(),
        id: group.server.id,
        name: group.server.display_name,
        protocol: DraftProtocol::Smb,
        smb_host: Some((group.host, group.port)),
        s3_account: None,
    }
}

/// A saved place's editable fields and address, as the spine stores them. `None` for an SMB
/// share, whose host the caller drafts from its group.
fn draft_of(entry: SavedEntry) -> Option<ServerDraft> {
    let draft = match entry {
        SavedEntry::Sftp(entry) => ServerDraft {
            display_name: named(&entry.display_name),
            host: Some(entry.host),
            port: Some(entry.port),
            username: Some(entry.username),
            remote_root: Some(entry.remote_root),
            start_folder: entry.start_folder,
            ..ServerDraft::new(DraftProtocol::Sftp)
        },
        SavedEntry::Webdav(entry) => ServerDraft {
            display_name: named(&entry.display_name),
            url: Some(entry.url),
            username: Some(entry.username),
            remote_root: Some(entry.remote_root),
            start_folder: entry.start_folder,
            ..ServerDraft::new(DraftProtocol::Webdav)
        },
        SavedEntry::S3(place) => ServerDraft {
            display_name: named(&s3_known_places::account_name(&place)),
            account_id: place.params().ok().map(|params| s3_known_places::account_id(&params)),
            bucket: place.bucket,
            ..ServerDraft::new(DraftProtocol::S3)
        },
        SavedEntry::Nfs(place) => ServerDraft {
            display_name: named(&nfs_known_places::server_name(&place)),
            host: Some(place.host),
            port: place.port,
            export: Some(place.export),
            uid: Some(place.uid),
            gid: Some(place.gid),
            start_folder: place.start_folder,
            ..ServerDraft::new(DraftProtocol::Nfs)
        },
        SavedEntry::Smb(_) => return None,
    };
    Some(draft)
}
