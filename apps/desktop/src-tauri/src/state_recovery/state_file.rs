//! Reading and writing one of Cmdr's own JSON state files so a file that's there but didn't load
//! is never saved over: [`read_state_file`] sets it aside (or copies it, when only some entries
//! didn't read) before anything starts empty, and when that can't happen, [`write_state_file`]
//! refuses every write to that path until a later read succeeds.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cmdr_fs::log_private::LogPrivate;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use super::{LOG_TARGET, StateFile};
use crate::ignore_poison::IgnorePoison;

/// What [`read_state_file`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum StateRead<T> {
    /// No file: a first launch, or a list never saved. Writes go through.
    Missing,
    /// The file read. When some entries didn't ([`lenient_entries`]), the rest are here and a copy
    /// of the whole file sits beside it.
    Read(T),
    /// The file couldn't be read or parsed, and is set aside now (the person hears about it). Start
    /// fresh; writes go through.
    SetAside,
    /// The file couldn't be read or parsed, and couldn't be moved either. Start fresh in memory:
    /// [`write_state_file`] refuses to touch the file for the rest of the session.
    Held,
}

impl<T> StateRead<T> {
    /// The value that read, if one did. `None` is the caller's cue to start from its default.
    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Read(value) => Some(value),
            Self::Missing | Self::SetAside | Self::Held => None,
        }
    }
}

/// Why [`write_state_file`] didn't write.
#[derive(Debug)]
pub enum StateWriteError {
    /// The file is there but didn't load this session, so a write would replace it with what's
    /// only in memory.
    NotLoaded,
    Io(std::io::Error),
}

impl fmt::Display for StateWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotLoaded => write!(f, "the file didn't load this session, so it isn't written over"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<std::io::Error> for StateWriteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// What this session already did about a path that didn't fully read, so a store that reads its
/// file on every call doesn't copy it or tell the person again each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Guard {
    /// Neither set aside nor copied: no write may land on it.
    Held,
    /// Read in part, and the whole file copied aside.
    Copied,
}

static GUARDS: Mutex<Option<HashMap<PathBuf, Guard>>> = Mutex::new(None);

fn guard(path: &Path) -> Option<Guard> {
    GUARDS.lock_ignore_poison().as_ref()?.get(path).copied()
}

fn set_guard(path: &Path, guard: Option<Guard>) {
    let mut guards = GUARDS.lock_ignore_poison();
    match guard {
        Some(guard) => {
            guards.get_or_insert_default().insert(path.to_path_buf(), guard);
        }
        None => {
            if let Some(map) = guards.as_mut() {
                map.remove(path);
            }
        }
    }
}

thread_local! {
    /// Entries [`lenient_entries`] dropped during the current [`read_state_file`] parse.
    static DROPPED: Cell<usize> = const { Cell::new(0) };
}

/// Reads the state file at `path` with `parse`, which answers `Err` with the reason a file
/// doesn't parse as this state (bad JSON, an unknown schema version).
///
/// A file that can't be read or parsed is set aside under `file`'s name before the caller starts
/// empty; one where [`lenient_entries`] dropped some entries is copied aside, and the entries that
/// read come back. Either way the person hears about it once. When neither the move nor the copy
/// happens, writes to `path` stay refused for the session.
pub fn read_state_file<T>(
    path: &Path,
    file: StateFile,
    parse: impl FnOnce(&[u8]) -> Result<T, String>,
) -> StateRead<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            set_guard(path, None);
            return StateRead::Missing;
        }
        Err(e) => return unloaded(path, file, &e.to_string()),
    };
    DROPPED.set(0);
    let parsed = parse(&bytes);
    let dropped = DROPPED.replace(0);
    match parsed {
        Err(why) => unloaded(path, file, &why),
        Ok(value) if dropped == 0 => {
            set_guard(path, None);
            StateRead::Read(value)
        }
        Ok(value) => {
            if guard(path).is_none() {
                let dropped = crate::pluralize::pluralize_with(dropped as u64, "entry", "entries");
                log::warn!(
                    target: LOG_TARGET,
                    // allowed-log-private: `file` is the `StateFile` variant, not a path
                    "{dropped} of the {file:?} file at {path:?} didn't read",
                    path = LogPrivate::file_path(path)
                );
                let copied = super::keep_copy(path, file).is_some();
                set_guard(path, Some(if copied { Guard::Copied } else { Guard::Held }));
            }
            StateRead::Read(value)
        }
    }
}

/// A file that's there but didn't load: set it aside, or hold it when that can't happen.
fn unloaded<T>(path: &Path, file: StateFile, why: &str) -> StateRead<T> {
    if guard(path) == Some(Guard::Held) {
        return StateRead::Held;
    }
    log::warn!(
        target: LOG_TARGET,
        // allowed-log-private: `file` is the `StateFile` variant, not a path
        "Couldn't load the {file:?} file at {path:?}: {why}",
        path = LogPrivate::file_path(path)
    );
    if super::set_aside(path, file).is_some() {
        set_guard(path, None);
        StateRead::SetAside
    } else {
        set_guard(path, Some(Guard::Held));
        StateRead::Held
    }
}

/// Writes `json` to `path` durably (`crate::config::durable_write_json`, through
/// `<name>.json.tmp`), creating the folder when it's missing. Refuses with
/// [`StateWriteError::NotLoaded`] while [`read_state_file`] holds `path`.
pub fn write_state_file(path: &Path, json: &str) -> Result<(), StateWriteError> {
    if guard(path) == Some(Guard::Held) {
        return Err(StateWriteError::NotLoaded);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::config::durable_write_json(path, &path.with_extension("json.tmp"), json)?;
    Ok(())
}

/// `#[serde(default, deserialize_with = "crate::state_recovery::lenient_entries")]` on a list of
/// independent entries: an entry that doesn't parse (a provider kind from a newer build, a typo)
/// is dropped and the rest read, where a plain `Vec` would fail the whole file. Inside
/// [`read_state_file`], a drop also keeps a copy of the file. JSON only.
pub fn lenient_entries<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let mut entries = Vec::with_capacity(raw.len());
    for value in raw {
        match serde_json::from_value(value) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                log::warn!(target: LOG_TARGET, "Dropped a saved entry that didn't read: {e}");
                DROPPED.set(DROPPED.get() + 1);
            }
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_recovery::testing;
    use crate::test_support::TestDir;
    use serde::Serialize;
    #[cfg(unix)] // POSIX modes, for the unreadable-file cell.
    use std::os::unix::fs::PermissionsExt;

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct List {
        #[serde(default, deserialize_with = "lenient_entries")]
        entries: Vec<Entry>,
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Entry {
        Plain { name: String },
    }

    fn parse(bytes: &[u8]) -> Result<List, String> {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }

    #[cfg(unix)] // `geteuid`, for the unreadable-file cell.
    fn running_as_root() -> bool {
        // SAFETY: (test) `geteuid` takes no arguments, shares no memory, and can't fail.
        unsafe { libc::geteuid() == 0 }
    }

    #[test]
    fn a_missing_file_reads_as_missing_and_writes_go_through() {
        let dir = TestDir::new("state_file_missing");
        let path = dir.join("list.json");
        assert_eq!(
            read_state_file(&path, StateFile::SftpServers, parse),
            StateRead::Missing
        );
        write_state_file(&path, "{}").expect("the write goes through");
    }

    #[cfg(unix)] // A POSIX 0000 mode makes the file unreadable; Windows needs an ACL for that.
    #[test]
    fn an_unreadable_file_is_set_aside_before_anything_writes() {
        if running_as_root() {
            return;
        }
        let notices = testing::capture_recoveries();
        let dir = TestDir::new("state_file_unreadable");
        let path = dir.join("list.json");
        fs::write(&path, br#"{"entries": [{"kind": "plain", "name": "a"}]}"#).expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("make it unreadable");

        assert_eq!(
            read_state_file(&path, StateFile::SftpServers, parse),
            StateRead::SetAside
        );
        write_state_file(&path, r#"{"entries": []}"#).expect("the fresh list saves");

        let kept = testing::kept_copies(&path);
        assert_eq!(kept.len(), 1);
        fs::set_permissions(&kept[0], fs::Permissions::from_mode(0o644)).expect("read the copy");
        assert_eq!(
            fs::read(&kept[0]).expect("read the copy"),
            br#"{"entries": [{"kind": "plain", "name": "a"}]}"#
        );
        assert_eq!(notices.recoveries().len(), 1);
    }

    #[test]
    fn a_file_with_one_unreadable_entry_keeps_the_rest_and_a_copy_of_all() {
        let notices = testing::capture_recoveries();
        let dir = TestDir::new("state_file_partial");
        let path = dir.join("list.json");
        let original = br#"{"entries": [{"kind": "plain", "name": "a"}, {"kind": "from_a_newer_build"}]}"#;
        fs::write(&path, original).expect("write");

        let read = read_state_file(&path, StateFile::SftpServers, parse);
        let again = read_state_file(&path, StateFile::SftpServers, parse);

        let expected = List {
            entries: vec![Entry::Plain { name: "a".into() }],
        };
        assert_eq!(read, StateRead::Read(expected));
        assert!(matches!(again, StateRead::Read(_)));
        assert_eq!(fs::read(&path).expect("the file stays"), original);
        let kept = testing::kept_copies(&path);
        assert_eq!(kept.len(), 1, "one copy however often it's read: {kept:?}");
        assert_eq!(fs::read(&kept[0]).expect("read the copy"), original);
        assert_eq!(notices.recoveries().len(), 1, "one notice");
        write_state_file(&path, r#"{"entries": []}"#).expect("an edit saves over it, the copy holds the rest");
    }

    /// The next launch reads the same partly unreadable file again (nothing saved over it yet).
    /// The copy it would make is already there, so there's no second copy, which would push out an
    /// older, different one, and no second notice for the same thing.
    #[test]
    fn a_partly_unreadable_file_already_copied_is_not_copied_or_announced_again() {
        let notices = testing::capture_recoveries();
        let dir = TestDir::new("state_file_partial_relaunch");
        let path = dir.join("list.json");
        let original = br#"{"entries": [{"kind": "plain", "name": "a"}, {"kind": "from_a_newer_build"}]}"#;
        fs::write(&path, original).expect("write");

        assert!(matches!(
            read_state_file(&path, StateFile::SftpServers, parse),
            StateRead::Read(_)
        ));
        set_guard(&path, None); // A new launch: this session's memory of the path is gone.
        let again = read_state_file(&path, StateFile::SftpServers, parse);

        assert!(matches!(again, StateRead::Read(_)));
        assert_eq!(testing::kept_copies(&path).len(), 1, "still one copy");
        assert_eq!(notices.recoveries().len(), 1, "still one notice");
        write_state_file(&path, r#"{"entries": []}"#).expect("an edit still saves over it");
    }

    /// When the file can't be moved aside (here an immutable file, which refuses the rename), it's
    /// held: every write refuses until a read succeeds, and the person hears about it once.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_that_cant_be_set_aside_is_never_written_over() {
        let notices = testing::capture_recoveries();
        let dir = TestDir::new("state_file_held");
        let path = dir.join("list.json");
        fs::write(&path, b"{\"trunc").expect("write");
        testing::set_immutable(&path, true);

        let first = read_state_file(&path, StateFile::SftpServers, parse);
        let second = read_state_file(&path, StateFile::SftpServers, parse);
        testing::set_immutable(&path, false);
        let refused = write_state_file(&path, "{}");

        assert_eq!(first, StateRead::Held);
        assert_eq!(second, StateRead::Held);
        assert!(matches!(refused, Err(StateWriteError::NotLoaded)), "{refused:?}");
        assert_eq!(fs::read(&path).expect("the file stays"), b"{\"trunc");
        assert_eq!(notices.recoveries().len(), 1);

        fs::write(&path, br#"{"entries": []}"#).expect("the file is healthy again");
        assert!(matches!(
            read_state_file(&path, StateFile::SftpServers, parse),
            StateRead::Read(_)
        ));
        write_state_file(&path, "{}").expect("a good read lifts the hold");
    }
}
