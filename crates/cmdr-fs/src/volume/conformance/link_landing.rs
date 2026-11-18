//! The promise a backend with `can_hold_links` makes to the copy engine.

use crate::app_path::AppPath;
use std::path::Path;

use crate::volume::{EntryKind, Volume};

/// A link's target lands byte for byte, reads back the same, and survives the
/// staged landing a copy gives it.
///
/// The engine creates the link on a `.cmdr-tmp-*` name and renames it into
/// place, so a backend that resolved the target against the volume, refused one
/// pointing out of it, or followed the link in its rename would land something
/// other than what the backup or source holds. `dir` is an empty, writable
/// folder. The target deliberately points at nothing and out of `dir`: a link is
/// never checked.
pub async fn assert_a_link_lands_verbatim(volume: &dyn Volume, dir: &Path) {
    let name = volume.name().to_string();
    assert!(volume.can_hold_links(), "{name} is expected to hold links");
    let target = Path::new("../../outside/of/this/volume");
    let (temp, landed, file) = (dir.join("temp"), dir.join("landed"), dir.join("file"));

    volume
        .create_symlink(&AppPath::legacy_unchecked(&temp), target)
        .await
        .unwrap_or_else(|e| panic!("create_symlink on {name}: {e:?}"));
    assert_eq!(
        volume
            .read_link_target(&AppPath::legacy_unchecked(&temp))
            .await
            .expect("the volume answers")
            .as_deref(),
        Some(target),
        "{name} must read back the target as stored"
    );
    assert_eq!(
        volume
            .entry_kind(&AppPath::legacy_unchecked(&temp))
            .await
            .expect("the volume answers"),
        EntryKind::Symlink,
        "{name} must report the link as the link"
    );

    assert!(
        volume
            .create_symlink(&AppPath::legacy_unchecked(&temp), Path::new("other"))
            .await
            .is_err(),
        "{name} must refuse an occupied name"
    );
    assert_eq!(
        volume
            .read_link_target(&AppPath::legacy_unchecked(&temp))
            .await
            .expect("the volume answers")
            .as_deref(),
        Some(target),
        "{name}'s refused create must leave what was there"
    );

    volume
        .rename(
            &AppPath::legacy_unchecked(&temp),
            &AppPath::legacy_unchecked(&landed),
            false,
        )
        .await
        .unwrap_or_else(|e| panic!("landing the link on {name}: {e:?}"));
    assert_eq!(
        volume
            .read_link_target(&AppPath::legacy_unchecked(&landed))
            .await
            .expect("the volume answers")
            .as_deref(),
        Some(target),
        "{name}'s rename must carry the link itself"
    );

    volume
        .create_file(&AppPath::legacy_unchecked(&file), b"bytes")
        .await
        .expect("the volume answers");
    assert_eq!(
        volume
            .read_link_target(&AppPath::legacy_unchecked(&file))
            .await
            .expect("the volume answers"),
        None,
        "{name} must say a file is no link"
    );

    volume
        .delete(&AppPath::legacy_unchecked(&landed))
        .await
        .unwrap_or_else(|e| panic!("deleting the link on {name}: {e:?}"));
    volume
        .delete(&AppPath::legacy_unchecked(&file))
        .await
        .expect("the volume answers");
}
