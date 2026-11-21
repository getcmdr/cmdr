//! Each undo item's paths on its volumes' live roots, decided ONCE per item, in one place.
//!
//! A volume's root can move between an operation and its undo (a Windows drive back on
//! another letter). The run rebases each unit here before anything reads it, so the
//! progress line, the skip report, the inverse op's journal rows, and the act itself all
//! name the same place. The host is asked once per volume per run, not once per item.

use std::collections::HashMap;

use cmdr_fs::app_path::{AppPath, AppPathBuf};

use super::super::store::UnitReading;
use super::super::types::SkipReason;
use super::runner::VolumeLookup;
use super::target::StoredPath;
use cmdr_fs::volume::app_paths::RootMove;

/// One run's view of where each volume's recorded paths move.
pub(super) struct LiveRoots<'a> {
    volumes: &'a dyn VolumeLookup,
    moves: HashMap<String, Option<RootMove>>,
}

/// A rebased path that names no app path: the live root can't hold the recorded names.
struct Unplaceable;

impl<'a> LiveRoots<'a> {
    pub(super) fn new(volumes: &'a dyn VolumeLookup) -> Self {
        Self {
            volumes,
            moves: HashMap::new(),
        }
    }

    /// `reading` with each path moved onto its volume's live root where the root moved, else
    /// as recorded. A refused row stays as it is: nothing acts on it. A unit whose moved path
    /// names no app path is refused, [`SkipReason::UnreadablePath`]: the undo never acts on a
    /// place it can't name.
    pub(super) fn rebase(&mut self, reading: UnitReading) -> UnitReading {
        let UnitReading::Read(mut unit) = reading else {
            return reading;
        };
        let source = self.moved(&unit.source_volume_id, &unit.source_path);
        let dest = match (&unit.dest_volume_id, &unit.dest_path) {
            (Some(volume_id), Some(path)) => self.moved(volume_id, path),
            _ => Ok(None),
        };
        match (source, dest) {
            (Ok(source), Ok(dest)) => {
                if let Some(moved) = source {
                    unit.source_path = moved;
                }
                if let Some(moved) = dest {
                    unit.dest_path = Some(moved);
                }
                UnitReading::Read(unit)
            }
            (Err(Unplaceable), _) | (_, Err(Unplaceable)) => UnitReading::Refused {
                unit: unit.map_paths(|path| path.stored().into_owned()),
                reason: SkipReason::UnreadablePath,
            },
        }
    }

    fn moved(&mut self, volume_id: &str, path: &AppPath) -> Result<Option<AppPathBuf>, Unplaceable> {
        let volumes = self.volumes;
        let Some(root_move) = self
            .moves
            .entry(volume_id.to_string())
            .or_insert_with(|| volumes.root_move(volume_id))
            .as_ref()
        else {
            return Ok(None);
        };
        match root_move.rebase(path.as_str()) {
            Some(moved) => AppPathBuf::from_wire(&moved).map(Some).map_err(|_| Unplaceable),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::store::RollbackUnit;
    use crate::types::{EntryType, ItemOutcome};
    use cmdr_fs::volume::Volume;

    /// Where the cells' paths sit: `/` here, a drive on Windows.
    const ROOT: &str = if cfg!(windows) { "C:" } else { "" };

    fn p(text: &str) -> AppPathBuf {
        AppPathBuf::from_wire(&format!("{ROOT}{text}")).expect("a canonical app path")
    }

    /// A host whose volume `vol-moved` came back at `/Volumes/New` after the operation ran
    /// at `/Volumes/Old` (a disk back at another mount point; on Windows, a drive back on
    /// another letter), counting how often it's asked.
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
                live: format!("{ROOT}/Volumes/New"),
                earlier: vec![format!("{ROOT}/Volumes/Old")],
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

    fn unit(source_volume: &str, source: &AppPath, dest_volume: &str, dest: &AppPath) -> UnitReading {
        UnitReading::Read(RollbackUnit {
            seq: 1,
            entry_type: EntryType::File,
            source_volume_id: source_volume.to_string(),
            source_path: source.to_owned(),
            dest_volume_id: Some(dest_volume.to_string()),
            dest_path: Some(dest.to_owned()),
            size: Some(3),
            mtime: Some(7),
            overwrote: false,
            outcome: ItemOutcome::Done,
            rollback_skip_reason: None,
            dest_identity: None,
            tags: None,
        })
    }

    fn read(reading: UnitReading) -> RollbackUnit {
        reading.into_read().expect("a typed unit")
    }

    #[test]
    fn an_undo_follows_a_drive_to_its_new_root() {
        let host = MovedDrive::default();
        let mut live = LiveRoots::new(&host);
        let one = read(live.rebase(unit(
            "vol-moved",
            &p("/Volumes/Old/photos/a.jpg"),
            "root",
            &p("/Users/test_user_1/a.jpg"),
        )));
        assert_eq!(one.source_path, p("/Volumes/New/photos/a.jpg"));
        assert_eq!(one.dest_path, Some(p("/Users/test_user_1/a.jpg")));

        let both = read(live.rebase(unit(
            "vol-moved",
            &p("/Volumes/Old/a"),
            "vol-moved",
            &p("/Volumes/Old/b"),
        )));
        assert_eq!(both.source_path, p("/Volumes/New/a"));
        assert_eq!(both.dest_path, Some(p("/Volumes/New/b")));
        assert_eq!(host.asked.load(Ordering::Relaxed), 2, "once per volume, not per item");
    }

    #[test]
    fn a_unit_whose_volumes_stood_still_is_reversed_as_recorded() {
        let recorded = unit("vol-x", &p("/Volumes/Backup/a"), "root", &p("/Users/test_user_1/a"));
        assert_eq!(
            LiveRoots::new(&MovedDrive::default()).rebase(recorded.clone()),
            recorded
        );
        assert_eq!(LiveRoots::new(&Unmoved).rebase(recorded.clone()), recorded);
    }

    /// A live root the recorded names can't sit under (here, relative text no host hands
    /// out) names no app path, so the unit is refused rather than acted on at the old root.
    #[test]
    fn a_move_onto_a_root_that_names_no_app_path_refuses_the_unit() {
        struct Broken;
        impl VolumeLookup for Broken {
            fn get(&self, _volume_id: &str) -> Option<Arc<dyn Volume>> {
                None
            }
            fn root_move(&self, _volume_id: &str) -> Option<RootMove> {
                Some(RootMove {
                    live: "relative/root".to_string(),
                    earlier: vec![format!("{ROOT}/Volumes/Old")],
                })
            }
        }
        let refused = LiveRoots::new(&Broken).rebase(unit(
            "vol-moved",
            &p("/Volumes/Old/a"),
            "vol-moved",
            &p("/Volumes/Old/b"),
        ));
        let UnitReading::Refused { unit, reason } = refused else {
            panic!("refused, never acted on at a place it can't name");
        };
        assert_eq!(reason, SkipReason::UnreadablePath);
        assert_eq!(unit.source_path, format!("{ROOT}/Volumes/Old/a"), "the row as recorded");
    }
}
