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

use std::borrow::Cow;

use cmdr_fs::app_path::{AppPathBuf, Namer, NotAnAppPath, PathFlavor, is_escape, is_scheme_url};
use rusqlite::{Connection, OptionalExtension};

use super::OperationLogStoreError;

/// The `meta` key holding the newest op id written before escapes went live (empty when the
/// log held none).
pub(crate) const ESCAPE_MARK_KEY: &str = "escape_mark_last_literal_op_id";

/// How an op's stored paths read. Its reader is the rollback planner's typed conversion
/// (`rollback_unit.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredPathReading {
    /// Written before escapes went live: every escape-range code point is literal text.
    Literal,
    /// Written since: the stored text is the app spelling, its escapes live.
    Live,
}

impl StoredPathReading {
    /// The app path a stored path text names, read as this op's rows were written. `namer`
    /// answers the namer of the volume the path is on, and is asked only for literal text
    /// holding an escape-range code point: no other text spells differently through either
    /// namer, so a row on a volume the host can't name still reads as it always did.
    ///
    /// # Errors
    ///
    /// [`StoredPathRefusal::NotAnAppPath`] as [`AppPathBuf::from_wire`], and
    /// [`StoredPathRefusal::UnknownNamer`] when the text needs a namer `namer` can't give.
    pub(crate) fn app_path(
        self,
        text: &str,
        namer: impl FnOnce() -> Option<Namer>,
    ) -> Result<AppPathBuf, StoredPathRefusal> {
        self.app_path_as(text, namer, PathFlavor::HOST)
    }

    /// [`Self::app_path`] under `host`'s rules, so a cell runs a Windows journal's rows on
    /// every OS.
    ///
    /// On a Windows host a row's text reads by its own shape, as the engine's `PathBuf`
    /// meant it when it was journaled (`local_split` is `to_string_lossy` of it):
    /// - **A drive or UNC row**: `\` separates, as Win32 reads it. `Path::join` writes `\`
    ///   there, so every row below a copied folder's top holds one (`C:/src\sub\a.txt`),
    ///   and no name on a Windows disk can.
    /// - **A POSIX `/` row** (the in-memory volume's, a POSIX server's): POSIX rules, so a `\`
    ///   stays the name character it is on that server.
    ///
    /// On macOS and Linux every row reads POSIX, `\` a name character, as it always did.
    pub(crate) fn app_path_as(
        self,
        text: &str,
        namer: impl FnOnce() -> Option<Namer>,
        host: PathFlavor,
    ) -> Result<AppPathBuf, StoredPathRefusal> {
        let posix_shaped = text.starts_with('/') && !text.starts_with("//");
        let flavor = if posix_shaped { PathFlavor::Posix } else { host };
        if self == StoredPathReading::Literal && text.chars().any(is_escape) {
            let namer = namer().ok_or(StoredPathRefusal::UnknownNamer)?;
            // Every escape-range code point literal, and on Windows a drive's `\` a separator.
            return Ok(AppPathBuf::from_external_text(text, flavor, namer)?);
        }
        let separated = if flavor == PathFlavor::Windows && !is_scheme_url(text) {
            Cow::Owned(text.replace('\\', "/"))
        } else {
            Cow::Borrowed(text)
        };
        Ok(AppPathBuf::parse(&separated, flavor)?)
    }
}

/// Why a stored path text names no app path the planner can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredPathRefusal {
    /// The text is no app path: relative, a `..` name, or anything else
    /// [`AppPathBuf::from_wire`] refuses.
    NotAnAppPath(NotAnAppPath),
    /// The text holds a literal escape-range code point, which spells through its volume's
    /// namer, and the host can't name that volume.
    UnknownNamer,
}

impl From<NotAnAppPath> for StoredPathRefusal {
    fn from(why: NotAnAppPath) -> Self {
        StoredPathRefusal::NotAnAppPath(why)
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
        let live = StoredPathReading::Live.app_path(&stored, || None).expect("parses");
        assert_eq!(
            live.as_str(),
            format!("{root}/\u{e9}.txt"),
            "read live, it would name `é.txt`"
        );
        let literal = StoredPathReading::Literal
            .app_path(&stored, || Some(Namer::HOST))
            .expect("parses");
        assert_ne!(literal, live);
        // What reaches the OS is the text the row held, code point for code point.
        let native = literal.to_native().expect("a local path");
        let held = native.to_str().expect("valid text");
        assert!(held.ends_with("\u{10FEC3}\u{10FEA9}.txt"), "{held:?}");
    }

    fn read_as(reading: StoredPathReading, text: &str, host: PathFlavor) -> Result<String, StoredPathRefusal> {
        reading
            .app_path_as(text, || Some(Namer::Units), host)
            .map(|path| path.as_str().to_owned())
    }

    /// A Windows journal's row below a copied folder's top holds `\`: `Path::join` wrote it
    /// between the bridged `C:/src` and each name, and Win32 read it as a separator. The undo
    /// reads it the same way, ❌ never refuses it (a refused row is a partial undo).
    #[test]
    fn a_windows_drive_row_reads_its_backslashes_as_separators() {
        for reading in [StoredPathReading::Literal, StoredPathReading::Live] {
            let windows = |text| read_as(reading, text, PathFlavor::Windows);
            assert_eq!(windows(r"C:/src\sub\a.txt").as_deref(), Ok("C:/src/sub/a.txt"));
            assert_eq!(windows(r"c:\src\a.txt").as_deref(), Ok("C:/src/a.txt"));
            assert_eq!(
                windows(r"//test_server/share/dir\a.txt").as_deref(),
                Ok("//test_server/share/dir/a.txt")
            );
            // A POSIX row on a Windows host (the in-memory volume, a POSIX server) keeps its
            // `\` as the name character it is there, as does a scheme row.
            assert_eq!(windows(r"/srv/a\b.txt").as_deref(), Ok(r"/srv/a\b.txt"));
            assert_eq!(
                windows(r"sftp://test_user_1@testnas.local:22/a\b").as_deref(),
                Ok(r"sftp://test_user_1@testnas.local:22/a\b")
            );
            // A literal escape-range code point in a drive row still reads literal.
            let literal = windows("C:/src\\caf\u{10FEE9}.txt").expect("reads");
            assert!(literal.starts_with("C:/src/caf"), "{literal:?}");
        }
    }

    /// On macOS and Linux `\` and `:` are name characters, and the row reads byte for byte.
    #[test]
    fn a_posix_row_keeps_its_backslashes_and_colons() {
        for text in [
            r"/Users/test_user_1/a\b.txt",
            "/Users/test_user_1/C:/x",
            "/Users/test_user_1/cafe\u{301}.txt",
            "/Users/test_user_1/caf\u{e9}.txt",
            "/Users/test_user_1/trailing. ",
            "/Users/test_user_1/trailing.",
            "/Users/test_user_1/\u{fffd}.bin",
            r"/adb:/12345678/sdcard\x/y",
            "adb://12345678/sdcard/a.jpg",
            "sftp://test_user_1@testnas.local:22/home/a b.txt",
            "smb://testnas.local/share/a:b.txt",
        ] {
            assert_eq!(
                read_as(StoredPathReading::Literal, text, PathFlavor::Posix).as_deref(),
                Ok(text)
            );
        }
        assert!(matches!(
            read_as(StoredPathReading::Literal, "/a/../b", PathFlavor::Posix),
            Err(StoredPathRefusal::NotAnAppPath(NotAnAppPath::DotName))
        ));
    }
}
