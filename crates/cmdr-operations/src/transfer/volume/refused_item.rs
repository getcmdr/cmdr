//! The volume engines' half of `../../refused_name.rs`: turning a failure into
//! the question, for one item whose destination refused its name.
//!
//! Who asks is whoever may wait on a person: the merge WALKER for the leaves it
//! submitted (a leaf is a future only the walker polls, so it must never wait
//! on the dispatch mutex itself, `merge_leaves.rs::LeafPool`), and the top-level
//! dispatch for a source whose own name was refused (a spawned task in the
//! concurrent driver, the closure in the serial one). Each asks only about a
//! refusal AT the item it owns, so a deep refusal answered Stop doesn't get
//! asked about again on its way up.

use cmdr_fs::app_path::AppPath;
use std::path::Path;

use super::super::super::refused_name::{RefusedItem, RefusedNameAnswer, ask_async, refusal_reason};
use super::super::super::types::RefusedNameReason;
use super::merge_ctx::MergeCtx;
use super::transfer_error::{PathRole, PathedVolumeError, failed_native};
use cmdr_fs::volume::VolumeError;

/// The item whose name may have been refused, as its owner knows it.
pub(super) struct RefusalSubject<'a> {
    pub source: &'a Path,
    /// Where the item was meant to land. A refusal names this path, or it is
    /// someone else's to ask about.
    pub dest: &'a Path,
    pub source_size: Option<u64>,
    pub source_is_directory: bool,
}

/// `Ok(())` ⇒ the person skipped the item: nothing of it is at the
/// destination, and the caller records it as skipped. `Err` ⇒ the failure
/// stands (it's no refused name, the person said Stop, or a cancel came first).
pub(super) async fn skip_if_refused(
    ctx: &MergeCtx<'_>,
    subject: &RefusalSubject<'_>,
    error: PathedVolumeError,
) -> Result<(), PathedVolumeError> {
    let Some(reason) = refused_here(subject, &error) else {
        return Err(error);
    };
    let item = RefusedItem {
        operation_id: ctx.operation_id,
        source_path: subject.source,
        dest_path: subject.dest,
        source_size: subject.source_size,
        source_is_directory: subject.source_is_directory,
        reason,
    };
    match ask_async(ctx.state, ctx.events, &item).await {
        RefusedNameAnswer::Skip => Ok(()),
        RefusedNameAnswer::Stop => Err(error),
        RefusedNameAnswer::Cancelled => Err(PathedVolumeError::new(
            PathRole::Destination,
            subject.dest,
            VolumeError::Cancelled("Operation cancelled by user".into()),
        )),
    }
}

/// Whether `error` is the destination refusing `subject`'s own name, and why.
pub(super) fn refused_here(subject: &RefusalSubject<'_>, error: &PathedVolumeError) -> Option<RefusedNameReason> {
    if error.role != PathRole::Destination || *failed_native(&error.path) != *subject.dest {
        return None;
    }
    refusal_reason(&error.clone().into_write_error())
}

/// The destination refusing the name `dest` ends in, before anything is written
/// for it: what [`skip_if_refused`] turns into the question for that item.
pub(super) fn name_refused_at(dest: &AppPath) -> PathedVolumeError {
    PathedVolumeError::new(
        PathRole::Destination,
        dest,
        VolumeError::InvalidName("The destination's filesystem doesn't take this name".into()),
    )
}

/// Whether `error` is a source item with no bytes to copy and no way to land as
/// itself (`VolumeError::NoContent`): one counted skip, ❌ never a failure of
/// the item or its folder.
pub(super) fn has_no_content(error: &PathedVolumeError) -> bool {
    matches!(
        error,
        PathedVolumeError {
            error: VolumeError::NoContent(_),
            role: PathRole::Source,
            ..
        }
    )
}
