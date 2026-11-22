//! The conflict scan's body (`Volume::scan_for_conflicts`), which the trait method in
//! `local_windows.rs` delegates to, as `local_posix/scan.rs` holds the POSIX one.

use cmdr_fs::app_path::{AppName, AppPath, PathArithmetic as _};
use cmdr_fs::log_error::LogError;
use cmdr_fs::volume::{ScanConflict, SourceItemInfo, VolumeError};

use super::paths::native_of;

/// The same per-name probe as `LocalPosixVolume`'s: a name under the folder `dest` (an app
/// path) is a conflict when something answers a stat there (a link followed), so a missing
/// destination folder reads empty.
pub(super) fn conflicts_under(dest: &AppPath, source_items: &[SourceItemInfo]) -> Vec<ScanConflict> {
    let mut conflicts = Vec::new();
    for item in source_items {
        // An item whose name names no child of the destination can't collide: it's refused
        // typed, alone, and logged (a conflict carries no per-item error), and the batch's
        // other items are still checked, as on POSIX.
        let dest_file = match AppName::from_app_spelling(&item.name) {
            Ok(name) => dest
                .join(name)
                .map_err(|why| VolumeError::InvalidName(why.to_string().into())),
            Err(refused) => Err(VolumeError::from(refused)),
        };
        let dest_file = match dest_file {
            Ok(dest_file) => dest_file,
            Err(refused) => {
                log::warn!("scan_for_conflicts: skipping a source name: {}", LogError(&refused));
                continue;
            }
        };
        let Ok(meta) = native_of(&dest_file).and_then(std::fs::metadata) else {
            continue;
        };
        let dest_modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_secs()).ok());
        conflicts.push(ScanConflict {
            source_path: item.name.clone(),
            dest_path: dest_file.into_string(),
            source_size: item.size,
            dest_size: meta.len(),
            source_modified: item.modified,
            dest_modified,
            source_is_directory: item.is_directory,
            dest_is_directory: meta.is_dir(),
        });
    }
    conflicts
}
