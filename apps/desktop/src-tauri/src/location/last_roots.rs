//! Each volume's last roots, saved across sessions (plan D7), so a path recorded under an
//! earlier root finds its disk again after that root moved.
//!
//! A volume's id is its identity and its root only an address. On Windows the address moves
//! routinely: a USB stick comes back on `F:` where it was `E:`, two drives swap letters. A
//! restored tab or an undo knows the volume id it was recorded on, so it asks
//! [`live_path`]: when the recorded path isn't under the volume's live root but is under a
//! root that volume held before (this session's, or one saved here by an earlier session), the
//! path is rebased onto the live root. A path already under the live root comes back as is,
//! which is every path on macOS and Linux unless a disk came back at another mount point.
//!
//! `volume-roots.json` in the app data dir holds the last [`ROOTS_KEPT`] roots per volume id.
//! A cache, left on its own fallback on purpose (`state_recovery/DETAILS.md` § Per store):
//! losing it costs one rebase after a root moved, never a person's data.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::file_system::volume::manager::get_volume_manager;
use crate::ignore_poison::IgnorePoison;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::app_paths::{path_is_under, rebase};

/// Filename inside `{app_data_dir}/`.
const FILE_NAME: &str = "volume-roots.json";

/// How many roots each volume keeps, newest last: enough for an undo a few letter changes
/// old, small enough that the file stays a few lines per drive.
const ROOTS_KEPT: usize = 4;

/// On-disk shape: the last roots each volume id answered at, oldest first.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SavedRoots {
    roots: BTreeMap<String, Vec<String>>,
}

impl SavedRoots {
    /// Note `root` as `id`'s newest, keeping [`ROOTS_KEPT`]. `false` when it already was.
    fn note(&mut self, id: &str, root: &str) -> bool {
        let held = self.roots.entry(id.to_string()).or_default();
        if held.last().map(String::as_str) == Some(root) {
            return false;
        }
        held.retain(|known| known != root);
        held.push(root.to_string());
        let excess = held.len().saturating_sub(ROOTS_KEPT);
        held.drain(..excess);
        true
    }
}

/// The roots as last saved, kept in memory so an unchanged root costs no write.
static SAVED: LazyLock<Mutex<Option<SavedRoots>>> = LazyLock::new(|| Mutex::new(None));

fn file_path() -> Option<PathBuf> {
    Some(crate::config::standalone_app_data_dir()?.join(FILE_NAME))
}

/// Every `(id, root)` earlier sessions saved, oldest root first per id, read once. An absent
/// or unreadable file reads as none.
pub(crate) fn saved_roots() -> Vec<(String, String)> {
    let mut saved = SAVED.lock_ignore_poison();
    let roots = saved.get_or_insert_with(|| file_path().map(|path| read(&path)).unwrap_or_default());
    roots
        .roots
        .iter()
        .flat_map(|(id, held)| held.iter().map(move |root| (id.clone(), root.clone())))
        .collect()
}

fn read(path: &Path) -> SavedRoots {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Save `root` as `id`'s newest root, writing the file only when that changed it.
pub(crate) fn save(id: &str, root: &str) {
    if root.is_empty() {
        return;
    }
    let mut saved = SAVED.lock_ignore_poison();
    let roots = saved.get_or_insert_with(|| file_path().map(|path| read(&path)).unwrap_or_default());
    if !roots.note(id, root) {
        return;
    }
    let Some(path) = file_path() else {
        return;
    };
    let json = match serde_json::to_string_pretty(&*roots) {
        Ok(json) => json,
        Err(e) => {
            log::warn!(target: "volume", "Couldn't serialize the volume roots: {e}");
            return;
        }
    };
    if let Err(e) = crate::config::durable_write_json(&path, &path.with_extension("json.tmp"), &json) {
        log::warn!(
            target: "volume",
            "Couldn't save {id}'s root {root}: {e}",
            root = LogPrivate::dir_path(root),
        );
    }
}

/// `path`, recorded on volume `volume_id`, as it reads under that volume's live root now:
/// `Some(rebased)` when the path sits under a root the volume held before and not under its
/// live one, `None` when it's fine as it is, the volume isn't registered, or no earlier root
/// holds it.
pub(crate) fn live_path(volume_id: &str, path: &str) -> Option<String> {
    let live = get_volume_manager()
        .get(volume_id)?
        .root()
        .to_string_lossy()
        .into_owned();
    let earlier = super::facts::earlier_roots_of(volume_id, &live);
    rebased_onto_live_root(path, &live, &earlier)
}

/// The pure half of [`live_path`]: `path` rebased from the deepest of `earlier` that holds it
/// onto `live`, or `None` when `live` already holds it or no earlier root does.
pub(crate) fn rebased_onto_live_root(path: &str, live: &str, earlier: &[String]) -> Option<String> {
    if path_is_under(path, live) {
        return None;
    }
    earlier
        .iter()
        // The deepest root that holds the path wins over a shallower one (`/` holds everything).
        .filter(|root| path_is_under(path, root) && !root.trim_end_matches('/').is_empty())
        .max_by_key(|root| root.len())
        .and_then(|root| rebase(path, root, live))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(list: &[&str]) -> Vec<String> {
        list.iter().map(|root| (*root).to_string()).collect()
    }

    #[test]
    fn a_path_under_the_live_root_stays_as_it_is() {
        assert_eq!(
            rebased_onto_live_root("/Volumes/Backup/x", "/Volumes/Backup", &[]),
            None
        );
        assert_eq!(rebased_onto_live_root("/Users/test_user_1", "/", &roots(&["/"])), None);
        assert_eq!(rebased_onto_live_root("F:/photos", "F:/", &roots(&["E:/"])), None);
    }

    #[test]
    fn a_drive_back_on_another_letter_takes_its_paths_along() {
        assert_eq!(
            rebased_onto_live_root("E:/photos/2026", "F:/", &roots(&["E:/"])),
            Some("F:/photos/2026".to_string())
        );
        assert_eq!(
            rebased_onto_live_root("E:/", "F:/", &roots(&["E:/"])),
            Some("F:/".to_string())
        );
    }

    #[test]
    fn a_disk_back_at_another_mount_point_takes_its_paths_along() {
        assert_eq!(
            rebased_onto_live_root("/Volumes/Backup/x", "/Volumes/Backup 1", &roots(&["/Volumes/Backup"])),
            Some("/Volumes/Backup 1/x".to_string())
        );
    }

    #[test]
    fn a_path_no_earlier_root_holds_is_left_alone() {
        assert_eq!(rebased_onto_live_root("G:/x", "F:/", &roots(&["E:/"])), None);
        assert_eq!(
            rebased_onto_live_root("/Volumes/Other/x", "/Volumes/Backup 1", &roots(&["/Volumes/Backup"])),
            None
        );
        // `/` is the boot disk's root, never an address a moved disk had.
        assert_eq!(
            rebased_onto_live_root("/Users/x", "/Volumes/Backup", &roots(&["/"])),
            None
        );
    }

    #[test]
    fn the_deepest_earlier_root_wins() {
        assert_eq!(
            rebased_onto_live_root("C:/mnt/data/x", "D:/", &roots(&["C:/", "C:/mnt/data"])),
            Some("D:/x".to_string())
        );
    }

    #[test]
    fn a_volume_keeps_its_last_few_roots_newest_last() {
        let mut saved = SavedRoots::default();
        assert!(saved.note("vol-x", "E:/"));
        assert!(!saved.note("vol-x", "E:/"), "an unchanged root writes nothing");
        assert!(saved.note("vol-x", "F:/"));
        assert!(saved.note("vol-x", "E:/"), "back on E: moves E: to newest");
        assert_eq!(saved.roots["vol-x"], ["F:/", "E:/"]);
        for root in ["G:/", "H:/", "I:/"] {
            saved.note("vol-x", root);
        }
        assert_eq!(saved.roots["vol-x"], ["E:/", "G:/", "H:/", "I:/"]);
    }

    #[test]
    fn the_saved_shape_round_trips() {
        let mut saved = SavedRoots::default();
        saved.note("vol-x", "E:/");
        let json = serde_json::to_string(&saved).expect("serializes");
        assert_eq!(serde_json::from_str::<SavedRoots>(&json).expect("parses"), saved);
        assert_eq!(read(Path::new("/definitely/not/here.json")), SavedRoots::default());
    }
}
