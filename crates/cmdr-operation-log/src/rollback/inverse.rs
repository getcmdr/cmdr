//! The per-item inverse acts: recheck one journaled item against its snapshot and
//! its restore target, and only when both hold hand the decided act to the runner
//! (`DETAILS.md` § "The two data-safety guards"). Each is an
//! `async fn(volumes, runner, unit) -> ItemResult`, reached through one `match` on
//! [`InverseAction`] in [`reverse_item`].
//!
//! **Data-safety-critical**: every act here fails safe. Drift, an unverifiable
//! precondition, an occupied target, or a gone original is a SKIP, never an act.

use std::path::Path;

use cmdr_fs::volume::{Volume, VolumeError};

use super::super::store::RollbackUnit;
use super::super::types::{EntryType, OpKind, SkipReason};
use super::target::{missing_item, removal_target, vanished_item};
use super::{
    ActRefusal, InverseAct, InverseAction, ItemResult, RollbackRunner, SnapshotVerdict, VolumeLookup,
    is_created_in_place, is_self_collision, item_action, restore_tags, verify_snapshot,
};

/// Reverse one item: derive the inverse action, recheck against the snapshot, and
/// (only if it verifies AND the target is clear) hand the decided act to the
/// runner. Every read here goes through the `Volume` trait, so local and remote
/// verify uniformly.
pub(super) async fn reverse_item(
    volumes: &dyn VolumeLookup,
    runner: &dyn RollbackRunner,
    kind: OpKind,
    unit: &RollbackUnit,
) -> ItemResult {
    let rebased = on_live_roots(volumes, unit);
    let unit = rebased.as_ref().unwrap_or(unit);
    let Some(action) = item_action(kind, unit.entry_type, is_created_in_place(unit)) else {
        return ItemResult::Skipped(SkipReason::Failed);
    };
    match action {
        InverseAction::RemoveFileIfUnchanged => remove_file_if_unchanged(volumes, runner, unit, false).await,
        InverseAction::RemoveCopyIfOriginalStays => remove_file_if_unchanged(volumes, runner, unit, true).await,
        InverseAction::RemoveDirIfEmpty => remove_dir_if_empty(volumes, runner, unit).await,
        InverseAction::RestoreMove => restore_move(volumes, runner, unit).await,
        InverseAction::RestoreTags => restore_tags(volumes, runner, unit).await,
    }
}

/// `unit` with its paths rebased onto their volumes' live roots, when either volume's root
/// moved since the operation ran ([`VolumeLookup::live_path`]); `None` when both paths stand
/// as recorded, which is every unit whose volumes sit where they did.
fn on_live_roots(volumes: &dyn VolumeLookup, unit: &RollbackUnit) -> Option<RollbackUnit> {
    let source = volumes.live_path(&unit.source_volume_id, &unit.source_path);
    let dest = match (&unit.dest_volume_id, &unit.dest_path) {
        (Some(volume_id), Some(path)) => volumes.live_path(volume_id, path),
        _ => None,
    };
    if source.is_none() && dest.is_none() {
        return None;
    }
    let mut rebased = unit.clone();
    if let Some(source) = source {
        rebased.source_path = source;
    }
    if let Some(dest) = dest {
        rebased.dest_path = Some(dest);
    }
    Some(rebased)
}

/// Remove the file a removal inverse targets, once it verifies against its snapshot.
/// `check_original` (a copy) also requires the original to still be there
/// ([`original_stays`]); it's checked after the copy verifies, so a copy already
/// gone stays an idempotent no-op whatever happened to its original.
async fn remove_file_if_unchanged(
    volumes: &dyn VolumeLookup,
    runner: &dyn RollbackRunner,
    unit: &RollbackUnit,
    check_original: bool,
) -> ItemResult {
    let (vol_id, path) = removal_target(unit);
    let Some(volume) = volumes.get(&vol_id) else {
        return ItemResult::Skipped(SkipReason::Failed);
    };
    let live = match volume.get_metadata(&path).await {
        Ok(entry) => entry,
        // Already gone ⇒ the desired end state already holds (idempotent), as long
        // as the volume still reaches the path.
        Err(VolumeError::NotFound(_)) => return missing_item(&[(&*volume, path.as_path())]),
        Err(_) => return ItemResult::Skipped(SkipReason::Failed),
    };
    // It must be the very node the op landed, when the op recorded which: a file
    // saved over the copy since can share its size and date.
    if let Err(skip) = landed_node_holds(&*volume, &path, unit).await {
        return ItemResult::Skipped(skip);
    }
    match verify_snapshot(unit.size, unit.mtime, live.size, live.modified_at) {
        SnapshotVerdict::Match => {
            if check_original && let Err(kept) = original_stays(volumes, unit).await {
                return ItemResult::Skipped(kept);
            }
            let removed = runner
                .perform(InverseAct::RemoveFile {
                    volume: &volume,
                    path: &path,
                })
                .await;
            removal_verdict(removed, &*volume, &path)
        }
        SnapshotVerdict::Drift => ItemResult::Skipped(SkipReason::Drift),
        SnapshotVerdict::Unverifiable => ItemResult::Skipped(SkipReason::UnverifiablePrecondition),
    }
}

/// Whether a copy's original is still where it was copied from (`unit.source_path`
/// on `unit.source_volume_id`), as the file that was copied: present, a file, and
/// the recorded size. Undoing a copy deletes it permanently, and "the copy is
/// redundant" only holds while the original exists: copy photos off a card, delete
/// them there, and the copy is the only one left.
///
/// Size only, never mtime: a local copy records the source's preserved mtime, but a
/// volume copy records the destination's own, which no source matches.
///
/// Fails safe on doubt: gone, a directory now, or a different size is
/// [`SkipReason::OriginalGone`]; a source volume that isn't registered, a stat that
/// errors for another reason, a size the backend can't report, or a `NotFound` for a
/// path outside the volume's root (a narrowed remote place, see [`missing_item`]) is
/// [`SkipReason::UnverifiablePrecondition`]. An original that a later logged
/// operation moved elsewhere counts as gone too: the journal isn't consulted, so a
/// copy is only ever removed against an original seen in place.
async fn original_stays(volumes: &dyn VolumeLookup, unit: &RollbackUnit) -> Result<(), SkipReason> {
    let Some(volume) = volumes.get(&unit.source_volume_id) else {
        return Err(SkipReason::UnverifiablePrecondition);
    };
    let path = unit.source_path.as_path();
    let original = match volume.get_metadata(path).await {
        Ok(entry) => entry,
        Err(VolumeError::NotFound(_)) if path.starts_with(volume.root()) => return Err(SkipReason::OriginalGone),
        Err(_) => return Err(SkipReason::UnverifiablePrecondition),
    };
    if original.is_directory {
        return Err(SkipReason::OriginalGone);
    }
    match (unit.size, original.size) {
        (None, _) => Ok(()),
        (Some(_), None) => Err(SkipReason::UnverifiablePrecondition),
        (Some(recorded), Some(live)) if live as i64 == recorded => Ok(()),
        (Some(_), Some(_)) => Err(SkipReason::OriginalGone),
    }
}

/// The verdict on a removal the planner decided: a file verified unchanged, or a
/// directory verified empty.
fn removal_verdict(removed: Result<(), ActRefusal>, volume: &dyn Volume, path: &Path) -> ItemResult {
    match removed {
        Ok(()) => ItemResult::Reversed,
        Err(ActRefusal::Volume(VolumeError::NotFound(_))) => missing_item(&[(volume, path)]),
        Err(ActRefusal::Locked) => ItemResult::Skipped(SkipReason::Locked),
        Err(ActRefusal::Volume(_)) => ItemResult::Skipped(SkipReason::Failed),
    }
}

async fn remove_dir_if_empty(
    volumes: &dyn VolumeLookup,
    runner: &dyn RollbackRunner,
    unit: &RollbackUnit,
) -> ItemResult {
    let (vol_id, path) = removal_target(unit);
    let Some(volume) = volumes.get(&vol_id) else {
        return ItemResult::Skipped(SkipReason::Failed);
    };
    if !volume.exists(&path).await {
        // Already removed ⇒ idempotent no-op, as long as the volume still reaches
        // the path.
        return missing_item(&[(&*volume, path.as_path())]);
    }
    if let Err(skip) = landed_node_holds(&*volume, &path, unit).await {
        return ItemResult::Skipped(skip);
    }
    // Only remove a directory the undo created if it's still empty — a file added
    // since must not be swept away. A `seq DESC` stream removes the dir's own
    // (unchanged) contents first, so a genuinely-restored tree is empty here.
    match volume.list_directory(&path, None).await {
        Ok(entries) if entries.is_empty() => {
            let removed = runner
                .perform(InverseAct::RemoveDir {
                    volume: &volume,
                    path: &path,
                })
                .await;
            removal_verdict(removed, &*volume, &path)
        }
        Ok(_) => ItemResult::Skipped(SkipReason::DirNotEmpty),
        Err(_) => ItemResult::Skipped(SkipReason::Failed),
    }
}

/// Whether the entry at `path` is still the node `unit`'s operation landed there,
/// for a row that recorded which. A row with no identity (written before the journal
/// kept one, or landed where no stable identity exists) holds: it keeps the snapshot
/// check alone. Every inverse asks this before it acts on what landed, so an identity
/// on a row is never ignored.
///
/// A different node is [`SkipReason::Drift`]: someone else's item under the recorded
/// name. A volume that can't say, or a stat that fails, proves nothing, so it fails
/// safe too. One `lstat`, never a walk.
async fn landed_node_holds(volume: &dyn Volume, path: &Path, unit: &RollbackUnit) -> Result<(), SkipReason> {
    let Some(recorded) = unit.dest_identity else {
        return Ok(());
    };
    match volume.node_identity(path).await {
        Ok(Some(live)) if live == recorded => Ok(()),
        Ok(Some(_)) => Err(SkipReason::Drift),
        Ok(None) => Err(SkipReason::UnverifiablePrecondition),
        Err(_) => Err(SkipReason::Failed),
    }
}

async fn restore_move(volumes: &dyn VolumeLookup, runner: &dyn RollbackRunner, unit: &RollbackUnit) -> ItemResult {
    if !unit.awaits_reversal() {
        return ItemResult::Skipped(SkipReason::Failed);
    }
    // Restore moves the item back FROM where it landed (dest) TO its original
    // (source). Both must be present in the row (move/trash/rename all record a
    // dest); a row without one is a journal shape bug — skip safe.
    let (Some(from_vol_id), Some(from_path)) = (&unit.dest_volume_id, &unit.dest_path) else {
        return ItemResult::Skipped(SkipReason::Failed);
    };
    let to_vol_id = &unit.source_volume_id;
    let to_path = &unit.source_path;

    let Some(from_volume) = volumes.get(from_vol_id) else {
        return ItemResult::Skipped(SkipReason::Failed);
    };
    let Some(to_volume) = volumes.get(to_vol_id) else {
        return ItemResult::Skipped(SkipReason::Failed);
    };
    // Either side refusing a path means the same thing `missing_item` guards: a
    // root that no longer reaches it.
    let sides = [(&*from_volume, from_path.as_path()), (&*to_volume, to_path.as_path())];

    // The thing to move back must still be where the op left it.
    let from_entry = match from_volume.get_metadata(from_path).await {
        Ok(e) => e,
        // Gone from there. Whether that's an undo already done depends on the other side:
        // back home (an earlier undo, Finder's Put Back) is the end state we wanted; absent
        // from home too (trash emptied, deleted since) means nothing came back, and the
        // undo must say so rather than credit a restore that never happened.
        Err(VolumeError::NotFound(_)) => {
            return match to_volume.get_metadata(to_path).await {
                Ok(_) => missing_item(&sides),
                Err(VolumeError::NotFound(_)) => vanished_item(&sides),
                Err(_) => ItemResult::Skipped(SkipReason::Failed),
            };
        }
        Err(_) => return ItemResult::Skipped(SkipReason::Failed),
    };
    // It must be the very node the op left there, when the op recorded which: the
    // Trash gives a freed name to the next item trashed under it, and a folder's
    // recheck below is existence alone.
    if let Err(skip) = landed_node_holds(&*from_volume, from_path, unit).await {
        return ItemResult::Skipped(skip);
    }
    // For a file, verify it hasn't changed since the op (dirs: existence only —
    // a subtree isn't cheaply verifiable, so existence + a clear target is the
    // contract). Drift / unverifiable ⇒ skip.
    if unit.entry_type == EntryType::File {
        match verify_snapshot(unit.size, unit.mtime, from_entry.size, from_entry.modified_at) {
            SnapshotVerdict::Match => {}
            SnapshotVerdict::Drift => return ItemResult::Skipped(SkipReason::Drift),
            SnapshotVerdict::Unverifiable => return ItemResult::Skipped(SkipReason::UnverifiablePrecondition),
        }
    }

    // Pinned non-destructive policy: never overwrite the restore target. If it's
    // occupied by a DIFFERENT entry, skip. A self-collision (a case-insensitive
    // volume answering for the target spelling with the very entry we're
    // restoring) is not a real collision, so restoring over it is safe: `force`
    // lets the same-entry rename land where the volume reports the target "exists".
    let same_volume = from_vol_id == to_vol_id;
    let mut force = false;
    if let Ok(occupant) = to_volume.get_metadata(to_path).await {
        if is_self_collision(&*to_volume, same_volume, from_path, to_path, &from_entry, &occupant).await {
            force = true;
        } else {
            return ItemResult::Skipped(SkipReason::RestoreTargetOccupied);
        }
    }

    // A cross-volume restore is always per-leaf (directories are recorded per file,
    // and cross-volume can't be a self-collision, so the target is clear); a
    // directory row reaching the streaming path would be a journal-shape bug, so
    // refuse it here rather than asking the executor to guess.
    if !same_volume && from_entry.is_directory {
        return ItemResult::Skipped(SkipReason::Failed);
    }
    match runner
        .perform(InverseAct::Restore {
            from: &from_volume,
            from_path,
            to: &to_volume,
            to_path,
            same_volume,
            force,
        })
        .await
    {
        Ok(()) => ItemResult::Reversed,
        Err(ActRefusal::Volume(VolumeError::AlreadyExists(_))) => {
            ItemResult::Skipped(SkipReason::RestoreTargetOccupied)
        }
        Err(ActRefusal::Volume(VolumeError::NotFound(_))) => missing_item(&sides),
        Err(_) => ItemResult::Skipped(SkipReason::Failed),
    }
}

#[cfg(test)]
mod live_root_tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::types::ItemOutcome;

    /// A host whose volume `vol-moved` came back at `F:/` after the operation ran on `E:/`.
    struct MovedDrive;

    impl VolumeLookup for MovedDrive {
        fn get(&self, _volume_id: &str) -> Option<Arc<dyn Volume>> {
            None
        }

        fn live_path(&self, volume_id: &str, path: &Path) -> Option<PathBuf> {
            let path = path.to_str()?;
            (volume_id == "vol-moved")
                .then(|| path.strip_prefix("E:/").map(|rest| PathBuf::from(format!("F:/{rest}"))))
                .flatten()
        }
    }

    /// The trait's default answer: every recorded path stands.
    struct Unmoved;

    impl VolumeLookup for Unmoved {
        fn get(&self, _volume_id: &str) -> Option<Arc<dyn Volume>> {
            None
        }
    }

    fn moved_unit(source_volume: &str, source: &str, dest_volume: &str, dest: &str) -> RollbackUnit {
        RollbackUnit {
            seq: 1,
            entry_type: EntryType::File,
            source_volume_id: source_volume.to_string(),
            source_path: PathBuf::from(source),
            dest_volume_id: Some(dest_volume.to_string()),
            dest_path: Some(PathBuf::from(dest)),
            size: Some(3),
            mtime: Some(7),
            overwrote: false,
            outcome: ItemOutcome::Done,
            rollback_skip_reason: None,
            dest_identity: None,
            tags: None,
        }
    }

    #[test]
    fn an_undo_follows_a_drive_to_its_new_letter() {
        let unit = moved_unit("vol-moved", "E:/photos/a.jpg", "root", "C:/Users/test_user_1/a.jpg");
        let rebased = on_live_roots(&MovedDrive, &unit).expect("the source drive moved");
        assert_eq!(rebased.source_path, PathBuf::from("F:/photos/a.jpg"));
        assert_eq!(rebased.dest_path, unit.dest_path, "the other side stood still");

        let both = moved_unit("vol-moved", "E:/a", "vol-moved", "E:/b");
        let rebased = on_live_roots(&MovedDrive, &both).expect("both sides moved");
        assert_eq!(rebased.dest_path, Some(PathBuf::from("F:/b")));
    }

    #[test]
    fn a_unit_whose_volumes_stood_still_is_reversed_as_recorded() {
        let unit = moved_unit("vol-x", "/Volumes/Backup/a", "root", "/Users/test_user_1/a");
        assert_eq!(on_live_roots(&MovedDrive, &unit), None);
        assert_eq!(on_live_roots(&Unmoved, &unit), None);
    }
}
