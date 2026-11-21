//! Where an inverse acts, and what a volume's "not there" means for it. Pure helpers
//! the acts (`inverse.rs`, `tags.rs`), the runner's progress, and the bookkeeping all
//! share, kept in a leaf so none of them depends on another for them.

use std::borrow::Cow;
use std::path::Path;

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathArithmetic, is_escape, shown_name, unescape_bytes, unescape_units};
use cmdr_fs::volume::Volume;

use super::super::store::RollbackUnit;
use super::super::types::SkipReason;
use super::ItemResult;

/// The (volume, path) a removal inverse targets: the item's dest (the copy /
/// created path), falling back to source when no dest was recorded.
pub(super) fn removal_target<P>(unit: &RollbackUnit<P>) -> (&str, &P) {
    match (&unit.dest_volume_id, &unit.dest_path) {
        (Some(vol), Some(path)) => (vol, path),
        _ => (&unit.source_volume_id, &unit.source_path),
    }
}

/// A unit's path as the journal spells it, for the books the planner writes back (the
/// inverse op's rows, a skip's example name, the progress line). A refused row holds its
/// stored text; a typed path decodes each escape back to the literal text it stands for
/// ([`literal_text`]).
pub(super) trait StoredPath: PartialEq {
    /// The text the journal holds for this path.
    fn stored(&self) -> Cow<'_, str>;
}

impl StoredPath for AppPathBuf {
    fn stored(&self) -> Cow<'_, str> {
        literal_text(self.as_str())
    }
}

impl StoredPath for String {
    fn stored(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

/// The journal's spelling of app text: while escapes aren't live, every escape in a unit's
/// path came from a literal escape-range code point (`AppPathBuf::from_literal_store` made
/// it, through either alphabet), so decoding it gives back exactly the text the row held;
/// borrowed when there's none (nearly every path). Once escapes are live, an escape of a
/// name that isn't text decodes lossily, as the journal's `to_string_lossy` spells one.
fn literal_text(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_escape) {
        return Cow::Borrowed(text);
    }
    if let Ok(bytes) = unescape_bytes(text) {
        return Cow::Owned(String::from_utf8_lossy(&bytes).into_owned());
    }
    match unescape_units(text) {
        Ok(units) => Cow::Owned(String::from_utf16_lossy(&units)),
        // Both alphabets in one path, which no namer makes: what a person reads.
        Err(_) => shown_name(text),
    }
}

/// The leaf of a stored path, split as the journal splits one (`std::path`'s rule over the
/// stored text, which is what every row's dir and name were made by).
pub(super) fn stored_leaf<P: StoredPath>(path: &P) -> Option<String> {
    let stored = path.stored();
    Path::new(stored.as_ref())
        .file_name()
        .map(|leaf| leaf.to_string_lossy().into_owned())
}

/// How deep a stored path sits, counted as the journal's `std::path` reading counts it
/// (its root's own components included), so the folder sweep's deepest-first order across
/// every root kind is the one it always was.
pub(super) fn stored_depth<P: StoredPath>(path: &P) -> usize {
    let stored = path.stored();
    Path::new(stored.as_ref()).components().count()
}

/// A stored path's (dir, name), split as [`stored_leaf`] splits it.
pub(super) fn stored_split<P: StoredPath>(path: &P) -> (String, String) {
    let stored = path.stored();
    let split = Path::new(stored.as_ref());
    let name = split
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = split
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    (dir, name)
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
        let sides = |path: &AppPath| missing_item(&[(&volume, path)]);
        assert!(matches!(
            sides(cmdr_fs::app_path!("/Volumes/x/a")),
            ItemResult::Skipped(SkipReason::AlreadyGone)
        ));
        assert!(matches!(
            sides(cmdr_fs::app_path!("/Volumes/xa")),
            ItemResult::Skipped(SkipReason::Failed)
        ));
        assert!(matches!(
            vanished_item(&[(&volume, cmdr_fs::app_path!("/Volumes/x/a"))]),
            ItemResult::Skipped(SkipReason::Vanished)
        ));
        // A `..` never reaches here: the typed reading refuses it as `UnreadablePath`
        // (`store/rollback_unit.rs`), so no act or verdict ever reads it.
        assert!(AppPathBuf::from_wire("/Volumes/x/../y").is_err());
    }

    /// The journal's spelling of a typed path is the literal text the row held, whichever
    /// alphabet its volume's namer escaped a literal escape-range code point with.
    #[test]
    fn a_typed_path_spells_back_the_text_its_row_held() {
        use cmdr_fs::app_path::Namer;
        let held = "/d/caf\u{10FEC3}\u{10FEA9}.txt";
        for namer in [Namer::Bytes, Namer::Units] {
            let typed = AppPathBuf::from_literal_store(held, namer).expect("an app path");
            assert_ne!(typed.as_str(), held, "{namer:?} escapes the literal code points");
            assert_eq!(typed.stored(), held, "{namer:?}");
            assert_eq!(stored_leaf(&typed).as_deref(), Some("caf\u{10FEC3}\u{10FEA9}.txt"));
        }
        let plain = AppPathBuf::from_wire("/d/a.txt").expect("an app path");
        assert!(matches!(plain.stored(), Cow::Borrowed("/d/a.txt")));
        assert_eq!(stored_split(&plain), ("/d".to_string(), "a.txt".to_string()));
    }
}
