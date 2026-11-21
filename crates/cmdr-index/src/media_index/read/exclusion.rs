//! Holding a folder exclusion at READ time.
//!
//! Excluding a folder vetoes its enrichment and purges its stored rows, but a read can't
//! lean on either: the purge can fail to land (`scheduler/purge.rs` keeps retrying it), a
//! folder excluded while its NAS was offline is only purged on reconnect, and a row can
//! commit in the moment between the veto and the delete. So every read that can return an
//! image's path, text, tag, score, or facts asks [`ReadExclusion::hides`] first. Ask Cmdr's
//! `search_photos` and `image_facts` tools read through `MediaIndex` too, which is what
//! keeps an excluded folder's OCR text away from a cloud model.
//!
//! The predicate is the veto's own ([`NetworkEnrichConfig::is_excluded`] over the OS path,
//! which folds names the way the platform's filesystem does), so reading and indexing can't
//! disagree about what's excluded. Exclusions are OS-path keyed while a network volume stores
//! index-relative paths, so a read places the volume's rows at EVERY root it's known by: the
//! live mount root while it's mounted, plus each root a pass recorded in `media.db`. That
//! keeps an unmounted NAS's excluded folders hidden, and keeps a folder excluded under a
//! share's old mount name hidden after the share remounts under a new one. A volume with no
//! known root can't place its rows, so while any folder is excluded it shows nothing, until
//! its next pass records the root.

use std::path::Path;

use crate::media_index::network::config::{self, NetworkEnrichConfig};
use crate::media_index::network::fetch::os_join;
use crate::media_index::store;

/// Where a volume's stored paths sit in OS space, for matching them against exclusions.
enum Placement {
    /// No folder is excluded: nothing to place, and nothing is hidden.
    NothingExcluded,
    /// Every mount root the volume's index-relative rows join onto (`/` for a local volume).
    /// A row is hidden when a folder excludes it at ANY of them.
    Roots(Vec<String>),
    /// Folders are excluded, but nothing knows where this volume's rows sit.
    Unplaceable,
}

/// One read's view of the exclusion: a snapshot of the config plus where the volume's rows
/// sit. Built once per read, so every row of one answer is judged against the same state.
pub(super) struct ReadExclusion {
    config: NetworkEnrichConfig,
    placement: Placement,
}

impl ReadExclusion {
    /// Resolve the exclusion for the volume behind `db_path`: the roots its passes recorded,
    /// plus the LIVE mount root when `volume_id` names a mounted volume. Costs nothing past
    /// the config snapshot while no folder is excluded.
    pub(super) fn resolve(db_path: &Path, volume_id: Option<&str>) -> Self {
        let config = config::snapshot();
        let placement = if config.excluded_folders.is_empty() {
            Placement::NothingExcluded
        } else {
            let mut roots = store::read_mount_roots(db_path);
            let live = volume_id
                .and_then(|id| crate::indexing::host::volumes::current().get(id))
                .map(|volume| crate::indexing::paths::app_text::stored_text(volume.root()).into_owned());
            if let Some(live) = live
                && !roots.contains(&live)
            {
                roots.push(live);
            }
            if roots.is_empty() {
                Placement::Unplaceable
            } else {
                Placement::Roots(roots)
            }
        };
        Self { config, placement }
    }

    /// Whether this read can hide anything at all, so a caller skips work it only needs
    /// while a folder is excluded.
    pub(super) fn is_active(&self) -> bool {
        !matches!(self.placement, Placement::NothingExcluded)
    }

    /// Whether the stored image at `index_path` sits under an excluded folder at any of the
    /// volume's roots, and so must not surface. An unplaceable volume hides every row.
    pub(super) fn hides(&self, index_path: &str) -> bool {
        match &self.placement {
            Placement::NothingExcluded => false,
            Placement::Roots(roots) => roots
                .iter()
                .any(|root| self.config.is_excluded(&os_join(root, index_path))),
            Placement::Unplaceable => true,
        }
    }
}
