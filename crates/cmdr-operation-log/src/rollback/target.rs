//! Where an inverse acts, and what a volume's "not there" means for it. Pure helpers
//! the acts (`inverse.rs`, `tags.rs`), the runner's progress, and the bookkeeping all
//! share, kept in a leaf so none of them depends on another for them.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, PathArithmetic};
use cmdr_fs::volume::Volume;

use super::super::store::RollbackUnit;
use super::super::types::SkipReason;
use super::ItemResult;

/// The (volume, path) a removal inverse targets: the item's dest (the copy /
/// created path), falling back to source when no dest was recorded.
pub(super) fn removal_target(unit: &RollbackUnit) -> (String, PathBuf) {
    match (&unit.dest_volume_id, &unit.dest_path) {
        (Some(vol), Some(path)) => (vol.clone(), path.clone()),
        _ => (unit.source_volume_id.clone(), unit.source_path.clone()),
    }
}

/// A unit's recorded path as the app path it holds, for the acts' volume reads and
/// arithmetic and the runner's [`super::InverseAct`]. ❗ The planner's one crossing:
/// [`RollbackUnit`] carries `std::path` values, so the path is the stored app text in a
/// `PathBuf`, and this reads it back losslessly. It goes when the unit carries app paths
/// built through the escape mark's `StoredPathReading`.
pub(super) fn app_path_of(path: &Path) -> Cow<'_, AppPath> {
    AppPath::legacy_unchecked(path)
}

/// The verdict for an item a volume answered "not there" about, given every
/// `(volume, path)` side the reversal touches.
///
/// A volume refuses a path above its own root as `NotFound`, and a remote place's
/// root can be narrowed after the operation ran (`network/DETAILS.md` § "Editing a
/// connected place"). An item there is NOT gone: it sits where the operation left
/// it, out of the volume's reach, and counting it as reversed reports an undo that
/// removed nothing. So it fails safe; only a path every side still serves can be
/// already gone. Containment is by whole names (the app path's `starts_with`), so
/// `/share/tmp2` isn't under `/share/tmp`.
pub(super) fn missing_item(sides: &[(&dyn Volume, &AppPath)]) -> ItemResult {
    if sides.iter().all(|(volume, path)| path.starts_with(volume.root())) {
        ItemResult::Skipped(SkipReason::AlreadyGone)
    } else {
        ItemResult::Skipped(SkipReason::Failed)
    }
}

/// The verdict for a restore whose item is gone from BOTH sides: `Vanished`, under the same
/// containment rule as [`missing_item`] (a path a narrowed root no longer reaches isn't gone).
pub(super) fn vanished_item(sides: &[(&dyn Volume, &AppPath)]) -> ItemResult {
    match missing_item(sides) {
        ItemResult::Skipped(SkipReason::AlreadyGone) => ItemResult::Skipped(SkipReason::Vanished),
        out_of_reach => out_of_reach,
    }
}

#[cfg(test)]
mod tests {
    use cmdr_fs::volume::InMemoryVolume;

    use super::*;

    /// "Already gone" needs a path every side's root still reaches, by whole names. A
    /// recorded path holding a `..` sits under no root: `Path::starts_with` read
    /// `/Volumes/x/../y` as under `/Volumes/x`, though it names `/Volumes/y`, outside it,
    /// so the undo credited a removal it never checked. It fails safe now.
    #[test]
    fn only_a_path_its_root_reaches_by_whole_names_is_already_gone() {
        let volume = InMemoryVolume::new("t").with_root("/Volumes/x");
        let sides = |text: &str| missing_item(&[(&volume, &app_path_of(Path::new(text)))]);
        assert!(matches!(
            sides("/Volumes/x/a"),
            ItemResult::Skipped(SkipReason::AlreadyGone)
        ));
        assert!(matches!(sides("/Volumes/xa"), ItemResult::Skipped(SkipReason::Failed)));
        assert!(matches!(
            sides("/Volumes/x/../y"),
            ItemResult::Skipped(SkipReason::Failed)
        ));
        assert!(matches!(
            vanished_item(&[(&volume, &app_path_of(Path::new("/Volumes/x/a")))]),
            ItemResult::Skipped(SkipReason::Vanished)
        ));
    }
}
