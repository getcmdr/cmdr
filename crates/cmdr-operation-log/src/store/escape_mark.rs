//! The escape mark: which rows were written before the app path's escapes went live, so
//! their stored paths read as the literal text they held (`crates/cmdr-fs/DETAILS.md` §
//! "App paths"; review 2's N7 of the typed app path, #1225).
//!
//! A row written before then can hold an escape-range code point as LITERAL text (a file
//! really named with U+10FEC3). Read live, `[U+10FEC3, U+10FEA9]` spells the bytes of `é`,
//! so a rollback of that copy would act on `é.txt`, a real, different file. The mark is one
//! `meta` row, stamped ONCE, at the first open by a build whose escapes are live
//! (`cmdr_fs::app_path::ESCAPES_LIVE`): the newest op id then on record. Op ids are UUIDv7,
//! which sort by time as text, so an op at or below the mark is older. No row changes shape,
//! so no migration (the plan's loud rule 4).

use cmdr_fs::app_path::{AppPathBuf, Namer, NotAnAppPath};
use rusqlite::{Connection, OptionalExtension};

use super::OperationLogStoreError;

/// The `meta` key holding the newest op id written before escapes went live (empty when the
/// log held none).
pub(crate) const ESCAPE_MARK_KEY: &str = "escape_mark_last_literal_op_id";

/// How an op's stored paths read. Crate-internal until its first reader, the rollback
/// planner's typed conversion (the app-path plan's milestone 8), lands beside it.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the rollback planner's typed conversion is its first caller")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredPathReading {
    /// Written before escapes went live: every escape-range code point is literal text.
    Literal,
    /// Written since: the stored text is the app spelling, its escapes live.
    Live,
}

impl StoredPathReading {
    /// The app path a stored path text names, read as this op's rows were written. `namer` is
    /// the namer of the volume the path is on.
    ///
    /// # Errors
    ///
    /// As [`AppPathBuf::from_wire`].
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the rollback planner's typed conversion is its first caller")
    )]
    pub(crate) fn app_path(self, text: &str, namer: Namer) -> Result<AppPathBuf, NotAnAppPath> {
        match self {
            StoredPathReading::Literal => AppPathBuf::from_literal_store(text, namer),
            StoredPathReading::Live => AppPathBuf::from_wire(text),
        }
    }
}

/// Stamp the mark if escapes are live and it isn't there yet. Idempotent: a later open finds
/// it and leaves it alone, so ops written since read live. Runs on every write open.
pub(crate) fn stamp_escape_mark(conn: &Connection, escapes_live: bool) -> Result<(), OperationLogStoreError> {
    if !escapes_live {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO meta (key, value)
         VALUES (?1, COALESCE((SELECT MAX(op_id) FROM operations), ''))",
        rusqlite::params![ESCAPE_MARK_KEY],
    )?;
    Ok(())
}

/// How `op_id`'s stored paths read: [`StoredPathReading::Literal`] for every op while no mark
/// is stamped (escapes aren't live yet, so no stored text holds one) and for an op at or below
/// the mark; [`StoredPathReading::Live`] for one written since.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the rollback planner's typed conversion is its first caller")
)]
pub(crate) fn stored_path_reading(conn: &Connection, op_id: &str) -> Result<StoredPathReading, OperationLogStoreError> {
    let mark: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            rusqlite::params![ESCAPE_MARK_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match mark {
        Some(last_literal) if op_id > last_literal.as_str() => StoredPathReading::Live,
        _ => StoredPathReading::Literal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations::{MIGRATIONS, run_migrations};

    fn db() -> Connection {
        let conn = cmdr_fs::sqlite_util::open_in_memory().expect("in memory");
        run_migrations(&conn, MIGRATIONS).expect("migrates");
        conn
    }

    fn add_op(conn: &Connection, op_id: &str) {
        conn.execute(
            "INSERT INTO operations (op_id, kind, initiator, execution_status, rollback_state, started_at,
                                     item_count, items_done, bytes_total, search_coverage)
             VALUES (?1, 'copy', 'user', 'done', 'rollbackable', 0, 0, 0, 0, 'full')",
            rusqlite::params![op_id],
        )
        .expect("insert op");
    }

    const OLD: &str = "0190a000-0000-7000-8000-000000000001";
    const NEWER: &str = "0190a000-0000-7000-8000-000000000002";
    const NEW: &str = "0190b000-0000-7000-8000-000000000001";

    #[test]
    fn every_op_reads_literal_until_escapes_go_live() {
        let conn = db();
        add_op(&conn, OLD);
        stamp_escape_mark(&conn, false).expect("no-op");
        assert_eq!(stored_path_reading(&conn, OLD).ok(), Some(StoredPathReading::Literal));
        assert_eq!(stored_path_reading(&conn, NEW).ok(), Some(StoredPathReading::Literal));
    }

    #[test]
    fn the_mark_is_stamped_once_and_splits_older_ops_from_newer() {
        let conn = db();
        add_op(&conn, OLD);
        add_op(&conn, NEWER);
        stamp_escape_mark(&conn, true).expect("stamps");
        add_op(&conn, NEW);
        // A later open leaves the mark where it was.
        stamp_escape_mark(&conn, true).expect("idempotent");
        assert_eq!(stored_path_reading(&conn, OLD).ok(), Some(StoredPathReading::Literal));
        assert_eq!(stored_path_reading(&conn, NEWER).ok(), Some(StoredPathReading::Literal));
        assert_eq!(stored_path_reading(&conn, NEW).ok(), Some(StoredPathReading::Live));
    }

    #[test]
    fn an_empty_log_marks_nothing_old() {
        let conn = db();
        stamp_escape_mark(&conn, true).expect("stamps");
        add_op(&conn, OLD);
        assert_eq!(stored_path_reading(&conn, OLD).ok(), Some(StoredPathReading::Live));
    }

    /// Review 2's N7 input: a pre-mark row that literally held two escape code points names
    /// that file, never `é.txt`.
    #[test]
    fn a_pre_mark_row_names_the_file_it_named() {
        let root = if cfg!(windows) { "C:/d" } else { "/d" };
        let stored = format!("{root}/\u{10FEC3}\u{10FEA9}.txt");
        let live = StoredPathReading::Live.app_path(&stored, Namer::HOST).expect("parses");
        assert_eq!(
            live.as_str(),
            format!("{root}/\u{e9}.txt"),
            "read live, it would name `é.txt`"
        );
        let literal = StoredPathReading::Literal
            .app_path(&stored, Namer::HOST)
            .expect("parses");
        assert_ne!(literal, live);
        // What reaches the OS is the text the row held, code point for code point.
        let native = literal.to_native().expect("a local path");
        let held = native.to_str().expect("valid text");
        assert!(held.ends_with("\u{10FEC3}\u{10FEA9}.txt"), "{held:?}");
    }
}
