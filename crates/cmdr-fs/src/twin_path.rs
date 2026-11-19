//! Row paths for same-named siblings, and the path a person reads for one.
//!
//! A device can hold two items with one name in a folder (MTP, #523), so a
//! name alone can't say which one a row means. Such a row's path carries a PICK
//! segment right before the shared name: NUL plus an id the backend chose
//! (`/DCIM/␀4242/IMG_1.JPG`). The leaf stays the real name. NUL because no
//! filesystem or device name can hold one, so a pick never collides with a
//! folder that exists. The backend that mints picks (`cmdr_mtp`'s `twins.rs`)
//! owns what the id means; everything here is backend-blind.
//!
//! Two jobs live here so they can't drift:
//!
//! - **The folder holding a path** steps over a pick ([`folder_of`]), ❌ never
//!   `Path::parent`, which would answer the pick itself.
//! - **The path a person reads** has every pick taken out ([`shown`]). ❌ A pick
//!   never reaches a person: every place that turns a path (or text holding one)
//!   into something shown, copied, or logged for reading goes through here. The
//!   frontend's twin is `apps/desktop/src/lib/utils/display-path.ts`.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::app_path::{AppName, AppPath, PathArithmetic};

/// What starts a pick segment.
pub const PICK: char = '\0';

/// The pick segment naming id `id`.
pub fn pick_segment(id: u64) -> String {
    format!("{PICK}{id}")
}

/// The id `segment` picks, or `None` for an ordinary segment.
pub fn pick_id(segment: &OsStr) -> Option<u64> {
    segment.to_str()?.strip_prefix(PICK)?.parse().ok()
}

fn is_pick(segment: &OsStr) -> bool {
    segment.as_encoded_bytes().first() == Some(&0)
}

/// The folder holding `path`'s item: its parent, stepping over a pick. `None`
/// for a root, like [`Path::parent`].
pub fn folder_of(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    if parent.file_name().is_some_and(is_pick) {
        parent.parent()
    } else {
        Some(parent)
    }
}

/// The id `path` picks for its own item, when it's a twin's row path. A pick
/// further up names an ancestor, which this doesn't report.
pub fn own_pick(path: &Path) -> Option<u64> {
    path.parent().and_then(Path::file_name).and_then(pick_id)
}

/// `path` without its own pick: the path every one of its twins shares, and
/// the spelling a NEW name in that folder takes. An ordinary path comes back
/// as it is.
pub fn unpicked(path: &Path) -> Cow<'_, Path> {
    match (own_pick(path), folder_of(path), path.file_name()) {
        (Some(_), Some(folder), Some(name)) => Cow::Owned(folder.join(name)),
        _ => Cow::Borrowed(path),
    }
}

/// The id `name` picks, or `None` for an ordinary name: [`pick_id`] for an app path's name.
pub fn pick_id_of_name(name: &str) -> Option<u64> {
    name.strip_prefix(PICK)?.parse().ok()
}

/// [`folder_of`] for an app path: its parent, stepping over a pick, and `None` at its
/// volume's root (a pick right under a scheme root steps back to that root, never above
/// it).
pub fn folder_of_app(path: &AppPath) -> Option<&AppPath> {
    let parent = path.parent()?;
    if parent.file_name().is_some_and(|name| name.starts_with(PICK)) {
        parent.parent()
    } else {
        Some(parent)
    }
}

/// [`own_pick`] for an app path.
pub fn own_pick_app(path: &AppPath) -> Option<u64> {
    path.parent()?.file_name().and_then(pick_id_of_name)
}

/// [`unpicked`] for an app path.
pub fn unpicked_app(path: &AppPath) -> Cow<'_, AppPath> {
    if own_pick_app(path).is_none() {
        return Cow::Borrowed(path);
    }
    let joined = folder_of_app(path)
        .zip(path.file_name())
        .and_then(|(folder, name)| folder.join(AppName::from_app_spelling(name).ok()?).ok());
    // A name already in a canonical path joins back onto its folder; a bridge value that
    // doesn't comes back as it is, as `unpicked` leaves a path it can't split.
    joined.map_or(Cow::Borrowed(path), Cow::Owned)
}

/// `text` as a person reads it: every pick segment taken out, with the `/`
/// that joined it, wherever in the text a path holding one sits. A pick is
/// exactly what [`pick_segment`] writes, NUL plus a decimal id, ending at a `/`
/// or the end of the text; any other NUL is left as it is (the frontend splits
/// a translated phrase on a bare one). Text with no NUL comes back borrowed. A
/// path, a message, a log line, or a whole response all go through this rule.
pub fn shown(text: &str) -> Cow<'_, str> {
    if !text.contains(PICK) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(PICK) {
        let (before, from_pick) = rest.split_at(start);
        let digits = from_pick[1..].bytes().take_while(u8::is_ascii_digit).count();
        let end = 1 + digits;
        if digits == 0 || !(end == from_pick.len() || from_pick[end..].starts_with('/')) {
            out.push_str(&rest[..=start]);
            rest = &from_pick[1..];
            continue;
        }
        match before.strip_suffix('/') {
            // `…/␀4242/name` and a trailing `…/␀4242` both lose the slash before.
            Some(head) => {
                out.push_str(head);
                rest = &from_pick[end..];
            }
            // A pick that starts the text (a relative row path) loses the one after.
            None => {
                out.push_str(before);
                rest = from_pick[end..].strip_prefix('/').unwrap_or(&from_pick[end..]);
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// [`shown`] for a path.
pub fn shown_path(path: &Path) -> Cow<'_, Path> {
    match path.to_str().map(shown) {
        Some(Cow::Owned(plain)) => Cow::Owned(PathBuf::from(plain)),
        _ => Cow::Borrowed(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(folder: &str, id: u64, name: &str) -> PathBuf {
        Path::new(folder).join(pick_segment(id)).join(name)
    }

    #[test]
    fn a_twin_row_keeps_its_name_and_its_folder() {
        let twin = row("/DCIM", 42, "IMG_1.JPG");

        assert_eq!(twin.file_name(), Some(OsStr::new("IMG_1.JPG")));
        assert_eq!(folder_of(&twin), Some(Path::new("/DCIM")));
        assert_eq!(own_pick(&twin), Some(42));
        assert_eq!(unpicked(&twin), Path::new("/DCIM/IMG_1.JPG"));
        assert_eq!(unpicked(&row("/DCIM", 42, "new.jpg")), Path::new("/DCIM/new.jpg"));
    }

    #[test]
    fn a_row_inside_a_twin_folder_is_an_ordinary_child_of_that_folder() {
        let twin_folder = row("/DCIM", 7, "Trip");
        let child = twin_folder.join("a.jpg");

        assert_eq!(folder_of(&child), Some(twin_folder.as_path()));
        assert_eq!(own_pick(&child), None, "the pick names the folder, not the child");
        assert_eq!(unpicked(&child), child.as_path());
    }

    #[test]
    fn an_ordinary_path_has_no_pick() {
        assert_eq!(folder_of(Path::new("/DCIM/IMG_1.JPG")), Some(Path::new("/DCIM")));
        assert_eq!(folder_of(Path::new("/IMG_1.JPG")), Some(Path::new("/")));
        assert_eq!(folder_of(Path::new("/")), None);
        assert_eq!(own_pick(Path::new("/DCIM/IMG_1.JPG")), None);
    }

    /// A row path as an app path, built by the type's own join on every OS.
    fn app_row(folder: &str, id: u64, name: &str) -> crate::app_path::AppPathBuf {
        let pick = pick_segment(id);
        let named = |text: &str| {
            AppName::from_app_spelling(text)
                .map(ToOwned::to_owned)
                .expect("a plain name")
        };
        AppPath::from_normalized(folder)
            .expect("a canonical folder")
            .join(&named(&pick))
            .and_then(|picked| picked.join(&named(name)))
            .expect("joins")
    }

    #[test]
    fn a_shown_path_has_every_pick_taken_out() {
        let shown_of = |path: &AppPath| shown(path.as_str()).into_owned();
        let in_trip = app_row("/DCIM", 7, "Trip").join(AppName::from_app_spelling("a.jpg").expect("a name"));

        assert_eq!(shown_of(&app_row("/DCIM", 42, "IMG_1.JPG")), "/DCIM/IMG_1.JPG");
        assert_eq!(shown_of(&in_trip.expect("joins")), "/DCIM/Trip/a.jpg");
        assert_eq!(shown_of(&app_row("/", 3, "x")), "/x");
        assert_eq!(
            shown(&format!("{}/IMG_1.JPG", pick_segment(3))),
            "IMG_1.JPG",
            "a relative row path"
        );
        assert_eq!(
            shown(&format!("/DCIM/{}", pick_segment(9))),
            "/DCIM",
            "a pick at the end"
        );
        assert_eq!(
            shown_path(Path::new(&format!("mtp://dev/65537/DCIM/{}/a", pick_segment(1)))),
            Path::new("mtp://dev/65537/DCIM/a")
        );
        assert!(matches!(shown("/DCIM/IMG_1.JPG"), Cow::Borrowed(_)));
    }

    #[test]
    fn text_holding_twin_paths_reads_with_the_plain_paths() {
        let first = app_row("/DCIM", 1, "a.jpg");
        let second = app_row("/DCIM", 2, "a.jpg");
        let text = format!("“{first}” and “{second}” clash");

        assert_eq!(shown(&text), "“/DCIM/a.jpg” and “/DCIM/a.jpg” clash");
        assert!(!shown(&text).contains(PICK));
    }

    /// The typed twins answer what the `Path` forms answer, on every root shape and every
    /// OS, and never climb above a volume's root.
    #[test]
    fn the_typed_twins_find_the_folder_the_pick_and_the_shared_path() {
        for folder in [
            "/DCIM",
            "mtp://dev/65537",
            "mtp://dev/65537/DCIM",
            "C:/DCIM",
            "//nas/share/x",
        ] {
            let flavor = if folder.starts_with('/') && !folder.starts_with("//") {
                crate::app_path::PathFlavor::Posix
            } else {
                crate::app_path::PathFlavor::Windows
            };
            let folder_path = AppPath::from_normalized_as(folder, flavor).expect("canonical");
            let pick = AppName::from_app_spelling(&pick_segment(42))
                .expect("a pick is a name")
                .to_owned();
            let name = AppName::from_app_spelling("IMG_1.JPG").expect("a name").to_owned();
            let twin = folder_path.join(&pick).and_then(|p| p.join(&name)).expect("joins");
            assert_eq!(folder_of_app(&twin), Some(folder_path), "{folder}");
            assert_eq!(own_pick_app(&twin), Some(42), "{folder}");
            let plain = folder_path.join(&name).expect("joins");
            assert_eq!(unpicked_app(&twin).as_str(), plain.as_str(), "{folder}");
            assert_eq!(own_pick_app(&plain), None, "{folder}");
            assert!(matches!(unpicked_app(&plain), Cow::Borrowed(_)), "{folder}");
        }
        let under_root = AppPath::from_normalized(&format!("mtp://dev/{}/IMG_1.JPG", pick_segment(4)))
            .expect("canonical")
            .to_owned();
        assert_eq!(
            folder_of_app(&under_root).map(AppPath::as_str),
            Some("mtp://dev/"),
            "a pick right under the root steps back to the root, ❌ never above it"
        );
        assert_eq!(
            folder_of_app(AppPath::from_normalized("mtp://dev/").expect("a root")),
            None
        );
    }

    #[test]
    fn a_nul_that_is_no_pick_stays() {
        assert_eq!(shown("modified \0"), "modified \0", "a bare NUL");
        assert_eq!(shown("a\0b"), "a\0b", "no id");
        assert_eq!(shown("/x/\u{0}12ab/y"), "/x/\u{0}12ab/y", "an id that runs into a name");
    }
}
