//! Creating sweeps and groups, and re-proposing a pending one.
//!
//! Creation is where a proposal FREEZES: whatever produced the op list (an explicit path
//! list or a resolved selector) hands over concrete rows here, and nothing re-derives them
//! later. Re-propose is the one way an op list changes afterwards, it belongs to the group's
//! author, and it works only while the group is `pending`.

use rusqlite::{Connection, Transaction, TransactionBehavior, params};

use super::super::AgentStoreError;
use super::GroupIntent;
use super::overlap::supersede_overlaps;
use super::server::{delete_server_draft, write_server_draft};
use crate::agent::types::{OpStatus, ProposalStatus};

/// A new sweep: one agent wake's output, before its groups exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewSweep {
    /// The chat thread this came out of, when it came out of one. A background wake has
    /// none, and deleting the thread later NULLs this rather than deleting the sweep.
    pub conversation_id: Option<i64>,
    /// Which model produced it. Provenance only: no logic reads it (agent decision D32).
    pub created_by_model: Option<String>,
    /// The agent's words for the sweep as a whole.
    pub rationale: Option<String>,
}

/// One proposed op under a verb whose group binds the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOp {
    pub source_path: String,
    /// What the source looked like when the proposal was created, when the producer knew.
    /// `None` is normal: a path that was missing, refused, or too slow to stat at creation.
    pub snapshot: Option<OpSnapshot>,
}

/// One proposed op under move or copy: a source landing in the group's destination folder,
/// under its own name or the one given here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOp {
    pub source_path: String,
    /// The name the source lands as, a NAME, never a path. `None` keeps its own, and a
    /// producer normalizes a name equal to the source's own to `None`, so the review draws an
    /// arrow only where a name changes.
    pub new_name: Option<String>,
    pub snapshot: Option<OpSnapshot>,
}

impl From<NewOp> for TransferOp {
    /// An op that keeps its own name: what a selector, which can't decide names, produces.
    fn from(op: NewOp) -> Self {
        Self {
            source_path: op.source_path,
            new_name: None,
            snapshot: op.snapshot,
        }
    }
}

/// One proposed rename. Rename's ops ALWAYS carry their own destinations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRename {
    pub source_path: String,
    /// The name the source becomes. A NAME, not a path: `start_bulk_rename` refuses a row
    /// that would change the parent.
    pub new_name: String,
    pub snapshot: Option<OpSnapshot>,
}

/// What a source looked like at creation, for the executor's drift check at apply. Every
/// field is optional because the drive index doesn't always hold all three.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpSnapshot {
    pub size: Option<u64>,
    /// Modification time, unix seconds.
    pub mtime: Option<i64>,
    pub inode: Option<u64>,
}

/// A group to create (or to re-propose an existing one into).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGroup {
    /// The verb, its target, and its ops, in the one shape they're valid in.
    pub intent: GroupIntent,
    /// The volume every source lives on. A sweep may span volumes; a group may not. Ignored
    /// for a server group, which has no sources: the spine stores the edited place's id, or
    /// `""` for an add (`GroupIntent::stored_source_volume`).
    pub source_volume_id: String,
    /// The friendly name the review dialog leads with. Carries the selector's pattern as
    /// display text when a selector produced the group.
    pub display_name: String,
    /// The agent's reason, shown LABELLED as the agent's words next to facts Cmdr knows.
    pub rationale: Option<String>,
    /// The selector this group froze, as JSON, for display and provenance.
    pub selector: Option<String>,
}

/// Insert a sweep and return its id.
pub fn create_sweep(conn: &Connection, sweep: &NewSweep, now: i64) -> Result<i64, AgentStoreError> {
    conn.prepare_cached(
        "INSERT INTO proposal_sets (conversation_id, created_at, created_by_model, rationale)
         VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![
        sweep.conversation_id,
        now,
        sweep.created_by_model,
        sweep.rationale
    ])?;
    Ok(conn.last_insert_rowid())
}

/// Insert a group and all of its ops into `set_id`, in one transaction, and return the group
/// id. The group's `seq` is the next ordinal in its sweep.
///
/// The op rows go in with `prepare_cached` inside the same transaction, so a 60 000-op group
/// is one commit and one compiled statement rather than 60 000 of either.
pub fn create_group(conn: &Connection, set_id: i64, group: &NewGroup, now: i64) -> Result<i64, AgentStoreError> {
    // IMMEDIATE, not deferred: this reads `MAX(seq)` and then writes. A deferred
    // transaction that upgrades to a writer after another connection wrote gets
    // `SQLITE_BUSY_SNAPSHOT`, which the busy timeout does NOT retry — it would surface as a
    // failed proposal rather than a wait.
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let seq: i64 = tx
        .prepare_cached("SELECT COALESCE(MAX(seq) + 1, 0) FROM proposals WHERE set_id = ?1")?
        .query_row(params![set_id], |row| row.get(0))?;
    let (destination, destination_volume_id) = group.intent.stored_destination();
    tx.prepare_cached(
        "INSERT INTO proposals (
            set_id, seq, verb, status, source_volume_id, destination, destination_volume_id,
            reversible, display_name, rationale, selector, created_at, source_folder
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?
    .execute(params![
        set_id,
        seq,
        group.intent.verb().as_token(),
        ProposalStatus::Pending.as_token(),
        group.intent.stored_source_volume(&group.source_volume_id),
        destination,
        destination_volume_id,
        group.intent.reversibility().as_token(),
        group.display_name,
        group.rationale,
        group.selector,
        now,
        source_folder_of(&group.intent),
    ])?;
    let group_id = tx.last_insert_rowid();
    insert_ops(&tx, group_id, &group.intent, now)?;
    write_beside_ops(&tx, group_id, &group.intent)?;
    // Newest wins, per file, in this same transaction: an older pending op on a file this group
    // names steps aside. Which groups shrank needs no reply: they moved `revision`, which an open
    // review compares on the re-read the caller's announcement triggers.
    supersede_overlaps(&tx, group_id, now)?;
    tx.commit()?;
    Ok(group_id)
}

/// What a group stores beside its ops, in the group's own transaction: a server group's draft
/// (replacing any), and no draft row for any other group. Its own transaction rather than a
/// producer's later write, because a server group's draft IS what it proposes: a pending group
/// without one could never be shown or saved (`DETAILS.md` § A server group).
fn write_beside_ops(tx: &Transaction<'_>, group_id: i64, intent: &GroupIntent) -> Result<(), AgentStoreError> {
    match intent {
        GroupIntent::SaveServer { draft, edit } => write_server_draft(tx, group_id, draft, edit.as_ref()),
        _ => delete_server_draft(tx, group_id),
    }
}

/// Write a group's op rows, numbered from 0.
fn insert_ops(conn: &Connection, group_id: i64, intent: &GroupIntent, now: i64) -> Result<(), AgentStoreError> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO proposal_ops (
            group_id, seq, source_path, destination, status, snapshot_size, snapshot_mtime, snapshot_inode, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (seq, (source_path, destination, snapshot)) in intent.rows().enumerate() {
        stmt.execute(params![
            group_id,
            seq as i64,
            source_path,
            destination,
            OpStatus::Pending.as_token(),
            snapshot.and_then(|s| s.size),
            snapshot.and_then(|s| s.mtime),
            snapshot.and_then(|s| s.inode),
            now,
        ])?;
    }
    Ok(())
}

/// What a re-propose did, or why it didn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReproposeOutcome {
    /// The group's ops, target, and text were replaced, and any acceptance record was torn
    /// up (so the next approval goes through a fresh preflight).
    Reproposed,
    /// The group isn't `pending`, so it's frozen to its author. `Approved`, `Interrupted`,
    /// `Completed`, and `Rejected` groups are the USER's; an agent that could still rewrite
    /// them could rewrite what the user already answered.
    NotPending { found: ProposalStatus },
    /// No group with that id.
    Unknown,
}

/// Replace a PENDING group's op list and text with `group`'s. The one way an op list changes
/// after creation, and the author's own path — the agent re-proposing against a sweep it owns.
///
/// Guarded twice over, both inside one `BEGIN IMMEDIATE`: the conditional `UPDATE ... WHERE
/// status = 'pending'` both checks and locks, and the acceptance record is DELETED, so a
/// preflight taken against the old op list can never carry an approval onto the new one. It also
/// bumps `revision`, so an approval the user gave while reading the OLD version refuses even
/// though it would preflight the new list afresh.
pub fn repropose_group(
    conn: &Connection,
    group_id: i64,
    group: &NewGroup,
    now: i64,
) -> Result<ReproposeOutcome, AgentStoreError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let (destination, destination_volume_id) = group.intent.stored_destination();
    let updated = tx
        .prepare_cached(
            "UPDATE proposals SET
                verb = ?2, source_volume_id = ?3, destination = ?4, destination_volume_id = ?5,
                reversible = ?6, display_name = ?7, rationale = ?8, selector = ?9, created_at = ?10,
                revision = revision + 1, source_folder = ?12
             WHERE id = ?1 AND status = ?11",
        )?
        .execute(params![
            group_id,
            group.intent.verb().as_token(),
            group.intent.stored_source_volume(&group.source_volume_id),
            destination,
            destination_volume_id,
            group.intent.reversibility().as_token(),
            group.display_name,
            group.rationale,
            group.selector,
            now,
            ProposalStatus::Pending.as_token(),
            source_folder_of(&group.intent),
        ])?;

    if updated == 0 {
        // Nothing moved: either the group is gone or it left `pending`. Read which, so the
        // caller gets the typed reason rather than a bare "no".
        let found: Option<String> = tx
            .prepare_cached("SELECT status FROM proposals WHERE id = ?1")?
            .query_row(params![group_id], |row| row.get(0))
            .ok();
        tx.commit()?;
        return match found {
            Some(token) => Ok(ReproposeOutcome::NotPending {
                found: super::decode_token("proposals.status", token, ProposalStatus::from_token)?,
            }),
            None => Ok(ReproposeOutcome::Unknown),
        };
    }

    tx.prepare_cached("DELETE FROM proposal_ops WHERE group_id = ?1")?
        .execute(params![group_id])?;
    tx.prepare_cached("DELETE FROM proposal_acceptances WHERE group_id = ?1")?
        .execute(params![group_id])?;
    insert_ops(&tx, group_id, &group.intent, now)?;
    write_beside_ops(&tx, group_id, &group.intent)?;
    // An amend is the newest word on the files it now names, exactly as a new group is.
    supersede_overlaps(&tx, group_id, now)?;
    tx.commit()?;
    Ok(ReproposeOutcome::Reproposed)
}

/// The folder this intent's ops share, for `proposals.source_folder`. From the in-memory list
/// the write already holds, so it costs no read.
fn source_folder_of(intent: &GroupIntent) -> Option<String> {
    super::shared_parent(intent.rows().map(|(source_path, _, _)| source_path))
}
