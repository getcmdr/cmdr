//! Giving the folders a cross-volume copy created their source folders' dates,
//! modification AND creation, and whatever else the source folder carries
//! (`Volume::folder_metadata`: an archive folder's AppleDouble xattrs), once
//! everything inside them has landed.
//!
//! Writing a child bumps its folder's date on every real store, so a folder can
//! only keep its source's date if it's set AFTER its last child: the merge walk
//! notes each folder it created as it leaves it (post-order), and the copy
//! stamps the whole list once the subtree's leaves have drained. A subtree that
//! failed or was cancelled is never stamped: a folder holding part of its
//! contents must not claim the date of the whole. The dates come from the
//! listing entry the walk already holds (the scan's stat for a top-level
//! folder), so they cost no round trip; the rest is one `folder_metadata` ask
//! per folder, which every backend but the archive answers empty with no I/O.
//! `DETAILS.md` § "Copies keep the source's metadata".
//!
//! A folder that arrives without a date its source had (the destination can't
//! hold it, or refused it) is counted for the loss note
//! (`crates/cmdr-operations/src/metadata_losses.rs`). A share that drops
//! under a folder's stamp is waited for, as every transfer step is
//! (`../volume_outage.rs`), and the folder stamped again; only a refusal from a
//! share that stayed up is a loss.
//!
//! ❌ Not the extract quarantine: a folder gets that the moment the walk creates
//! it (`extract_quarantine.rs`), since this pass skips every folder of a
//! cancelled subtree. A quarantine a SIDECAR carries is replaced by the
//! archive's here, so this pass never undoes that one.

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::{CarriedMetadata, ClassOutcome, MetadataClasses};

use super::super::super::metadata_losses::FolderLosses;
use super::super::super::state::WriteOperationState;
use super::super::volume_outage::{OutageEnd, StartedOn, wait_after_shortfall};
use super::landing::std_of;
use cmdr_fs::volume::Volume;

/// A source folder's own dates (Unix seconds), as the listing entry or the
/// scan's stat reported them. `None` is a date the source didn't carry.
// DEFAULT-OK: both "the source carried no date", which leaves the folder the
// store's own; nothing reads it as a claim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SourceFolderDates {
    pub modified_at: Option<u64>,
    pub created_at: Option<u64>,
}

impl SourceFolderDates {
    /// What a listing entry says about its folder.
    pub(super) fn of_entry(entry: &cmdr_fs::entry::FileEntry) -> Self {
        Self {
            modified_at: entry.modified_at,
            created_at: entry.created_at,
        }
    }
}

/// One folder the walk created: where it landed, the source folder it came
/// from, and that folder's dates.
struct FilledFolder {
    dest: AppPathBuf,
    source: PathBuf,
    dates: SourceFolderDates,
}

/// The folders one top-level source's walk created, each with its source
/// folder, in the order they're stamped.
// DEFAULT-OK: an empty list is a walk that hasn't created a folder yet.
#[derive(Default)]
pub(super) struct FolderMetadata {
    /// In the order the walk LEFT them, which puts every folder after the
    /// folders inside it, and the top-level one last.
    folders: Vec<FilledFolder>,
}

impl FolderMetadata {
    /// A folder the walk created at `dest` from the source folder `source` and
    /// has just finished filling.
    pub(super) fn note_filled(&mut self, dest: AppPathBuf, source: PathBuf, dates: SourceFolderDates) {
        self.folders.push(FilledFolder { dest, source, dates });
    }

    /// Puts every noted folder's metadata on `dest_volume`, deepest first: its
    /// dates, and what `source_volume` says the source folder carries. Counts
    /// what each folder arrives without, of what it had worth keeping.
    ///
    /// Call it only once the subtree landed in full. Best effort: a refusal is
    /// a `log::warn!` and the copy stands; a destination that can hold none of
    /// it (`MetadataReport::holds_none`) gets asked no more, though every
    /// folder still counts what it lost.
    ///
    /// A share that drops under a folder's stamp is waited for, and the folder
    /// stamped again (see [`stamp_through_outages`]).
    pub(super) async fn stamp(
        self,
        source_volume: &Arc<dyn Volume>,
        dest_volume: &Arc<dyn Volume>,
        state: &Arc<WriteOperationState>,
    ) {
        let mut stamping = Stamping {
            asking: true,
            waiting: true,
        };
        for folder in self.folders {
            if state.work_stopped() {
                return;
            }
            let Some((carried, outcome)) =
                stamp_through_outages(source_volume, dest_volume, &folder, state, &mut stamping).await
            else {
                return;
            };
            stamping.asking = !matches!(outcome, Stamped::HeldNone);
            note_losses(dest_volume, &folder, &carried, outcome, state);
        }
    }
}

/// What one stamp pass has learned so far about its destination.
struct Stamping {
    /// Whether to ask the destination at all: `false` once it said it can hold
    /// none of a folder's metadata.
    asking: bool,
    /// Whether a share that drops is still worth waiting for: `false` once a
    /// wait gave up, so the folders after it don't each wait again.
    waiting: bool,
}

/// Reads what the source folder carries and stamps the folder with it, once
/// more after each outage that may have cut either off. `None` when the work
/// stopped during a wait.
///
/// Neither call has an error to classify: a backend reports a drop under the
/// stamp as a refused class, and under the source read as nothing carried. So
/// a stamp that came up short, or a source that was asked for something, asks
/// the volumes themselves ([`wait_after_shortfall`]): one that went away, or
/// came back on a newer session while the stamp ran, is waited for and the
/// folder stamped again. Safe to run twice: it sets the same values. A refusal
/// from a share that stayed up is a refusal, counted as a loss.
async fn stamp_through_outages(
    source_volume: &Arc<dyn Volume>,
    dest_volume: &Arc<dyn Volume>,
    folder: &FilledFolder,
    state: &Arc<WriteOperationState>,
    stamping: &mut Stamping,
) -> Option<(CarriedMetadata, Stamped)> {
    let volumes = [source_volume, dest_volume];
    let mut outages = 0;
    loop {
        let started_on = StartedOn::now(&volumes);
        let (carried, asked_the_source) = source_set(source_volume, dest_volume, folder, state).await;
        let outcome = if stamping.asking {
            stamp_one(dest_volume, folder, &carried).await
        } else {
            Stamped::NotAsked
        };
        if !stamping.waiting || !(asked_the_source || outcome.came_up_short()) {
            return Some((carried, outcome));
        }
        match wait_after_shortfall(state, &volumes, &started_on, &mut outages).await {
            OutageEnd::Back => {}
            OutageEnd::NotWaitable => return Some((carried, outcome)),
            OutageEnd::GaveUp { .. } => {
                stamping.waiting = false;
                return Some((carried, outcome));
            }
            OutageEnd::Cancelled => return None,
        }
    }
}

/// What stamping one folder did.
enum Stamped {
    /// The destination answered, class by class.
    Answered {
        created: ClassOutcome,
        modified: ClassOutcome,
        xattrs: ClassOutcome,
        flags: ClassOutcome,
    },
    /// The destination can hold none of a folder's metadata; ask it no more.
    HeldNone,
    /// An earlier folder's [`Stamped::HeldNone`] ended the asking.
    NotAsked,
}

impl Stamped {
    /// Whether the destination refused a class: a server's refusal, or a drop
    /// under the stamp, which a backend reports the same way.
    fn came_up_short(&self) -> bool {
        match self {
            Self::Answered {
                created,
                modified,
                xattrs,
                flags,
            } => [created, modified, xattrs, flags]
                .iter()
                .any(|outcome| **outcome == ClassOutcome::Refused),
            Self::HeldNone | Self::NotAsked => false,
        }
    }
}

/// What the source folder carries beyond its dates: its xattrs (an archive's
/// sidecar, a local folder's tags), asked only of a source that reports them,
/// and with the archive's quarantine in place of any a sidecar names; and its
/// flags (Hidden, Locked), asked only when the destination keeps a folder's,
/// since a share answers them with a round trip a loss count alone doesn't
/// earn. Also answers whether the source was asked at all.
async fn source_set(
    source_volume: &Arc<dyn Volume>,
    dest_volume: &Arc<dyn Volume>,
    folder: &FilledFolder,
    state: &WriteOperationState,
) -> (CarriedMetadata, bool) {
    let flags = dest_volume
        .metadata_caps()
        .folder_stores
        .intersection(MetadataClasses::FLAGS);
    let wanted = source_volume
        .metadata_caps()
        .reports
        .intersection(MetadataClasses::XATTRS | flags);
    if wanted.is_empty() {
        return (CarriedMetadata::default(), false);
    }
    let mut carried = source_volume
        .folder_metadata(&AppPath::legacy_unchecked(&folder.source), wanted)
        .await;
    super::extract_quarantine::keep_archive_quarantine(state, source_volume, &mut carried.xattrs);
    (carried, true)
}

/// Counts what the folder had worth keeping that it arrived without: what the
/// destination's caps say it can't hold, or what it refused.
fn note_losses(
    dest_volume: &Arc<dyn Volume>,
    folder: &FilledFolder,
    carried: &CarriedMetadata,
    outcome: Stamped,
    state: &WriteOperationState,
) {
    let caps = dest_volume.metadata_caps();
    let (created, modified, xattrs, flags) = match outcome {
        Stamped::Answered {
            created,
            modified,
            xattrs,
            flags,
        } => (created, modified, xattrs, flags),
        Stamped::HeldNone | Stamped::NotAsked => (
            ClassOutcome::NotApplicable,
            ClassOutcome::NotApplicable,
            ClassOutcome::NotApplicable,
            ClassOutcome::NotApplicable,
        ),
    };
    let source = folder.dates;
    let created_worth_keeping = match (source.created_at, source.modified_at) {
        (Some(created), Some(modified)) => created < modified,
        (Some(_), None) => true,
        (None, _) => false,
    };
    // The same "worth keeping" a file's xattrs answer: any but the quarantine.
    let xattrs_worth_keeping = carried.worth_keeping(None, 0).contains(MetadataClasses::XATTRS);
    state.metadata_losses.note_folder(FolderLosses {
        created: created_worth_keeping
            && (!caps.folder_stores.contains(MetadataClasses::CREATED) || created == ClassOutcome::Refused),
        modified: source.modified_at.is_some() && (!caps.folder_dates || modified == ClassOutcome::Refused),
        xattrs: xattrs_worth_keeping
            && (!caps.folder_stores.contains(MetadataClasses::XATTRS) || xattrs == ClassOutcome::Refused),
        // Asked only of a destination that keeps them (`source_set`), so only
        // a refusal is a loss here.
        flags: !carried.flags.is_empty() && flags == ClassOutcome::Refused,
    });
}

/// Stamps one folder with its dates and `carried`.
async fn stamp_one(dest_volume: &Arc<dyn Volume>, folder: &FilledFolder, carried: &CarriedMetadata) -> Stamped {
    let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs);
    let carried = CarriedMetadata {
        created: folder.dates.created_at.map(at),
        ..carried.clone()
    };
    let modified = folder.dates.modified_at.map(at);
    if carried.is_empty() && modified.is_none() {
        // A source that carried nothing at all (an S3 prefix) leaves the
        // folder the store's own, and costs the destination nothing.
        return Stamped::Answered {
            created: ClassOutcome::Absent,
            modified: ClassOutcome::Absent,
            xattrs: ClassOutcome::Absent,
            flags: ClassOutcome::Absent,
        };
    }
    let dest: &AppPath = &folder.dest;
    let report = dest_volume.apply_metadata(dest, &carried, modified).await;
    if report.holds_none() {
        return Stamped::HeldNone;
    }
    if report.any_refused() {
        log::warn!(
            target: "transfer",
            "folder metadata: {} keeps some of the copy's own, the destination refused the source's: {report:?}",
            LogPrivate::dir_path(&*std_of(dest))
        );
    }
    Stamped::Answered {
        created: report.created,
        modified: report.modified,
        xattrs: report.xattrs,
        flags: report.flags,
    }
}
