//! Whether, and which, symlinks a COPY lands as symlinks on the destination.
//!
//! One value per copy, built before the scan: it answers for the sources the
//! person selected (their targets are read here, so the scan can count each as
//! the one leaf it is) and for the links a merge walk meets later. `DETAILS.md`
//! § "A copy carries a link as the link".
//!
//! A destination proves it can hold a link once per operation, on a tracked
//! temp, and only when a link actually turned up: the backend's
//! `Volume::can_hold_links` can't speak for the filesystem under it (exFAT on a
//! local volume), and that has to be known BEFORE a conflict is resolved, because
//! a link and what it points at clash differently.
//!
//! A move never builds one (`MergeCtx::links` stays `None`): it follows a link
//! on purpose (`../DETAILS.md` § "Symlinks are opaque to a move").

use cmdr_fs::app_path::AppPath;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream;
use tokio::sync::OnceCell;

use super::super::super::state::{ScanResult, WriteOperationState};
use super::super::staged_write::{StagedWrite, WriteStaging};
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{Volume, VolumeError};

/// How many selected sources the preflight reads a link target from at once.
const SELECTED_READS_AT_ONCE: usize = 16;

/// What one copy does with the symlinks it meets.
// DEFAULT-OK: the default carries nothing, which is what every link did before
// this existed and what a move still does.
#[derive(Default)]
pub(super) struct LinkCarry {
    /// `false` when nothing can come of asking: a destination backend with no
    /// links, or a copy inside one volume, whose behavior this leaves alone.
    possible: bool,
    /// Where the destination proves itself, once a link turns up.
    dest_root: PathBuf,
    /// The destination's answer, asked at most once per operation.
    holds_links: OnceCell<bool>,
    /// The selected sources that are links, with their targets.
    selected: HashMap<PathBuf, PathBuf>,
}

impl LinkCarry {
    /// What this copy carries, reading the link target of each selected source.
    ///
    /// A source whose target can't be read is left out: it takes the ordinary
    /// route, whose scan and copy report a real failure with its own wording.
    pub(super) async fn for_copy(
        state: &Arc<WriteOperationState>,
        source_volume: &Arc<dyn Volume>,
        source_paths: &[PathBuf],
        dest_volume: &Arc<dyn Volume>,
        dest_root: &Path,
    ) -> Self {
        let mut carry = Self {
            possible: !Arc::ptr_eq(source_volume, dest_volume) && dest_volume.can_hold_links(),
            dest_root: dest_root.to_path_buf(),
            ..Self::default()
        };
        if !carry.possible {
            return carry;
        }
        let read: Vec<Option<(PathBuf, PathBuf)>> = stream::iter(source_paths.to_vec())
            .map(|path| {
                let source_volume = Arc::clone(source_volume);
                async move {
                    let target = source_volume
                        .read_link_target(&AppPath::legacy_unchecked(&path))
                        .await
                        .ok()
                        .flatten();
                    target.map(|target| (path, target))
                }
            })
            .buffer_unordered(SELECTED_READS_AT_ONCE.min(source_volume.max_concurrent_ops().max(1)))
            .collect()
            .await;
        let selected: HashMap<PathBuf, PathBuf> = read.into_iter().flatten().collect();
        if !selected.is_empty() && carry.destination_holds_links(state, dest_volume).await {
            carry.selected = selected;
        }
        carry
    }

    /// The target of a SELECTED source this copy lands as a link.
    pub(super) fn selected_target(&self, source: &Path) -> Option<&Path> {
        self.selected.get(source).map(PathBuf::as_path)
    }

    /// The selected sources this copy lands as links.
    pub(super) fn selected(&self) -> impl Iterator<Item = &PathBuf> {
        self.selected.keys()
    }

    /// The target to land for a link a walk LISTED, or `None` when this link
    /// takes the older route: a source that can't say, or a destination that
    /// can't hold it. `Err` is the source failing to answer about this item.
    pub(super) async fn target_of_listed(
        &self,
        state: &Arc<WriteOperationState>,
        source_volume: &Arc<dyn Volume>,
        dest_volume: &Arc<dyn Volume>,
        link: &Path,
    ) -> Result<Option<PathBuf>, VolumeError> {
        if !self.possible {
            return Ok(None);
        }
        // The source first, so a backend with no answer costs the destination
        // no probe.
        let target = match source_volume.read_link_target(&AppPath::legacy_unchecked(link)).await {
            Ok(target) => target,
            Err(VolumeError::NotSupported) => None,
            Err(e) => return Err(e),
        };
        match target {
            Some(target) if self.destination_holds_links(state, dest_volume).await => Ok(Some(target)),
            _ => Ok(None),
        }
    }

    /// Whether a cached preview describes a different transfer from this one:
    /// a selected link counted as one leaf that this copy follows, or one this
    /// copy lands as a link counted as what it points at. Either way the source
    /// is scanned afresh (`preflight.rs`).
    pub(super) fn disagrees_with_preview(&self, cached: &ScanResult, source_paths: &[PathBuf]) -> bool {
        let selected: HashSet<&Path> = source_paths.iter().map(PathBuf::as_path).collect();
        let counted_as_links: HashSet<&Path> = cached
            .files
            .iter()
            .filter(|file| file.is_symlink && selected.contains(file.path.as_path()))
            .map(|file| file.path.as_path())
            .collect();
        counted_as_links.iter().any(|link| !self.selected.contains_key(*link))
            || self
                .selected
                .keys()
                .any(|link| !counted_as_links.contains(link.as_path()))
    }

    async fn destination_holds_links(&self, state: &Arc<WriteOperationState>, dest_volume: &Arc<dyn Volume>) -> bool {
        *self
            .holds_links
            .get_or_init(|| prove_it_holds_links(state, dest_volume, &self.dest_root))
            .await
    }
}

/// Makes one link in `folder` on a tracked temp and takes it away again, so a
/// crash in between leaves what any staged write leaves: a `.cmdr-tmp-*` name
/// the sweep clears. Any refusal counts as "no": one that's about the folder
/// rather than links meets the real writes too, with the item's own name on it.
async fn prove_it_holds_links(state: &Arc<WriteOperationState>, dest_volume: &Arc<dyn Volume>, folder: &Path) -> bool {
    let final_name = folder.join("link-probe");
    let announced = state.scope().host().announce_on(&**dest_volume, &final_name);
    let staged = StagedWrite::begin(state, &**dest_volume, announced, WriteStaging::Stage);
    let made = dest_volume
        .create_symlink(&AppPath::legacy_unchecked(staged.target()), Path::new("."))
        .await;
    staged.abandon(dest_volume).await;
    if let Err(e) = &made {
        log::debug!(
            target: "copy",
            "link_carry: {} can't hold links here, so they take the older route: {}",
            LogPrivate::volume(dest_volume.name()),
            LogError(e),
        );
    }
    made.is_ok()
}

#[cfg(test)]
#[path = "link_carry_tests.rs"]
mod tests;
