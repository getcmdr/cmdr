//! Dir interning: every directory prefix stored once, forever, as a `dirs` row
//! (`DETAILS.md` § Schema), and the walk back to a full path.

use std::borrow::Cow;
use std::path::PathBuf;

use cmdr_fs::app_path::PathFlavor;
use rusqlite::{Connection, OptionalExtension};

use super::{OperationLogStoreError, fold_name};

/// Intern a directory path for `volume_id`, returning its `dir_id`. Interns each
/// ancestor once (reused across operations, so a hot directory is stored once
/// forever), then returns the leaf's id. A volume root (empty path) interns a
/// single row with name `""` and NULL parent.
///
/// A path's root other than `/` is ONE name in the chain, see [`split_root`]: a
/// remote volume's `scheme://authority` (`adb://serial/…`,
/// `sftp://test_user_1@nas:22/…`), and on Windows a drive (`C:/`) or a UNC share
/// (`//server/share/`).
///
/// Runs on a write-capable connection — inside the writer thread's transaction
/// in production, or directly in tests. Idempotent: the same `(volume_id, path)`
/// always returns the same id.
pub(crate) fn intern_dir(conn: &Connection, volume_id: &str, path: &str) -> Result<i64, OperationLogStoreError> {
    intern_dir_as(conn, volume_id, path, PathFlavor::HOST)
}

/// [`intern_dir`], reading `path`'s root by `flavor`'s rules, so a test runs either
/// host's rules on every OS.
pub(crate) fn intern_dir_as(
    conn: &Connection,
    volume_id: &str,
    path: &str,
    flavor: PathFlavor,
) -> Result<i64, OperationLogStoreError> {
    // The volume root anchors the chain (name ""), so a file directly at the
    // volume root still has a dir to reference.
    let mut parent = intern_one(conn, volume_id, None, "")?;
    let (root, rest) = split_root(path, flavor);
    for component in root.as_deref().into_iter().chain(names_of(rest)) {
        parent = intern_one(conn, volume_id, Some(parent), component)?;
    }
    Ok(parent)
}

/// Intern the folder named `name` inside the interned dir `parent_dir_id`, on the
/// parent's volume, returning its id: a DIR item row's own folder
/// (`operation_items.self_dir_id`). Interning the child of an already interned parent
/// keeps [`split_root`]'s semantics for free, since a root other than `/` is already
/// one name up the chain. An empty name (a row naming the volume root itself) is the
/// parent.
pub(crate) fn intern_child_dir(
    conn: &Connection,
    parent_dir_id: i64,
    name: &str,
) -> Result<i64, OperationLogStoreError> {
    if name.is_empty() {
        return Ok(parent_dir_id);
    }
    let volume_id: String = conn
        .prepare_cached("SELECT volume_id FROM dirs WHERE dir_id = ?1")?
        .query_row(rusqlite::params![parent_dir_id], |row| row.get(0))?;
    intern_one(conn, &volume_id, Some(parent_dir_id), name)
}

/// The separator that marks a path as rooted at a URL-style volume root.
const SCHEME_SEPARATOR: &str = "://";

/// The names below a path's root: split on `/`, empty ones dropped (so a trailing or
/// doubled `/` never reaches a row). A `.` and a `..` are names, stored as they are.
fn names_of(rest: &str) -> impl Iterator<Item = &str> {
    rest.split('/').filter(|c| !c.is_empty())
}

/// Splits a path's root off its head, when it has one other than `/`, returning it
/// as the ONE name it's stored as, and what follows. A path with no such head comes
/// back whole.
///
/// - A `scheme://authority` head ([`split_scheme_root`]), on every host, kept as it's
///   spelled (`sftp://h:22`, no trailing `/`).
/// - On Windows ([`PathFlavor::Windows`]), a drive (`C:`, stored as `C:/`, its letter
///   uppercased as the app path's canonical form spells it) and a UNC share
///   (`//server/share`, stored as `//server/share/`). A one-letter scheme is a drive
///   there (`C://x` is `C:/x`), as the app path reads it.
///
/// Each of these root names holds a `/`, which no real folder name can, so
/// [`reconstruct_dir_path`] tells it from a name: a folder named `C:` under `/` stays
/// `/C:`. On macOS and Linux only the scheme head applies, so every POSIX row keeps its
/// bytes (a leading `//` there is the `/` root, as the OS reads it).
fn split_root(path: &str, flavor: PathFlavor) -> (Option<Cow<'_, str>>, &str) {
    if flavor == PathFlavor::Windows
        && let Some((root, rest)) = split_windows_root(path)
    {
        return (Some(Cow::Owned(root)), rest);
    }
    let (root, rest) = split_scheme_root(path);
    (root.map(Cow::Borrowed), rest)
}

/// A Windows drive or UNC root off the head of `path`, spelled as [`split_root`] stores
/// it, and what follows; `None` for any other shape (a UNC head missing its server or
/// share included, which keeps the plain splitting).
fn split_windows_root(path: &str) -> Option<(String, &str)> {
    let bytes = path.as_bytes();
    if let [letter, b':', after @ ..] = bytes
        && letter.is_ascii_alphabetic()
        && after.first().is_none_or(|b| *b == b'/')
    {
        return Some((format!("{}:/", char::from(letter.to_ascii_uppercase())), &path[2..]));
    }
    let after = path.strip_prefix("//")?;
    let (server, tail) = after.split_once('/')?;
    let share_len = tail.find('/').unwrap_or(tail.len());
    if server.is_empty() || share_len == 0 {
        return None;
    }
    let root_len = "//".len() + server.len() + "/".len() + share_len;
    Some((format!("{}/", &path[..root_len]), &path[root_len..]))
}

/// Splits `scheme://authority` off the head of a remote volume's path, returning
/// it and what follows. A path with no such head comes back whole.
///
/// The root has to stay one name: splitting it on `/` like the rest drops the
/// empty segment inside `://`, and [`reconstruct_dir_path`] then rebuilds
/// `adb://serial/x` as `/adb:/serial/x`, a path no volume answers to, which the
/// rollback would hand to one. No real folder name can contain `://` (a name
/// holds no `/`), so a name that does is always this root.
fn split_scheme_root(path: &str) -> (Option<&str>, &str) {
    let Some(scheme_len) = path.find(SCHEME_SEPARATOR) else {
        return (None, path);
    };
    let is_scheme = scheme_len > 0
        && path[..scheme_len]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    if !is_scheme {
        return (None, path);
    }
    let authority_start = scheme_len + SCHEME_SEPARATOR.len();
    let root_len = path[authority_start..]
        .find('/')
        .map_or(path.len(), |slash| authority_start + slash);
    (Some(&path[..root_len]), &path[root_len..])
}

/// Intern one `(volume_id, parent, name)` dir row, returning its id. Insert-or-
/// ignore against the IFNULL identity index, then read the id back (reliable
/// whether or not the insert did anything). The identity is the EXACT name;
/// `name_folded` rides along as a plain column.
fn intern_one(
    conn: &Connection,
    volume_id: &str,
    parent: Option<i64>,
    name: &str,
) -> Result<i64, OperationLogStoreError> {
    conn.prepare_cached(
        "INSERT INTO dirs (volume_id, parent_dir_id, name, name_folded)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (volume_id, IFNULL(parent_dir_id, 0), name) DO NOTHING",
    )?
    .execute(rusqlite::params![volume_id, parent, name, fold_name(name)])?;

    // `parent.unwrap_or(0)` mirrors the index's IFNULL(parent, 0): dir ids start
    // at 1, so 0 is a safe stand-in for a NULL (root) parent.
    let id = conn
        .prepare_cached(
            "SELECT dir_id FROM dirs
             WHERE volume_id = ?1 AND IFNULL(parent_dir_id, 0) = ?2 AND name = ?3",
        )?
        .query_row(rusqlite::params![volume_id, parent.unwrap_or(0), name], |row| {
            row.get::<_, i64>(0)
        })?;
    Ok(id)
}

/// The interned id of the directory `path` on `volume_id`, or `None` when no
/// operation ever recorded it. The read-only twin of [`intern_dir`]: the same
/// walk, by the same EXACT names, inserting nothing. Each step is one lookup on
/// the `dirs_identity` index.
pub(crate) fn find_dir(conn: &Connection, volume_id: &str, path: &str) -> Result<Option<i64>, OperationLogStoreError> {
    find_dir_as(conn, volume_id, path, PathFlavor::HOST)
}

/// [`find_dir`], reading `path`'s root by `flavor`'s rules, as [`intern_dir_as`] does.
pub(crate) fn find_dir_as(
    conn: &Connection,
    volume_id: &str,
    path: &str,
    flavor: PathFlavor,
) -> Result<Option<i64>, OperationLogStoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT dir_id FROM dirs
         WHERE volume_id = ?1 AND IFNULL(parent_dir_id, 0) = ?2 AND name = ?3",
    )?;
    let mut step = |parent: i64, name: &str| {
        stmt.query_row(rusqlite::params![volume_id, parent, name], |row| row.get::<_, i64>(0))
            .optional()
    };
    // The volume root's row is named "" under no parent (0 stands in for NULL).
    let Some(mut dir) = step(0, "")? else {
        return Ok(None);
    };
    let (root, rest) = split_root(path, flavor);
    for name in root.as_deref().into_iter().chain(names_of(rest)) {
        let Some(child) = step(dir, name)? else {
            return Ok(None);
        };
        dir = child;
    }
    Ok(Some(dir))
}

/// Reconstruct a dir's full path by walking `parent_dir_id` to the volume root.
/// The root (name `""`) contributes nothing, so a path renders as `/a/b/c`; one led
/// by a remote `scheme://authority` name as `scheme://authority/a/b`; and one led by a
/// Windows root name ([`split_root`]: `C:/`, `//server/share/`, the only names that
/// end in `/`) as `C:/a/b` or `//server/share/a/b`.
pub fn reconstruct_dir_path(conn: &Connection, dir_id: i64) -> Result<String, OperationLogStoreError> {
    let mut names = Vec::new();
    let mut current = Some(dir_id);
    let mut stmt = conn.prepare_cached("SELECT name, parent_dir_id FROM dirs WHERE dir_id = ?1")?;
    while let Some(id) = current {
        let (name, parent): (String, Option<i64>) =
            stmt.query_row(rusqlite::params![id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if !name.is_empty() {
            names.push(name);
        }
        current = parent;
    }
    names.reverse();
    Ok(render(&names))
}

/// Join a resolved dir path with a leaf name into a full path. The root renders as
/// `/`, so `join_leaf("/", "a")` is `/a` and `join_leaf("/a/b", "c")` is `/a/b/c`.
pub(crate) fn join_leaf(dir_path: &str, name: &str) -> PathBuf {
    PathBuf::from(join_leaf_text(dir_path, name))
}

/// [`join_leaf`]'s text: `Path::join`'s rule over `/` alone, on every host, so a drive
/// path `C:/x` joins to `C:/x/a`, where `Path::join` on Windows wrote `C:/x\a`. A
/// separator goes between unless the dir is empty or ends in one (a root: `/`, `C:/`,
/// `//server/share/`), and a name led by `/` replaces the dir, as `Path::join` does on
/// macOS and Linux, where this answers its exact bytes.
pub(crate) fn join_leaf_text(dir_path: &str, name: &str) -> String {
    if name.starts_with('/') {
        return name.to_owned();
    }
    let separator = if dir_path.is_empty() || dir_path.ends_with('/') {
        ""
    } else {
        "/"
    };
    format!("{dir_path}{separator}{name}")
}

/// The path a chain of names below the volume-root row spells, root first.
fn render(names: &[String]) -> String {
    // The scheme test goes first: a scheme root with an empty authority (`x://`) ends in
    // `/` too, and keeps its old rendering.
    match names.split_first() {
        Some((root, _)) if root.contains(SCHEME_SEPARATOR) => names.join("/"),
        Some((root, rest)) if root.ends_with('/') => format!("{root}{}", rest.join("/")),
        _ => format!("/{}", names.join("/")),
    }
}
