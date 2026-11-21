//! The two paths a staged write addresses on its volume, each spelled both
//! ways once: [`Temp`] (the `.cmdr-tmp-*` it writes) and [`Spelled`] (the final
//! name). A leaf: `staged_write` and `recovered_name` both name them, and
//! `recovered_name` naming `staged_write` would weld it to `volume::*` in a
//! module cycle (`scripts/check/checks/DETAILS.md` § "Rust module cycles").

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::staging::StagingTemp;
use std::path::{Path, PathBuf};

/// A staged write's `.cmdr-tmp-*`: the guard that owns it, and its spelling
/// for the destination volume, made once with it. Every volume call on the
/// temp addresses [`app`](Self::app); the ledgers and the logs read
/// [`path`](Self::path), so their text stays byte-exact.
pub(super) struct Temp {
    guard: StagingTemp,
    app: AppPathBuf,
}

impl Temp {
    pub(super) fn new(guard: StagingTemp) -> Self {
        let app = AppPath::legacy_unchecked(guard.path()).into_owned();
        Self { guard, app }
    }

    /// The temp as the ledgers and the logs spell it.
    pub(super) fn path(&self) -> &Path {
        self.guard.path()
    }

    /// The temp as its volume addresses it.
    pub(super) fn app(&self) -> &AppPath {
        &self.app
    }
}

/// A path a staged write addresses on its volume, in both spellings made once:
/// the `std::path` one for the ledgers, the logs, and the errors (its text
/// exits, byte-exact on a name that isn't UTF-8), the app one for every volume
/// call.
#[derive(Debug, Clone)]
pub(super) struct Spelled {
    path: PathBuf,
    app: AppPathBuf,
}

impl Spelled {
    pub(super) fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            app: AppPath::legacy_unchecked(path).into_owned(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn app(&self) -> &AppPath {
        &self.app
    }
}
