//! A local item's metadata by path, for both siblings: putting a source's metadata on an item
//! that has landed (`Volume::apply_metadata`), and reading what a folder carries
//! (`Volume::folder_metadata`). The streamed write's own apply, on the open handle, is the
//! shared `crate::streams`. Every OS difference lives in `cmdr_fs::metadata`'s arms, so this
//! body is the same on each: a class an OS can't hold answers `NotApplicable` there.

use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::{
    CarriedMetadata, LandedMetadata, MetadataClasses, MetadataReport, StagedItem, StagedTimes, UserFlags, apply_landed,
    apply_staged, default_file_mode, default_folder_mode, routed_mode,
};
use std::path::Path;

/// [`Volume::apply_metadata`](cmdr_fs::volume::Volume::apply_metadata)
/// for a local item that has landed: the xattrs, the routed mode folded against
/// the item's kind, birth and mtime in one call, then the flags last (Locked
/// refuses every later write). `path` is the OS's path.
pub(crate) fn apply_metadata_at(
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
                log::debug!(target: "local", "apply_metadata: couldn't stat {}: {e}", LogPrivate::path(path));
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
            target: "local",
            "apply_metadata: {} took only some of the source's metadata: {report:?}",
            LogPrivate::path(path)
        );
    }
    report
}

/// What the folder at `path` (the OS's path) carries beyond its listing entry, among
/// `wanted`'s xattrs and flags: one stat, which answers the flags, and one xattr listing plus
/// a read per name when the xattrs are asked for. Anything but a folder carries nothing here.
pub(crate) fn folder_metadata_at(path: &Path, wanted: MetadataClasses) -> CarriedMetadata {
    let wanted = wanted.intersection(MetadataClasses::XATTRS | MetadataClasses::FLAGS);
    if wanted.is_empty() {
        return CarriedMetadata::default();
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && wanted.contains(MetadataClasses::XATTRS) => {
            CarriedMetadata::of_local_dir(path, &meta).restricted_to(wanted)
        }
        Ok(meta) if meta.is_dir() => CarriedMetadata::of_local_stat(&meta).restricted_to(wanted),
        _ => CarriedMetadata::default(),
    }
}
