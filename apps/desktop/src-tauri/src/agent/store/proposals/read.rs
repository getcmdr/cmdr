//! Reading the spine back: group headers, op pages, and counts.
//!
//! Everything here is sized for a group of 60 000 ops, which is a legitimate group:
//!
//! - **Counts come from `COUNT(*)`** ([`count_ops`]), ❌ never from the length of a loaded
//!   list. A summary of a 60 000-op group must not cost 60 000 rows.
//! - **Ops are read PAGED and ordered by `(group_id, seq)`** ([`page_ops`]), which is the
//!   unique index, so a page is an index range scan rather than a sort.
//!
//! [`page_ops`] is the only function in the whole module that materializes op rows. The
//! claim path deliberately doesn't call it (`claim.rs` streams instead), and a test pins
//! that.

use cmdr_fs::app_path::AppPathBuf;
use rusqlite::{Connection, OptionalExtension, params};

use super::super::AgentStoreError;
use super::super::ServerDraftProblem;
use super::draft::{ServerDraft, ServerEdit, ServerProposal};
use super::{ProposalGroup, decode_token};
use crate::agent::types::ServerSaveNote;
use crate::agent::types::{OpStatus, ProposalStatus, ProposalVerb, Reversibility};

/// One op as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalOp {
    pub id: i64,
    pub group_id: i64,
    pub seq: i64,
    pub source_path: String,
    /// The name this source becomes: always under a rename group, and under a move or copy
    /// when it lands under a new one. `None` everywhere else.
    pub destination: Option<String>,
    pub status: OpStatus,
    pub snapshot_size: Option<u64>,
    pub snapshot_mtime: Option<i64>,
    pub snapshot_inode: Option<u64>,
}

/// One sweep as stored, header only: what a list surface shows above its groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalSweep {
    pub id: i64,
    /// The chat thread this came out of, when it came out of one. NULLed rather than
    /// cascaded when that thread is deleted, so the decision record outlives it.
    pub conversation_id: Option<i64>,
    pub created_at: i64,
    /// Provenance only: no logic reads it (agent decision D32).
    pub created_by_model: Option<String>,
    /// The agent's words for the sweep as a whole.
    pub rationale: Option<String>,
}

/// A group header plus its op count: what a list surface needs, without a single op row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    pub group: ProposalGroup,
    /// How many ops are in the group's LIVE set (`pending`); a deselected op is excluded
    /// from it and stays as a row.
    pub live_op_count: u64,
    /// Every op row the group has, whatever its status.
    pub total_op_count: u64,
    /// What a server group proposes; `None` for a file group. A server group whose draft is
    /// missing fails the read rather than arriving without one.
    pub server: Option<ServerProposal>,
}

const GROUP_COLUMNS: &str = "id, set_id, seq, verb, status, source_volume_id, destination, destination_volume_id,
     reversible, display_name, rationale, selector, created_at, decided_at, revision, source_folder";

/// Read one group header. `None` when there's no such group.
pub fn get_group(conn: &Connection, group_id: i64) -> Result<Option<ProposalGroup>, AgentStoreError> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {GROUP_COLUMNS} FROM proposals WHERE id = ?1"))?;
    let mut rows = stmt.query(params![group_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(map_group_row(row)?)),
        None => Ok(None),
    }
}

/// Read one sweep header. `None` when there's no such sweep.
///
/// Read one at a time rather than as a whole-table list: a caller lists GROUPS (filtered,
/// counted) and then names the handful of sweeps those groups belong to, so nothing ever
/// scans every sweep a long-lived `main.db` has accumulated.
pub fn get_sweep(conn: &Connection, set_id: i64) -> Result<Option<ProposalSweep>, AgentStoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, conversation_id, created_at, created_by_model, rationale FROM proposal_sets WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![set_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(ProposalSweep {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            created_at: row.get(2)?,
            created_by_model: row.get(3)?,
            rationale: row.get(4)?,
        })),
        None => Ok(None),
    }
}

/// The groups of ONE sweep the user turned down at or after `since`, in proposal order.
///
/// Scoped by `decided_at` rather than by status alone so a rejection weeks later reports only
/// itself: the follow-up turn asks "why did you say no to THIS?", and re-listing every group
/// the sweep ever lost would teach the agent the same lesson twice.
pub fn rejected_groups_since(
    conn: &Connection,
    set_id: i64,
    since: i64,
) -> Result<Vec<ProposalGroup>, AgentStoreError> {
    let sql = format!(
        "SELECT {GROUP_COLUMNS} FROM proposals
         WHERE set_id = ?1 AND status = ?2 AND decided_at IS NOT NULL AND decided_at >= ?3
         ORDER BY seq"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![set_id, ProposalStatus::Rejected.as_token(), since])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_group_row(row)?);
    }
    Ok(out)
}

/// Group summaries, newest first, optionally filtered to one status. Counts come from
/// `COUNT(*)` subqueries, so this stays cheap however large the groups are.
pub fn list_groups(conn: &Connection, status: Option<ProposalStatus>) -> Result<Vec<GroupSummary>, AgentStoreError> {
    let sql = format!(
        "SELECT {GROUP_COLUMNS},
                (SELECT COUNT(*) FROM proposal_ops o WHERE o.group_id = p.id AND o.status = ?2) AS live_ops,
                (SELECT COUNT(*) FROM proposal_ops o WHERE o.group_id = p.id) AS total_ops
         FROM proposals p
         WHERE (?1 IS NULL OR p.status = ?1)
         ORDER BY p.created_at DESC, p.id DESC"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params![
        status.map(ProposalStatus::as_token),
        OpStatus::Pending.as_token()
    ])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(GroupSummary {
            group: map_group_row(row)?,
            live_op_count: row.get(16)?,
            total_op_count: row.get(17)?,
            server: None,
        });
    }
    // One sidecar read per SERVER group only: those are few, and a file group costs nothing.
    for summary in &mut out {
        if summary.group.verb.is_server() {
            summary.server = Some(read_sidecar(conn, summary.group.id)?);
        }
    }
    Ok(out)
}

/// How many ops the group has in `status`. The ONLY way anything asks "how many": a group of
/// 60 000 must never be loaded to be counted.
pub fn count_ops(conn: &Connection, group_id: i64, status: Option<OpStatus>) -> Result<u64, AgentStoreError> {
    let count: i64 = conn
        .prepare_cached("SELECT COUNT(*) FROM proposal_ops WHERE group_id = ?1 AND (?2 IS NULL OR status = ?2)")?
        .query_row(params![group_id, status.map(OpStatus::as_token)], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}

/// How many groups sit in `status`, and how many live ops they hold between them.
///
/// One statement, both `COUNT(*)`: the indicator that consumes this is always mounted, and a
/// group of 60 000 ops is legitimate, so drawing a badge must never cost a row.
pub fn count_pending(conn: &Connection, status: ProposalStatus) -> Result<(u64, u64), AgentStoreError> {
    let (groups, ops): (i64, i64) = conn
        .prepare_cached(
            "SELECT COUNT(*),
                    COALESCE((SELECT COUNT(*) FROM proposal_ops o
                              JOIN proposals p2 ON p2.id = o.group_id
                              WHERE p2.status = ?1 AND o.status = ?2), 0)
             FROM proposals WHERE status = ?1",
        )?
        .query_row(params![status.as_token(), OpStatus::Pending.as_token()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    Ok((groups.max(0) as u64, ops.max(0) as u64))
}

/// One page of a group's ops, ordered by `seq` — the index order, so no sort happens.
///
/// The only op-row-materializing read in the module. Everything that needs a whole group's
/// worth of ops (the review dialog, the executor) pages through this; ❌ nothing calls it to
/// count, compare, or claim.
pub fn page_ops(conn: &Connection, group_id: i64, limit: u32, offset: u32) -> Result<Vec<ProposalOp>, AgentStoreError> {
    #[cfg(test)]
    tests_support::note_page_ops_call();

    let mut stmt = conn.prepare_cached(
        "SELECT id, group_id, seq, source_path, destination, status, snapshot_size, snapshot_mtime, snapshot_inode
         FROM proposal_ops WHERE group_id = ?1 ORDER BY seq LIMIT ?2 OFFSET ?3",
    )?;
    let mut rows = stmt.query(params![group_id, limit, offset])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(ProposalOp {
            id: row.get(0)?,
            group_id: row.get(1)?,
            seq: row.get(2)?,
            source_path: row.get(3)?,
            destination: row.get(4)?,
            status: decode_token("proposal_ops.status", row.get(5)?, OpStatus::from_token)?,
            snapshot_size: row.get(6)?,
            snapshot_mtime: row.get(7)?,
            snapshot_inode: row.get(8)?,
        });
    }
    Ok(out)
}

/// The server proposal a group holds: `Ok(None)` for a file group or no group at all, and a
/// typed [`AgentStoreError::ServerDraft`] for a server group whose draft is missing or won't
/// decode. ❌ Never a default draft: a card with invented fields is one the user could approve.
pub fn read_server_proposal(conn: &Connection, group_id: i64) -> Result<Option<ServerProposal>, AgentStoreError> {
    match get_group(conn, group_id)? {
        Some(group) if group.verb.is_server() => read_sidecar(conn, group_id).map(Some),
        _ => Ok(None),
    }
}

/// A draft row as stored: `draft`, `editing_id`, `previous`, `completion_note`.
type DraftRow = (String, Option<String>, Option<String>, Option<String>);

/// The draft row of a group KNOWN to be a server group, decoded, or the typed failure.
pub(super) fn read_sidecar(conn: &Connection, group_id: i64) -> Result<ServerProposal, AgentStoreError> {
    let row: Option<DraftRow> = conn
        .prepare_cached(
            "SELECT draft, editing_id, previous, completion_note FROM proposal_server_drafts WHERE group_id = ?1",
        )?
        .query_row(params![group_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .optional()?;
    let fail = |problem| AgentStoreError::ServerDraft { group_id, problem };
    let Some((draft, editing, previous, note)) = row else {
        return Err(fail(ServerDraftProblem::Missing));
    };
    let decode =
        |json: &str| serde_json::from_str::<ServerDraft>(json).map_err(|_| fail(ServerDraftProblem::Undecodable));
    let edit = match (editing, previous) {
        (Some(editing), Some(previous)) => Some(ServerEdit {
            editing,
            previous: decode(&previous)?,
        }),
        (None, None) => None,
        // The table's CHECK holds them together; a hand-edited file is the only way here.
        _ => return Err(fail(ServerDraftProblem::Undecodable)),
    };
    let note = match note {
        Some(token) => Some(ServerSaveNote::from_token(&token).ok_or_else(|| fail(ServerDraftProblem::UnknownNote))?),
        None => None,
    };
    Ok(ServerProposal {
        draft: decode(&draft)?,
        edit,
        note,
    })
}

/// A stored destination, read as the app path it was written as ([`super::typed_place`]). A
/// row an older build wrote with a `..` in it names no place without its volume's answer,
/// which nothing can ask now: it reads as no destination, so its approval refuses as
/// `TargetMissing`, ❌ never a lexical guess.
fn typed_destination(group_id: i64, stored: Option<String>) -> Option<AppPathBuf> {
    let text = stored?;
    let typed = super::typed_place(&text);
    if typed.is_none() {
        log::warn!(
            target: "agent::store",
            "group {group_id}: its stored destination names no place, so it reads as none"
        );
    }
    typed
}

fn map_group_row(row: &rusqlite::Row<'_>) -> Result<ProposalGroup, AgentStoreError> {
    Ok(ProposalGroup {
        id: row.get(0)?,
        set_id: row.get(1)?,
        seq: row.get(2)?,
        verb: decode_token("proposals.verb", row.get(3)?, ProposalVerb::from_token)?,
        status: decode_token("proposals.status", row.get(4)?, ProposalStatus::from_token)?,
        source_volume_id: row.get(5)?,
        destination: typed_destination(row.get(0)?, row.get(6)?),
        destination_volume_id: row.get(7)?,
        reversible: decode_token("proposals.reversible", row.get(8)?, Reversibility::from_token)?,
        display_name: row.get(9)?,
        rationale: row.get(10)?,
        selector: row.get(11)?,
        created_at: row.get(12)?,
        decided_at: row.get(13)?,
        revision: row.get(14)?,
        source_folder: row.get(15)?,
    })
}

/// A call counter that exists only under `cfg(test)`, so a test can assert that a code path
/// (the claim transaction) never materializes op rows. Compiled out of every real build.
///
/// THREAD-LOCAL, not a global: the test harness runs tests in parallel threads in one
/// process, so a shared counter would read another test's calls and the assertion would pass
/// or fail by timing.
#[cfg(test)]
pub(super) mod tests_support {
    use std::cell::Cell;

    thread_local! {
        static PAGE_OPS_CALLS: Cell<u64> = const { Cell::new(0) };
    }

    pub(in crate::agent::store::proposals) fn note_page_ops_call() {
        PAGE_OPS_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    /// How many times `page_ops` has run on THIS thread.
    pub(in crate::agent::store::proposals) fn page_ops_calls() -> u64 {
        PAGE_OPS_CALLS.with(Cell::get)
    }
}
