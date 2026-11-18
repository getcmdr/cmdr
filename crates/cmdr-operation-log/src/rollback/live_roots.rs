//! Each undo item's paths on its volumes' live roots, decided ONCE per item, in one place.
//!
//! A volume's root can move between an operation and its undo (a Windows drive back on
//! another letter). The run rebases each unit here before anything reads it, so the
//! progress line, the skip report, the inverse op's journal rows, and the act itself all
//! name the same place. The host is asked once per volume per run, not once per item.

use std::collections::HashMap;
use std::path::PathBuf;

use super::super::store::RollbackUnit;
use super::runner::VolumeLookup;
use cmdr_fs::volume::app_paths::RootMove;

/// One run's view of where each volume's recorded paths move.
pub(super) struct LiveRoots<'a> {
    volumes: &'a dyn VolumeLookup,
    moves: HashMap<String, Option<RootMove>>,
}

impl<'a> LiveRoots<'a> {
    pub(super) fn new(volumes: &'a dyn VolumeLookup) -> Self {
        Self {
            volumes,
            moves: HashMap::new(),
        }
    }

    /// `unit` with each path moved onto its volume's live root where the root moved, else
    /// as recorded. A path that isn't UTF-8 stays as recorded.
    pub(super) fn rebase(&mut self, mut unit: RollbackUnit) -> RollbackUnit {
        if let Some(moved) = self.moved(&unit.source_volume_id, &unit.source_path) {
            unit.source_path = moved;
        }
        if let (Some(volume_id), Some(path)) = (unit.dest_volume_id.clone(), unit.dest_path.as_ref())
            && let Some(moved) = self.moved(&volume_id, path)
        {
            unit.dest_path = Some(moved);
        }
        unit
    }

    fn moved(&mut self, volume_id: &str, path: &std::path::Path) -> Option<PathBuf> {
        let volumes = self.volumes;
        let root_move = self
            .moves
            .entry(volume_id.to_string())
            .or_insert_with(|| volumes.root_move(volume_id))
            .as_ref()?;
        root_move.rebase(path.to_str()?).map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::types::{EntryType, ItemOutcome};
    use cmdr_fs::volume::Volume;

    /// A host whose volume `vol-moved` came back at `F:/` after the operation ran on `E:/`,
    /// counting how often it's asked.
    #[derive(Default)]
    struct MovedDrive {
        asked: AtomicUsize,
    }

    impl VolumeLookup for MovedDrive {
        fn get(&self, _volume_id: &str) -> Option<Arc<dyn Volume>> {
            None
        }

        fn root_move(&self, volume_id: &str) -> Option<RootMove> {
            self.asked.fetch_add(1, Ordering::Relaxed);
            (volume_id == "vol-moved").then(|| RootMove {
                live: "F:/".to_string(),
                earlier: vec!["E:/".to_string()],
            })
        }
    }

    /// The trait's default answer: every recorded path stands.
    struct Unmoved;

    impl VolumeLookup for Unmoved {
        fn get(&self, _volume_id: &str) -> Option<Arc<dyn Volume>> {
            None
        }
    }

    fn unit(source_volume: &str, source: &str, dest_volume: &str, dest: &str) -> RollbackUnit {
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
        let host = MovedDrive::default();
        let mut live = LiveRoots::new(&host);
        let one = live.rebase(unit(
            "vol-moved",
            "E:/photos/a.jpg",
            "root",
            "C:/Users/test_user_1/a.jpg",
        ));
        assert_eq!(one.source_path, Path::new("F:/photos/a.jpg"));
        assert_eq!(one.dest_path.as_deref(), Some(Path::new("C:/Users/test_user_1/a.jpg")));

        let both = live.rebase(unit("vol-moved", "E:/a", "vol-moved", "E:/b"));
        assert_eq!(both.source_path, Path::new("F:/a"));
        assert_eq!(both.dest_path.as_deref(), Some(Path::new("F:/b")));
        assert_eq!(host.asked.load(Ordering::Relaxed), 2, "once per volume, not per item");
    }

    #[test]
    fn a_unit_whose_volumes_stood_still_is_reversed_as_recorded() {
        let recorded = unit("vol-x", "/Volumes/Backup/a", "root", "/Users/test_user_1/a");
        assert_eq!(
            LiveRoots::new(&MovedDrive::default()).rebase(recorded.clone()),
            recorded
        );
        assert_eq!(LiveRoots::new(&Unmoved).rebase(recorded.clone()), recorded);
    }
}
