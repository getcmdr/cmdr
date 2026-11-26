//! Two groups naming one file (#1524): which op steps aside, and when a group has nothing left.
//!
//! Nothing stops the agent proposing the same file in two pending groups, a night apart. Left
//! alone, the older op stays approvable after the newer one (or any other run) already moved the
//! file, and its approval then finds nothing at the old path. So the overlap is settled at the
//! proposal boundary, ❌ never on the execution path:
//!
//! - **Newest wins, per file** ([`supersede_overlaps`], inside the creating or amending
//!   transaction): the older op becomes `superseded`.
//! - **A run that moved a file away** ([`retire_handled_elsewhere`], at settle) makes every other
//!   pending op on that path `handled_elsewhere`.
//!
//! Either way the op keeps its ROW (the decision record says what was offered), the group's
//! `revision` moves (an approval of the version the user read refuses as `listChanged` rather
//! than running fewer files unannounced), and a group left with nothing to offer is `retired`.
//! Depth: `DETAILS.md` § Two groups naming one file.

use rusqlite::{Connection, Transaction, TransactionBehavior, params};

use super::super::AgentStoreError;
use super::get_group;
use crate::agent::types::{OpStatus, ProposalStatus};

/// Which other groups lost ops to a newer suggestion or a run, in id order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Retirement {
    /// Groups that still have something to offer, now with fewer ops.
    pub shrunk: Vec<i64>,
    /// Groups left with nothing to offer, now `retired`.
    pub retired: Vec<i64>,
}

impl Retirement {
    /// Whether any other group changed.
    pub fn is_empty(&self) -> bool {
        self.shrunk.is_empty() && self.retired.is_empty()
    }
}

/// Newest wins: every OTHER pending group's still-offered op (`pending` or `excluded`) on a file
/// `group_id` names becomes `superseded`. Runs inside the transaction that wrote `group_id`'s ops,
/// so the new group and the older ones it takes files from commit as one.
pub(super) fn supersede_overlaps(conn: &Connection, group_id: i64, now: i64) -> Result<Retirement, AgentStoreError> {
    retire_overlaps(conn, group_id, OpStatus::Pending, OpStatus::Superseded, now)
}

/// A settled run's `done` ops moved their files away, so every other pending group's op still
/// naming one of those paths becomes `handled_elsewhere`, in one `BEGIN IMMEDIATE`. A run whose
/// verb only READS its sources (copy, extract, compress) changes nothing here: the path still
/// means the file the other ops meant.
pub fn retire_handled_elsewhere(conn: &Connection, group_id: i64, now: i64) -> Result<Retirement, AgentStoreError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let Some(group) = get_group(&tx, group_id)? else {
        return Ok(Retirement::default());
    };
    if !group.verb.moves_its_sources() {
        return Ok(Retirement::default());
    }
    let retirement = retire_overlaps(&tx, group_id, OpStatus::Done, OpStatus::HandledElsewhere, now)?;
    tx.commit()?;
    Ok(retirement)
}

/// Turn every other PENDING group's still-offered op on a path `from_group`'s `from_status` ops
/// name into `as_status`, bump each touched group's revision, and retire the ones left with
/// nothing to offer. The caller holds the transaction.
///
/// Same volume only: the same text on another volume is another file. Paths compare as stored
/// text, which is how every producer spells them (the drive index's own paths).
fn retire_overlaps(
    conn: &Connection,
    from_group: i64,
    from_status: OpStatus,
    as_status: OpStatus,
    now: i64,
) -> Result<Retirement, AgentStoreError> {
    let mut touched: Vec<i64> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(
            "UPDATE proposal_ops SET status = ?4
             WHERE status IN (?5, ?6)
               AND group_id IN (
                   SELECT id FROM proposals
                   WHERE status = ?3 AND id <> ?1
                     AND source_volume_id = (SELECT source_volume_id FROM proposals WHERE id = ?1)
               )
               AND source_path IN (SELECT source_path FROM proposal_ops WHERE group_id = ?1 AND status = ?2)
             RETURNING group_id",
        )?;
        let mut rows = stmt.query(params![
            from_group,
            from_status.as_token(),
            ProposalStatus::Pending.as_token(),
            as_status.as_token(),
            OpStatus::Pending.as_token(),
            OpStatus::Excluded.as_token(),
        ])?;
        while let Some(row) = rows.next()? {
            touched.push(row.get(0)?);
        }
    }
    touched.sort_unstable();
    touched.dedup();

    let mut retirement = Retirement::default();
    for group_id in touched {
        let retired = conn
            .prepare_cached(
                "UPDATE proposals SET status = ?2, decided_at = ?3, revision = revision + 1
                 WHERE id = ?1 AND status = ?4
                   AND NOT EXISTS (
                       SELECT 1 FROM proposal_ops WHERE group_id = ?1 AND status IN (?5, ?6)
                   )",
            )?
            .execute(params![
                group_id,
                ProposalStatus::Retired.as_token(),
                now,
                ProposalStatus::Pending.as_token(),
                OpStatus::Pending.as_token(),
                OpStatus::Excluded.as_token(),
            ])?;
        if retired == 1 {
            retirement.retired.push(group_id);
        } else {
            conn.prepare_cached("UPDATE proposals SET revision = revision + 1 WHERE id = ?1")?
                .execute(params![group_id])?;
            retirement.shrunk.push(group_id);
        }
    }
    Ok(retirement)
}
