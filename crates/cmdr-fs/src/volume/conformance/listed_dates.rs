//! What a volume's listings promise about dates: no entry carries a date its
//! volume doesn't claim through [`Volume::supported_dates`].

use crate::app_path::AppPath;
use std::path::Path;

use crate::volume::Volume;

/// No entry of `dir`'s listing, nor `get_metadata` of one, carries a creation
/// or added date that `volume.supported_dates(dir)` doesn't claim.
///
/// `dir` must already hold at least one entry, and the assertion checks that
/// first, so a fixture that seeded nothing can't pass by accident. Seed it the
/// way the backend fills dates (a real file on a real store), since what's
/// under test is the backend's own mapping.
///
/// **Why this one is worth a shared assertion.** A sort by Created or Added
/// orders rows only where the volume claims the date and falls back to
/// Modified elsewhere. A backend that fills a date it doesn't claim sorts by a
/// date the user never sees honored; one that claims less than it fills hides
/// a real date behind the fallback. The claim is a hand-written answer per
/// backend, and nothing but this keeps it in step with the mapping code.
pub async fn assert_listing_dates_are_claimed(volume: &dyn Volume, dir: &Path) {
    let listed = volume
        .list_directory(&AppPath::legacy_unchecked(dir), None)
        .await
        .unwrap_or_else(|e| panic!("{} must be listable, got {e:?}", dir.display()));
    assert!(
        !listed.is_empty(),
        "fixture precondition: {} must hold at least one entry",
        dir.display()
    );
    let claimed = volume.supported_dates(&AppPath::legacy_unchecked(dir)).await;

    for entry in &listed {
        let stat = volume
            .get_metadata(&AppPath::legacy_unchecked(Path::new(&entry.path)))
            .await
            .unwrap_or_else(|e| panic!("{} must be stattable, got {e:?}", entry.path));
        for (read, entry) in [("its folder's listing", entry), ("get_metadata", &stat)] {
            if let Some(created) = entry.created_at {
                assert!(
                    claimed.created,
                    "{} carries a creation date ({created}) in {read}, but supported_dates({}) claims none ({claimed:?})",
                    entry.path,
                    dir.display()
                );
            }
            if let Some(added) = entry.added_at {
                assert!(
                    claimed.added,
                    "{} carries an added date ({added}) in {read}, but supported_dates({}) claims none ({claimed:?})",
                    entry.path,
                    dir.display()
                );
            }
        }
    }
}
