//! The app's side of restic browsing. Everything that reads a repository is
//! `crates/cmdr-restic`; what's here is the two seams that reach it and the
//! one decision only the app can make (which hub every view shares).
//!
//! ## Two seams, no hooks
//!
//! Everything at and under `<repo>/Browse snapshots` is a ROUTE:
//! `VolumeManager::resolve` sends it to the read-only `cmdr_restic::ResticVolume`
//! once the hub confirms `<repo>` is a repo (`volume/manager/restic_routing.rs`).
//! The `Browse snapshots` row in `<repo>`'s own listing is a listing OVERLAY
//! (`overlay.rs`), which reaches a pane and nothing else. No volume backend
//! names restic, so the repo's real files stay ordinary files.

pub mod overlay;
pub mod snapshot_watch;
pub mod wiring;

#[cfg(test)]
mod overlay_tests;
#[cfg(test)]
mod snapshot_watch_tests;
// POSIX fixture: `LocalPosixVolume`, source modes, symlinks, and a `mkfifo` (`libc`).
#[cfg(all(test, unix))]
pub(crate) mod test_repo;
#[cfg(test)]
mod walker_exposure_tests;
#[cfg(test)]
mod wiring_tests;
