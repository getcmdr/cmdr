//! A server card's approve: the user's click, handed to `agent::suggested_ops::server`, which
//! claims the card, checks it again against what's saved, and saves it through the cores the
//! add and edit server sheets run. This file only bridges it and words the answer for the dialog.

use serde::Serialize;
use tauri::AppHandle;

use super::now_secs;
use super::suggested_ops::SuggestedOpsError;
use crate::agent::suggested_ops::server::{AppSaver, SaveRefusal, ServerApprovalOutcome, approve_server};
use crate::network::manual_servers::AddServerError;
use crate::network::saved_server_fields::SavedServerOutcome;

/// What approving a server card did, in the terms the dialog acts on.
///
/// Every refusal but `alreadyAnswered` and `unknown` gives the card back: nothing was written,
/// and it's `pending` again with the reason under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ServerApprovalResultView {
    /// Saved, and in the servers list now, not connected. `volumeId` is the saved place's
    /// (`None` for an SMB host, whose share learns one at its first mount).
    Saved { volume_id: Option<String> },
    /// The AI changed the card since the user read it. Nothing was saved; re-read it.
    ListChanged,
    /// The card left `pending` before this arrived.
    AlreadyAnswered,
    /// No card with that id.
    Unknown,
    /// The save refused, with the same outcome the servers sheet words.
    Refused { outcome: SavedServerOutcome },
    /// The SMB add refused the address, with the same error the add sheet words.
    AddServer { error: AddServerError },
    /// The server the card edits changed since the AI suggested this (its address or a field
    /// the card shows), so saving would overwrite a change the card didn't show.
    ServerChanged,
    /// That server was saved since the AI suggested adding it, as the place with this id.
    AlreadySaved { server_id: String },
    /// Another saved server took this name since the AI suggested it.
    NameTaken { name: String },
}

/// Approve a server card the user read at `revision`: save it, with no password and no contact
/// with the server (an edit of a connected place rides the session the user opened).
///
/// **On a blocking thread, driving the app's runtime.** `approve_server` holds a `Connection`
/// across an await, which a command's future can't (it must be `Send`), and the save it starts
/// must run on the app's runtime: a save past its deadline finishes there, detached.
///
/// ❗ Emits `volumes-changed` on a save, as `update_saved_server` does: the Servers list and the
/// switcher re-read the saved list on it.
#[tauri::command]
#[specta::specta]
pub async fn suggested_ops_approve_server(
    app: AppHandle,
    group_id: i64,
    revision: i64,
) -> Result<ServerApprovalResultView, SuggestedOpsError> {
    let db_path = super::db_path(&app).ok_or(SuggestedOpsError::StoreNotOpen)?;
    let now = now_secs();
    let memory = crate::agent::memory::store_for(&app);
    let saver = AppSaver::new(app);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let conn = crate::agent::store::open_write_connection(&db_path)
            .map_err(|e| SuggestedOpsError::Store { detail: e.to_string() })?;
        tauri::async_runtime::block_on(approve_server(&conn, group_id, revision, &saver, now, memory.as_ref()))
            .map_err(|e| SuggestedOpsError::Store { detail: e.to_string() })
    })
    .await
    .map_err(|e| SuggestedOpsError::Unexpected { detail: e.to_string() })??;
    if matches!(outcome, ServerApprovalOutcome::Saved { .. }) {
        crate::volume_broadcast::emit_volumes_changed();
    }
    to_server_approval_view(outcome)
}

/// Map the service's outcome onto what the dialog acts on. Pure, so the mapping is pinned by
/// tests rather than read out of the command.
pub(super) fn to_server_approval_view(
    outcome: ServerApprovalOutcome,
) -> Result<ServerApprovalResultView, SuggestedOpsError> {
    Ok(match outcome {
        ServerApprovalOutcome::Saved { volume_id } => ServerApprovalResultView::Saved { volume_id },
        ServerApprovalOutcome::ListChanged => ServerApprovalResultView::ListChanged,
        ServerApprovalOutcome::AlreadyAnswered => ServerApprovalResultView::AlreadyAnswered,
        ServerApprovalOutcome::Unknown => ServerApprovalResultView::Unknown,
        // A file card approves through `suggested_ops_approve`; the dialog never sends one here.
        ServerApprovalOutcome::NotAServerGroup => {
            log::warn!(
                target: "agent::suggested_ops",
                "a file group reached the server approval, which never runs a file operation"
            );
            ServerApprovalResultView::Unknown
        }
        ServerApprovalOutcome::Refused(SaveRefusal::Server(outcome)) => ServerApprovalResultView::Refused { outcome },
        ServerApprovalOutcome::Refused(SaveRefusal::AddServer(error)) => ServerApprovalResultView::AddServer { error },
        ServerApprovalOutcome::ServerChanged => ServerApprovalResultView::ServerChanged,
        ServerApprovalOutcome::AlreadySaved { server_id } => ServerApprovalResultView::AlreadySaved { server_id },
        ServerApprovalOutcome::NameTaken { name } => ServerApprovalResultView::NameTaken { name },
        // It may have written, so it stays `approved`: the dialog re-reads rather than offering
        // the click again.
        ServerApprovalOutcome::NoAnswer => return Err(SuggestedOpsError::ApprovalDidntFinish),
    })
}

#[cfg(test)]
#[path = "suggested_ops_server_test.rs"]
mod suggested_ops_server_test;
