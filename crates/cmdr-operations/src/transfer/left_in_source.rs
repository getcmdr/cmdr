//! What a move's source sweep left in place, tallied the same way by both
//! engines: the local cross-filesystem sweep (`move_op/source_sweep.rs`) and the
//! cross-volume one (`volume/source_sweep.rs`).
//!
//! Two kinds of leftover, both the user's only copy of something: items that
//! turned up in a source folder after the move looked at it, and originals saved
//! over after their copy started. Neither is an error, so the move still
//! completes; the completion event carries the tally as typed data
//! (`AppearedDuringMove`) and the frontend words it.

use std::path::Path;

use cmdr_fs::os_error::OsErrorClass;
use cmdr_fs::volume::VolumeError;

use crate::types::AppearedDuringMove;

/// The running tally across one move's top-level sources.
///
// DEFAULT-OK: the zero value is the claim "the sweep has left nothing behind",
// which is exactly true of a sweep that hasn't run yet and stays true of one
// that took every source it was given.
#[derive(Default)]
pub(crate) struct LeftInSource {
    /// Items the move never saw, counted once per unknown subtree.
    appeared: u32,
    /// Originals saved over after their copy started, each counted once.
    changed: u32,
    /// The names of the folders holding either kind, one per top-level source
    /// that kept something, in sweep order.
    folders: Vec<String>,
}

impl LeftInSource {
    /// Records what the sweep left under one top-level source. A source that
    /// kept nothing adds nothing, so the folder list only names ones that did.
    pub(crate) fn note(&mut self, source: &Path, source_is_dir: bool, appeared: u32, changed: u32) {
        if appeared == 0 && changed == 0 {
            return;
        }
        self.appeared += appeared;
        self.changed += changed;
        self.folders.push(folder_holding(source, source_is_dir));
    }

    /// The completion event's typed field, or `None` when the move took
    /// everything it was asked to take (the ordinary case).
    pub(crate) fn appeared_during_move(&self) -> Option<AppearedDuringMove> {
        let folder_name = self.folders.first()?.clone();
        Some(AppearedDuringMove {
            item_count: self.appeared,
            changed_count: self.changed,
            folder_name,
            folder_count: self.folders.len() as u32,
        })
    }
}

/// The name of the folder a source's leftovers sit in, for the completion
/// sentence: the source itself when it's a folder, the folder holding it when
/// it's a file.
fn folder_holding(source: &Path, source_is_dir: bool) -> String {
    let folder = if source_is_dir {
        source
    } else {
        source.parent().unwrap_or(source)
    };
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

/// Whether a move's sweep may leave an OS junk file it skipped (#314, #1305) in
/// place, with its folder, because the OS refused the delete: Explorer holding
/// `Thumbs.db` open (a sharing violation), an open `.nfs*` on Linux (`EBUSY`), a
/// permission. It's junk the person asked to skip, so a refusal is ❌ never a
/// failed move. Anything else (a failing disk, a dropped share) still stops the
/// sweep, so the outage wait and the error path keep working.
pub(crate) fn junk_delete_refused(class: Option<OsErrorClass>) -> bool {
    matches!(
        class,
        Some(
            OsErrorClass::AccessDenied
                | OsErrorClass::NotPermitted
                | OsErrorClass::Busy
                | OsErrorClass::InUseByAnotherApp
        )
    )
}

/// [`junk_delete_refused`] for a backend's answer: its typed refusals, or the
/// OS class it carries.
pub(crate) fn junk_volume_delete_refused(error: &VolumeError) -> bool {
    matches!(
        error,
        VolumeError::FileInUse(_) | VolumeError::DeletePending(_) | VolumeError::PermissionDenied { .. }
    ) || junk_delete_refused(error.os_class())
}
