//! A `rollback_unit` row as the rollback planner acts on it: its interned dirs resolved, and
//! its stored path texts read as typed app paths through the op's escape mark
//! (`escape_mark.rs`). A row whose text the typed reading refuses comes back as
//! [`UnitReading::Refused`], with the reason the planner records: the undo never acts on it
//! (`DETAILS.md` § "The escape mark").

use cmdr_fs::app_path::{AppPathBuf, Namer};
use cmdr_fs::entry::TagsChange;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::NodeIdentity;
use rusqlite::Connection;

use super::escape_mark::{StoredPathReading, StoredPathRefusal, stored_path_reading};
use super::{OperationItemRow, OperationLogStoreError, awaits_reversal, join_leaf_text, resolve_dir};
use crate::types::{EntryType, ItemOutcome, SkipReason};

/// One `rollback_unit` row with its interned dir prefixes resolved to full paths
/// and real volume ids — everything the rollback engine needs to reverse the
/// item without a second query per row. `source` is the item's original location;
/// `dest` (present for copy/move/trash/rename) is where the item ended up (the
/// thing a restore-move brings back, or a remove-inverse deletes).
///
/// `P` is how the paths are held: typed app paths for a row the planner acts on, the
/// stored text for one it refused ([`UnitReading::Refused`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RollbackUnit<P = AppPathBuf> {
    /// The item's position within its operation.
    pub seq: i64,
    /// File, directory, or symlink.
    pub entry_type: EntryType,
    /// The volume of the item's original location.
    pub source_volume_id: String,
    /// The item's original location.
    pub source_path: P,
    /// The volume it ended up on.
    pub dest_volume_id: Option<String>,
    /// Where it ended up.
    pub dest_path: Option<P>,
    /// The size snapshot the reversal verifies against.
    pub size: Option<i64>,
    /// The mtime snapshot the reversal verifies against.
    pub mtime: Option<i64>,
    /// Whether landing it replaced an existing entry.
    pub overwrote: bool,
    /// What happened to the item.
    pub outcome: ItemOutcome,
    /// Why an earlier rollback held the item back, when one did. Present only
    /// alongside [`ItemOutcome::Skipped`].
    pub rollback_skip_reason: Option<SkipReason>,
    /// Which node the item landed as at `dest_path`, when that was recorded. A
    /// restore moves back only that node, never a stranger under its name.
    pub dest_identity: Option<NodeIdentity>,
    /// A tag write's value before and after. The undo writes `before` back only
    /// while the file still reads `after`.
    pub tags: Option<TagsChange>,
}

impl<P> RollbackUnit<P> {
    /// Whether a reversal still owes this item an attempt: the operation landed it
    /// (`done`), or an earlier rollback held it back (`skipped` WITH a rollback
    /// verdict). A `skipped` row without one is the operation's own skip, a log entry
    /// that never moved anything. The in-code twin of `AWAITS_REVERSAL_SQL`.
    pub(crate) fn awaits_reversal(&self) -> bool {
        awaits_reversal(self.outcome, self.rollback_skip_reason)
    }

    /// The same row with each path mapped through `f`.
    pub(crate) fn map_paths<Q>(self, mut f: impl FnMut(P) -> Q) -> RollbackUnit<Q> {
        let RollbackUnit {
            seq,
            entry_type,
            source_volume_id,
            source_path,
            dest_volume_id,
            dest_path,
            size,
            mtime,
            overwrote,
            outcome,
            rollback_skip_reason,
            dest_identity,
            tags,
        } = self;
        RollbackUnit {
            seq,
            entry_type,
            source_volume_id,
            source_path: f(source_path),
            dest_volume_id,
            dest_path: dest_path.map(f),
            size,
            mtime,
            overwrote,
            outcome,
            rollback_skip_reason,
            dest_identity,
            tags,
        }
    }

    /// The same row with each path held another way.
    fn with_paths<Q>(self, source_path: Q, dest_path: Option<Q>) -> RollbackUnit<Q> {
        RollbackUnit {
            seq: self.seq,
            entry_type: self.entry_type,
            source_volume_id: self.source_volume_id,
            source_path,
            dest_volume_id: self.dest_volume_id,
            dest_path,
            size: self.size,
            mtime: self.mtime,
            overwrote: self.overwrote,
            outcome: self.outcome,
            rollback_skip_reason: self.rollback_skip_reason,
            dest_identity: self.dest_identity,
            tags: self.tags,
        }
    }
}

/// One row as the planner reads it. Crate-internal: the planner is its one reader, and a
/// cell outside reads its own rows through `read_typed_rollback_units`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UnitReading {
    /// Every path read as a typed app path: the planner verifies and acts on exactly these.
    Read(RollbackUnit),
    /// A stored path the typed reading refuses. The planner records `reason` for the item
    /// and never acts on it.
    Refused {
        /// The row, its paths the text it stores.
        unit: RollbackUnit<String>,
        /// What the planner records for it.
        reason: SkipReason,
    },
}

impl UnitReading {
    /// The typed unit, or `None` for a refused row.
    pub(crate) fn into_read(self) -> Option<RollbackUnit> {
        match self {
            UnitReading::Read(unit) => Some(unit),
            UnitReading::Refused { .. } => None,
        }
    }
}

/// How one op's rows read: its escape mark's answer, and the host's namer for each volume.
pub(crate) struct UnitReader<'a> {
    reading: StoredPathReading,
    namer_of: &'a (dyn Fn(&str) -> Option<Namer> + Sync),
}

impl<'a> UnitReader<'a> {
    /// The reader for `op_id`'s rows. `namer_of` answers a volume id's namer, `None` for a
    /// volume the host can't name (the rollback's `VolumeLookup::namer`).
    pub(crate) fn for_op(
        conn: &Connection,
        op_id: &str,
        namer_of: &'a (dyn Fn(&str) -> Option<Namer> + Sync),
    ) -> Result<Self, OperationLogStoreError> {
        Ok(Self {
            reading: stored_path_reading(conn, op_id)?,
            namer_of,
        })
    }

    /// Resolve one raw row's interned dirs and read its paths. Two dir resolutions per row,
    /// so the engine calls it only for a row it will act on.
    pub(crate) fn unit_of(
        &self,
        conn: &Connection,
        item: &OperationItemRow,
    ) -> Result<UnitReading, OperationLogStoreError> {
        let (source_volume_id, source_dir) = resolve_dir(conn, item.source_dir_id)?;
        let source_text = join_leaf_text(&source_dir, &item.source_name);
        let (dest_volume_id, dest_text) = match (item.dest_dir_id, &item.dest_name) {
            (Some(dir_id), Some(name)) => {
                let (vol, dir) = resolve_dir(conn, dir_id)?;
                (Some(vol), Some(join_leaf_text(&dir, name)))
            }
            _ => (None, None),
        };
        let stored = RollbackUnit {
            seq: item.seq,
            entry_type: item.entry_type,
            source_volume_id,
            source_path: (),
            dest_volume_id,
            dest_path: None,
            size: item.size,
            mtime: item.mtime,
            overwrote: item.overwrote,
            outcome: item.outcome,
            rollback_skip_reason: item.rollback_skip_reason,
            dest_identity: item.dest_identity,
            tags: item.tags.clone(),
        };
        let source = self.path_of(&stored.source_volume_id, &source_text);
        let dest = match (&stored.dest_volume_id, &dest_text) {
            (Some(vol), Some(text)) => Some(self.path_of(vol, text)),
            _ => None,
        };
        Ok(match (source, dest.transpose()) {
            (Ok(source), Ok(dest)) => UnitReading::Read(stored.with_paths(source, dest)),
            (Err(reason), _) | (_, Err(reason)) => UnitReading::Refused {
                unit: stored.with_paths(source_text, dest_text),
                reason,
            },
        })
    }

    /// One stored path as the app path it names, or the skip its refusal records: text that
    /// is no app path is [`SkipReason::UnreadablePath`]; a volume the host can't name is the
    /// planner's "can't tell" for a volume it can't reach, [`SkipReason::Failed`].
    fn path_of(&self, volume_id: &str, text: &str) -> Result<AppPathBuf, SkipReason> {
        self.reading
            .app_path(text, || (self.namer_of)(volume_id))
            .map_err(|refusal| {
                log::warn!(
                    target: "operation_log",
                    "rollback: a stored path names no place undo can act on ({refusal:?}): {}",
                    LogPrivate::path(text)
                );
                match refusal {
                    StoredPathRefusal::NotAnAppPath(_) => SkipReason::UnreadablePath,
                    StoredPathRefusal::UnknownNamer => SkipReason::Failed,
                }
            })
    }
}
