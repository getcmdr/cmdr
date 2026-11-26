//! Approving a server proposal: the user's click on a server card, saved through the cores
//! the add and edit server sheets run, or given back with the reason.
//!
//! [`approve_server`] runs, in order: the claim (the store's, one `BEGIN IMMEDIATE`, the revision
//! as its binding); the anti-spoofing checks again against what's saved NOW (an add saved since,
//! a name taken since, an edited server changed since); one save through a [`ServerSaver`]; then
//! the group's end by what the save says. Only a save that wrote NOTHING gives the claim back:
//! anything that may have written ends `completed`, or stays `approved` for the next launch to
//! call `interrupted`, so no second click can write twice. Why: `../DETAILS.md` § Approving a
//! server.
//!
//! ❌ The `main.db` write transaction is never held across the save: the claim commits, the save
//! runs, then the completion writes.

mod app_saver;
mod card;
mod placement;
mod saved;

#[cfg(test)]
pub(crate) mod test_fixtures;
#[cfg(test)]
mod tests;

pub(crate) use app_saver::AppSaver;
pub(crate) use card::server_card;
pub use card::{ServerField, ServerFieldView, ServerProposalView};
pub(crate) use placement::{AddClash, SFTP_PORT, SMB_PORT};
pub(crate) use saved::SavedServers;

use std::time::Duration;

use futures_util::future::BoxFuture;
use rusqlite::Connection;

use super::changed::{self, SuggestionChange};
use crate::agent::memory::MemoryStore;
use crate::agent::store::AgentStoreError;
use crate::agent::store::proposals::{
    ClaimedServer, CompleteOutcome, ServerClaimOutcome, ServerDraft, claim_server_group, mark_server_group_completed,
};
use crate::agent::types::DraftProtocol;
use crate::network::manual_servers::{AddServerError, ManualConnectResult};
use crate::network::saved_server_fields::SavedServerOutcome;
use crate::saved_servers::{PlaceEdit, ServerTarget};

const LOG_TARGET: &str = "agent::suggested_ops";

/// How long a save may take before the approval stops waiting. A save to a place nobody
/// connected is a store write; one to a connected place confirms its folders over the live
/// session (`network/live_server_edit.rs`), and an NFS identity change dials, so it gets room.
/// Past it the save runs on, detached, and the group stays `approved`: whether it wrote is
/// unknown, so the next launch calls it `interrupted`, ❌ never pending again.
const SAVE_DEADLINE: Duration = Duration::from_secs(60);

/// One save, as exactly one of the server sheets' own cores runs it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ServerSave {
    /// An SFTP, WebDAV, NFS, or S3 add: `saved_servers::save_target`, which dials nothing for a
    /// place nobody connected.
    Add(ServerTarget),
    /// An SMB add: `manual_servers::save_manual_server_without_contact` with `address`
    /// (`smb://host:port`, plus `/share` when the draft names one), so the host stays out of
    /// discovery and the GUID probe until the user first connects it.
    SmbAdd {
        address: String,
        name: String,
        username: Option<String>,
    },
    /// An edit of one saved place: `saved_servers::save_place_edit`, which keeps the address,
    /// the account, and every switch as stored.
    Edit { id: String, edit: PlaceEdit },
    /// An edit of an SMB host's name and account: `smb_hosts::edit_host` with NO address, so it
    /// saves in place and never moves the host or its passwords.
    SmbHostEdit {
        host_id: String,
        name: String,
        username: Option<String>,
    },
}

/// What a save did, as the approval needs it: whether anything was written.
///
/// ❗ No "partial": every core here either refuses before writing or writes all of it. The
/// contact-free SMB add writes the host and then its share row with no failure point it
/// reports between them (both stores' file writes log rather than answer), so a
/// `ServerSaveNote` has no producer yet. A core that grows one answers a variant here, and the
/// group completes with that note (`mark_server_group_completed` already stores it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveResult {
    Saved,
    /// The core refused before writing anything, so the card can go back to the user.
    Refused(SaveRefusal),
}

/// A core's own refusal, which it documents as writing nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveRefusal {
    Server(SavedServerOutcome),
    AddServer(AddServerError),
}

impl SaveResult {
    /// A saved-server core's answer. Every refusal is one the core makes BEFORE it writes:
    /// checked per variant against `saves.rs`, the wirings' `save_without_connecting`,
    /// `live_server_edit.rs` ("a refusal moves NOTHING, the store included"), `smb_hosts.rs`,
    /// and `server_move.rs`. ❌ No `_` arm: a new variant has to be classified, because one
    /// wrongly called pre-write would let a second click write twice.
    pub fn of_outcome(outcome: SavedServerOutcome) -> Self {
        match outcome {
            SavedServerOutcome::Saved => SaveResult::Saved,
            refused @ (SavedServerOutcome::StartFolderOutsideRoot
            | SavedServerOutcome::RootNotFound
            | SavedServerOutcome::StartFolderNotFound
            | SavedServerOutcome::Unreachable
            | SavedServerOutcome::AddressTaken { .. }
            | SavedServerOutcome::SecretNotMoved
            | SavedServerOutcome::OperationRunning
            | SavedServerOutcome::ShareMounted { .. }
            | SavedServerOutcome::AccountChanged) => SaveResult::Refused(SaveRefusal::Server(refused)),
        }
    }

    /// The contact-free SMB add's answer. Both of `AddServerError`'s variants come from reading
    /// the address, before the save writes the host or its share (`manual_servers.rs`).
    pub fn of_smb_add(result: Result<ManualConnectResult, AddServerError>) -> Self {
        match result {
            Ok(_) => SaveResult::Saved,
            Err(error @ (AddServerError::InvalidAddress { .. } | AddServerError::Unreachable { .. })) => {
                SaveResult::Refused(SaveRefusal::AddServer(error))
            }
        }
    }
}

/// The saved stores, and the cores that save to them. The command layer implements it over
/// `saved_servers`; tests hand in fixtures, so the approval is testable without real stores.
pub(crate) trait ServerSaver: Send + Sync {
    /// What's saved now, in memory, ❌ never the network.
    fn saved(&self) -> SavedServers;
    /// Run one save. Owned and `Send`: the approval runs it as its own task, so a save past
    /// [`SAVE_DEADLINE`] finishes detached rather than being dropped mid-write.
    fn save(&self, save: ServerSave) -> BoxFuture<'static, SaveResult>;
}

/// What approving a server card did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerApprovalOutcome {
    /// Saved. `volume_id` is the saved place's (`None` for an SMB add: its share learns one at
    /// its first mount).
    Saved { volume_id: Option<String> },
    /// The group was amended since the card the user approved from. Nothing changed.
    ListChanged,
    /// It already left `pending`.
    AlreadyAnswered,
    /// No group with that id.
    Unknown,
    /// A file group: its approval is `bridge::approve_and_execute`.
    NotAServerGroup,
    /// The add's address was saved since it was proposed; the place to edit instead.
    AlreadySaved { server_id: String },
    /// Another saved server took the name since it was proposed.
    NameTaken { name: String },
    /// The edited server's address or editable fields changed since the card read them, so
    /// saving would overwrite a change the card didn't show.
    ServerChanged,
    /// The save's core refused, writing nothing. The group is `pending` again.
    Refused(SaveRefusal),
    /// The save didn't answer in time, or its task died. It may have written, so the group stays
    /// `approved` (the next launch calls it `interrupted`), and nothing can approve it again.
    NoAnswer,
}

/// Approve a server card: claim it, check it again against what's saved, save it, and end it.
pub(crate) async fn approve_server(
    conn: &Connection,
    group_id: i64,
    revision: i64,
    saver: &dyn ServerSaver,
    now: i64,
    memory: Option<&MemoryStore>,
) -> Result<ServerApprovalOutcome, AgentStoreError> {
    approve_server_within(conn, group_id, revision, saver, now, memory, SAVE_DEADLINE).await
}

/// [`approve_server`] with the deadline a test sets.
pub(crate) async fn approve_server_within(
    conn: &Connection,
    group_id: i64,
    revision: i64,
    saver: &dyn ServerSaver,
    now: i64,
    memory: Option<&MemoryStore>,
    deadline: Duration,
) -> Result<ServerApprovalOutcome, AgentStoreError> {
    let claimed = match claim_server_group(conn, group_id, revision, now)? {
        ServerClaimOutcome::Claimed(claimed) => claimed,
        ServerClaimOutcome::RevisionChanged { .. } => return Ok(ServerApprovalOutcome::ListChanged),
        ServerClaimOutcome::NotPending { .. } => return Ok(ServerApprovalOutcome::AlreadyAnswered),
        ServerClaimOutcome::NotAServerGroup => return Ok(ServerApprovalOutcome::NotAServerGroup),
        ServerClaimOutcome::Unknown => return Ok(ServerApprovalOutcome::Unknown),
    };
    changed::announce(conn, SuggestionChange::Approved, Some(group_id));

    let planned = match plan_save(&claimed, &saver.saved()) {
        Ok(planned) => planned,
        Err(refusal) => {
            give_back(conn, group_id, &refusal);
            return Ok(refusal);
        }
    };
    let save = saver.save(planned.save);
    let answer =
        cmdr_fs::deadline::timeout_detached_typed(deadline, || NoAnswer::TimedOut, NoAnswer::Panicked, async move {
            Ok(save.await)
        })
        .await;
    match answer {
        Ok(SaveResult::Saved) => {}
        Ok(SaveResult::Refused(refusal)) => {
            let refused = ServerApprovalOutcome::Refused(refusal);
            give_back(conn, group_id, &refused);
            return Ok(refused);
        }
        Err(gap) => {
            let gap = match gap {
                NoAnswer::TimedOut => format!("no answer within {deadline:?}"),
                NoAnswer::Panicked(detail) => format!("its task died: {detail}"),
            };
            log::warn!(
                target: LOG_TARGET,
                "server group {group_id}'s save didn't answer ({gap}); it stays approved, and the next launch calls it interrupted"
            );
            return Ok(ServerApprovalOutcome::NoAnswer);
        }
    }
    if mark_server_group_completed(conn, group_id, None)? != CompleteOutcome::Completed {
        log::warn!(target: LOG_TARGET, "server group {group_id} saved but was no longer approved to complete");
    }
    super::analytics::group_approved(claimed.group.verb, 0);
    crate::agent::outcomes::record_completion(conn, memory, group_id, now);
    Ok(ServerApprovalOutcome::Saved {
        volume_id: planned.volume_id,
    })
}

/// Why a save gave no answer. Only ever logged.
#[derive(Debug)]
enum NoAnswer {
    TimedOut,
    Panicked(String),
}

/// The one save a claimed card comes to, and the id of the place it saves.
struct PlannedSave {
    save: ServerSave,
    volume_id: Option<String>,
}

/// Check a claimed card against what's saved now, and turn it into its save. Pure. A refusal
/// here is one the claim gives back: nothing was written.
fn plan_save(claimed: &ClaimedServer, saved: &SavedServers) -> Result<PlannedSave, ServerApprovalOutcome> {
    let draft = &claimed.draft;
    // A place nobody saved any more gets the core's own answer for it, unasked.
    let gone = || ServerApprovalOutcome::Refused(SaveRefusal::Server(SavedServerOutcome::Unreachable));
    let Some(edit) = &claimed.edit else {
        let placed = saved.place_add(draft).map_err(|clash| match clash {
            AddClash::AlreadySaved { server_id } => ServerApprovalOutcome::AlreadySaved { server_id },
            AddClash::UnknownAccount { .. } | AddClash::Unkeyable => gone(),
        })?;
        if let Some(name) = &draft.display_name {
            name_free(saved, name, placed.own_row.as_deref())?;
        }
        let save = match placed.target {
            Some(target) => ServerSave::Add(target),
            None => ServerSave::SmbAdd {
                address: smb_address(draft),
                name: draft.display_name.clone().unwrap_or_default(),
                username: draft.username.clone(),
            },
        };
        return Ok(PlannedSave {
            save,
            volume_id: placed.place_id,
        });
    };
    let (row, place) = saved.place(&edit.editing).ok_or_else(gone)?;
    if drifted(&place.draft, &edit.previous) {
        return Err(ServerApprovalOutcome::ServerChanged);
    }
    if let Some(name) = draft
        .display_name
        .as_deref()
        .filter(|_| draft.display_name != edit.previous.display_name)
    {
        name_free(saved, name, Some(&row.id))?;
    }
    let save = match draft.protocol {
        DraftProtocol::Smb => ServerSave::SmbHostEdit {
            host_id: row.id.clone(),
            name: draft.display_name.clone().unwrap_or_default(),
            username: draft.username.clone(),
        },
        DraftProtocol::Sftp | DraftProtocol::Webdav | DraftProtocol::Nfs | DraftProtocol::S3 => ServerSave::Edit {
            id: edit.editing.clone(),
            edit: PlaceEdit {
                display_name: draft.display_name.clone(),
                remote_root: draft.remote_root.clone(),
                start_folder: draft.start_folder.clone(),
                uid: draft.uid,
                gid: draft.gid,
            },
        },
    };
    Ok(PlannedSave {
        save,
        volume_id: Some(edit.editing.clone()),
    })
}

fn name_free(saved: &SavedServers, name: &str, own_row: Option<&str>) -> Result<(), ServerApprovalOutcome> {
    match saved.name_holder(name, own_row) {
        Some(holder) => Err(ServerApprovalOutcome::NameTaken {
            name: holder.name.clone(),
        }),
        None => Ok(()),
    }
}

/// Whether a saved place no longer reads as the card's `previous`: its address or any field an
/// edit may change. ❌ Not the SMB host's share count, which only describes the card, and
/// nothing a reconnect writes (the last-connected time, a mount path): no draft carries those.
fn drifted(now: &ServerDraft, previous: &ServerDraft) -> bool {
    let comparable = |draft: &ServerDraft| ServerDraft {
        shares_affected: None,
        ..draft.clone()
    };
    comparable(now) != comparable(previous)
}

/// The address the contact-free SMB save reads: `smb://host:port`, and the share it saves a row
/// for. The draft's host is a plain ASCII host and its share a plain leaf (`propose_server`
/// checked both), so nothing here needs escaping.
fn smb_address(draft: &ServerDraft) -> String {
    let host = draft.host.as_deref().unwrap_or_default();
    let port = draft.port.unwrap_or(SMB_PORT);
    match &draft.share {
        Some(share) => format!("smb://{host}:{port}/{share}"),
        None => format!("smb://{host}:{port}"),
    }
}

/// Give a claim back: nothing was written, so the card is the user's again. A store that can't
/// is logged, ❌ never raised: the refusal is the answer the user needs, and the group then reads
/// `interrupted` next launch, which is honest.
fn give_back(conn: &Connection, group_id: i64, why: &ServerApprovalOutcome) {
    // The kind only: a name or an address in a refusal is the user's own data.
    let why = why.kind();
    match super::give_back(conn, group_id) {
        Ok(true) => log::info!(target: LOG_TARGET, "server group {group_id} went back to the list: {why}"),
        Ok(false) => log::warn!(
            target: LOG_TARGET,
            "server group {group_id} was refused ({why}) but is no longer a claim to give back"
        ),
        Err(e) => log::warn!(
            target: LOG_TARGET,
            "server group {group_id} was refused ({why}) and couldn't go back to the list: {e}"
        ),
    }
}

impl ServerApprovalOutcome {
    /// What kind of answer this is, for a log line.
    fn kind(&self) -> &'static str {
        match self {
            Self::Saved { .. } => "saved",
            Self::ListChanged => "list_changed",
            Self::AlreadyAnswered => "already_answered",
            Self::Unknown => "unknown",
            Self::NotAServerGroup => "not_a_server_group",
            Self::AlreadySaved { .. } => "already_saved",
            Self::NameTaken { .. } => "name_taken",
            Self::ServerChanged => "server_changed",
            Self::Refused(SaveRefusal::Server(_)) => "refused_by_the_save",
            Self::Refused(SaveRefusal::AddServer(_)) => "refused_by_the_smb_add",
            Self::NoAnswer => "no_answer",
        }
    }
}
