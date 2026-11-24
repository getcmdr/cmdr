//! "Cmdr is about to write here": every path the engine creates, replaces,
//! renames, or removes is announced first, so the host can tell the engine's own
//! writes from somebody else's. Cmdr's answer feeds the downloads watcher's
//! ignore set, through `operations_host::own_writes`, so a copy into
//! `~/Downloads` never pops a "new download" toast.

use cmdr_fs::app_path::AppPath;
use std::path::Path;

/// Where the engine announces its own writes.
///
/// ❗ **Announce BEFORE the syscall** (a rename: both ends), once per path per
/// file landing, ❌ never per chunk: the watcher's event can arrive before the
/// syscall returns, and an announce after it loses that race.
pub trait OwnWrites: Send + Sync {
    /// Cmdr is about to create, replace, rename, or remove `path`. Returns at
    /// once: it sits beside a syscall on every file landing.
    fn announce(&self, path: &Path);
}

/// The answer for a host with nobody listening: a bench, a tool, an engine
/// before the app wired its own.
pub(super) struct NoOwnWrites;

impl OwnWrites for NoOwnWrites {
    fn announce(&self, _path: &Path) {}
}

/// A path the host heard about before the engine writes there.
///
/// Only [`EngineHost::announce`](super::EngineHost::announce) and
/// [`EngineHost::announce_on`](super::EngineHost::announce_on) make one, and the
/// writers that create or rename onto a path take that destination as an
/// `Announced`: `overwrite::stage_and_land_file` (and the copy strategies above
/// it), the local move's `rename_onto_free_name` and `move_resolved_into_place`,
/// and the volume engine's `StagedWrite::begin` (and fresh compress's
/// `begin_generated`). Calling one without announcing
/// first doesn't compile.
#[derive(Debug, Clone, Copy)]
pub struct Announced<'a>(&'a Path);

impl<'a> Announced<'a> {
    pub(super) fn new(path: &'a Path) -> Self {
        Self(path)
    }

    /// `path`, for a test that drives a writer directly and so stands in for
    /// the caller that would have announced it.
    #[cfg(test)]
    pub(crate) fn assumed(path: &'a Path) -> Self {
        Self(path)
    }

    /// The announced path.
    pub(crate) fn path(self) -> &'a Path {
        self.0
    }
}

/// [`Announced`] for a path a volume addresses, held as the app path the volume
/// engine's writers take (`StagedWrite::begin`). Only
/// [`EngineHost::announce_spelled_on`](super::EngineHost::announce_spelled_on)
/// makes one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AnnouncedOnVolume<'a>(&'a AppPath);

impl<'a> AnnouncedOnVolume<'a> {
    pub(super) fn new(path: &'a AppPath) -> Self {
        Self(path)
    }

    /// `path`, for a test that drives a writer directly and so stands in for
    /// the caller that would have announced it.
    #[cfg(test)]
    pub(crate) fn assumed(path: &'a AppPath) -> Self {
        Self(path)
    }

    /// The announced path.
    pub(crate) fn path(self) -> &'a AppPath {
        self.0
    }
}
