//! Putting a source's metadata on a POSIX item that has landed (`Volume::apply_metadata`).
//! The streamed write's own apply, on the open handle, is the shared `crate::streams`.

use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::{
    CarriedMetadata, LandedMetadata, MetadataReport, StagedItem, StagedTimes, UserFlags, apply_landed, apply_staged,
    default_file_mode, default_folder_mode, routed_mode,
};
use std::path::Path;

/// [`Volume::apply_metadata`](cmdr_fs::volume::Volume::apply_metadata)
/// for a local item that has landed: the xattrs, the routed mode folded against
/// the item's kind, birth and mtime in one call, then the flags last (Locked
/// refuses every later write).
pub(super) fn apply_metadata_at(
    path: &Path,
    carried: &CarriedMetadata,
    modified: Option<std::time::SystemTime>,
) -> MetadataReport {
    // The kind decides the default the mode folds against; an item that can't
    // be stat'ed keeps the mode it has.
    let mode = carried.mode.and_then(|mode| {
        let default = match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => default_folder_mode(),
            Ok(_) => default_file_mode(),
            Err(e) => {
                log::debug!(target: "local_posix", "apply_metadata: couldn't stat {}: {e}", LogPrivate::path(path));
                return None;
            }
        };
        routed_mode(mode, default)
    });
    let staged = CarriedMetadata {
        mode,
        flags: UserFlags::NONE,
        ..carried.clone()
    };
    let mut report = if staged.is_empty() && modified.is_none() {
        MetadataReport::ABSENT
    } else {
        apply_staged(
            StagedItem::Path(path),
            &staged,
            StagedTimes {
                modified,
                accessed: None,
            },
        )
    };
    if !carried.flags.is_empty() {
        report.flags = apply_landed(path, &LandedMetadata::default().with_flags(carried.flags)).flags;
    }
    if report.any_refused() {
        log::warn!(
            target: "local_posix",
            "apply_metadata: {} took only some of the source's metadata: {report:?}",
            LogPrivate::path(path)
        );
    }
    report
}
