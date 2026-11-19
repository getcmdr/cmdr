//! Where a listed entry is, both ways: the path the OS opens to stat it, and the name and app
//! path its row carries.
//!
//! The row is built by the app path's own join (`AppPath::join`, one allocation), so a row
//! never takes `std::path`'s host rules: on Windows a child of `C:/x` is `C:/x/a.txt`, ❌ never
//! `C:/x\a.txt`. The OS path joins the raw name, so the stat reaches the file the OS listed,
//! whatever its name.
//!
//! ❗ A name that isn't plain Unicode text (a Linux name that isn't UTF-8, a Windows name holding
//! an unpaired surrogate) keeps today's lossy row until escapes go live (milestone 10f of
//! `docs/specs/app-path-plan.md`): its stat still goes through the raw name, and the listing
//! counts it in a debug line. A folder the reader can't spell as a canonical, escape-free app
//! path (a transition-bridge value: `/x/link/..`, a trailing `/`) keeps today's rows too: the
//! OS path's own text.

use std::borrow::Cow;
use std::cell::Cell;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppName, AppNameBuf, AppPath, PathArithmetic, is_escape};
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::VolumeError;

/// What an entry's row says it is: its name and its app path, as the wire carries them.
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) path: String,
}

impl Row {
    /// The row of the entry at `os`, spelled `app` where the caller has one that rows can
    /// carry ([`rows_can_carry`]), else today's text of the OS path. ❗ The filter is here, ❌
    /// not at each caller: a path holding an escape (one made from a native `caf\xe9`) would
    /// otherwise put the escape on the wire before 10f, a second row beside the listing's.
    pub(crate) fn of(os: &Path, app: Option<&AppPath>) -> Row {
        match app.filter(|app| rows_can_carry(app)) {
            Some(app) => Row {
                name: app.file_name().unwrap_or(app.as_str()).to_owned(),
                path: app.as_str().to_owned(),
            },
            None => Row {
                name: os
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| os.to_string_lossy().into_owned()),
                path: os.to_string_lossy().into_owned(),
            },
        }
    }
}

/// How a failure names a local path, as its caller reads it: on macOS and Linux the OS path's
/// text (the app path's own bytes, lossy only where a name isn't UTF-8, as every local error
/// was before app paths), on Windows the app path (`C:/x`), ❌ never the `\\?\` form only the
/// OS reads.
pub(crate) struct Named(String);

impl Named {
    /// A label for errors about the path spelled `text`.
    pub(crate) fn new(text: String) -> Self {
        Self(text)
    }

    /// `err` as a `VolumeError` naming this path. The text only labels the error
    /// (`from_io_at` reads it as a path's display); it never reaches a file API.
    pub(crate) fn error(&self, err: &io::Error) -> VolumeError {
        VolumeError::from_io_at(err, self.0.as_str())
    }
}

/// Whether rows may spell entries under `app` by the app path's text: it's canonical (a
/// bridge value's text isn't the place it names) and holds no escape (escapes go live in 10f;
/// until then a row is today's lossy text).
pub(crate) fn rows_can_carry(app: &AppPath) -> bool {
    app.is_canonical() && !app.as_str().chars().any(is_escape)
}

/// A folder the reader lists: the path the OS opens, and the app path its rows join under.
pub(crate) struct ListingDir<'a> {
    os: Cow<'a, Path>,
    /// `None` where rows keep the OS path's text (see the module doc).
    app: Option<Cow<'a, AppPath>>,
    /// Names this listing spelled lossily, for the debug line.
    lossy: Cell<usize>,
}

impl<'a> ListingDir<'a> {
    /// `os`, whose rows join under `app` when it can carry them ([`rows_can_carry`]).
    pub(crate) fn new(os: Cow<'a, Path>, app: Option<Cow<'a, AppPath>>) -> Self {
        Self {
            os,
            app: app.filter(|app| rows_can_carry(app)),
            lossy: Cell::new(0),
        }
    }

    /// The path the OS lists.
    pub(crate) fn os(&self) -> &Path {
        &self.os
    }

    /// The entry the OS listed as `raw`: the path to stat it through (the raw name, never a
    /// lossy one) and its row.
    pub(crate) fn child(&self, raw: &OsStr) -> (PathBuf, Row) {
        let os = self.os.join(raw);
        if let Some(dir) = &self.app
            && let Ok(name) = AppNameBuf::from_native_os_unicode(raw)
            && let Ok(path) = dir.join(&name)
        {
            return (
                os,
                Row {
                    name: name.into_string(),
                    path: path.into_string(),
                },
            );
        }
        let name = raw.to_string_lossy().into_owned();
        if self.app.is_some() {
            self.lossy.set(self.lossy.get() + 1);
        }
        let path = lossy_child_text(self.app.as_deref(), &os, &name);
        (os, Row { name, path })
    }

    /// The entry named `name` (a mutation's created or renamed name, a conflict scan's source
    /// name): by the typed join when the joined path is one rows carry ([`rows_can_carry`]),
    /// else today's spelling, the folder's OS path joined with the name's TEXT. ❗ A name
    /// holding an escape takes today's spelling too, so its escape isn't decoded into a file
    /// the row doesn't name (until 10f; a decoded re-stat made a second, escaped row beside
    /// the listing's). `None` only on Windows, whose `\\?\` path has no text join
    /// (`platform::text_child`).
    pub(crate) fn named_child(&self, name: &str) -> Option<(PathBuf, Row)> {
        if let Some(dir) = &self.app
            && let Ok(name) = AppName::from_app_spelling(name)
            && let Ok(path) = dir.join(name)
            && rows_can_carry(&path)
            && let Ok(os) = path.to_native()
        {
            let row = Row::of(&os, Some(&path));
            return Some((os, row));
        }
        let os = super::platform::text_child(&self.os, name)?;
        let row = Row::of(&os, None);
        Some((os, row))
    }

    /// Logs how many names this listing spelled lossily, if any.
    pub(crate) fn report_lossy(&self) {
        let lossy = self.lossy.get();
        if lossy > 0 {
            log::debug!(
                "listing {}: {lossy} name(s) aren't Unicode text, listed lossily until escapes go live",
                LogPrivate::dir_path(self.os.as_os_str())
            );
        }
    }
}

/// Today's row text for a child whose name isn't Unicode text: the folder's app path plus the
/// lossy name (on macOS and Linux the same bytes as the OS path's own text; on Windows the OS
/// path is the `\\?\` form, which no row carries), or the OS path's text under a folder rows
/// don't spell typed.
fn lossy_child_text(dir: Option<&AppPath>, os: &Path, lossy_name: &str) -> String {
    let Some(dir) = dir else {
        return os.to_string_lossy().into_owned();
    };
    let text = dir.as_str();
    if text.ends_with('/') {
        format!("{text}{lossy_name}")
    } else {
        format!("{text}/{lossy_name}")
    }
}
