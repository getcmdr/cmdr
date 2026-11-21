//! The destination an Overwrite is replacing, held aside until whatever replaces
//! it has landed.
//!
//! Three shapes reach it. A same-volume move replaces by RENAMING, so the new
//! bytes need no temp and the only thing at risk is the entry being replaced;
//! each such aside settles the moment its one rename answers. A file→file
//! Overwrite's landing (`WriteStaging::StageOverOriginal`, on a destination
//! without a one-step replace) sets the original aside for the staged temp to
//! take its name, and settles it the moment the temp lands
//! ([`DisplacedDestination::hold_for_swap`], driven by
//! `staged_write_landing.rs`). A cross-type Overwrite (a folder landing on a
//! file, or a file on a folder) lands leaf by leaf over the rest of the
//! operation, so those asides go to the operation's [`DisplacedLedger`] and
//! settle when it ends. The local-FS twin is
//! `crates/cmdr-operations/src/overwrite.rs::DisplacedEntry`.

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::super::super::in_flight_temps::{self, ItemKind, NotOnDisk, RecordHome, TrackedRecord};
use super::super::super::state::WriteOperationState;
use super::super::super::types::RecoveredOriginal;
use super::super::recovered_name::rescue_out_of_temp_space;
use super::super::volume_outage::{OutageEnd, OutageFailure, StartedOn, lost_connection, wait_after_failure};
use super::cleanup::{TreeRemoval, remove_tree};
use super::transfer_error::{PathRole, PathedVolumeError};
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::staging::StagingTemp;
use cmdr_fs::volume::{Volume, VolumeError};

/// The destination an Overwrite is about to replace, renamed out of the way
/// rather than deleted.
///
/// A same-volume move replaces by RENAMING the source onto the name, and a
/// rename can't clobber (`force = false`, and MTP's `force = true` doesn't
/// delete an existing dest either), so the name has to be free first. Deleting
/// to free it is what makes the gap fatal: the replacing rename is a separate
/// call, and an SMB `STATUS_SHARING_VIOLATION`, an MTP `MoveObject` refusal, or
/// a session blip fails it after the delete has already succeeded — the
/// destination gone, the source not moved. One rename aside costs one call on
/// every backend and turns that into a restore. A cross-type Overwrite has the
/// same gap, only wider: the folder replacing a file streams in over many calls,
/// any of which can fail or be cancelled.
///
/// The guard rides along so the aside stays hidden from the pane for exactly as
/// long as it is on disk. ❗ It wears the `.cmdr-temp-` marker, which
/// `cleanup.rs::reap_stale_transfer_temps` does NOT match (it reaps `.cmdr-tmp-`
/// only), so an aside nothing could put back survives for the user to find.
#[must_use = "an aside nobody settles stays under its scratch name until the next launch's sweep"]
pub(crate) struct DisplacedDestination {
    aside: StagingTemp,
    original: PathBuf,
    /// The two paths in the volume's own spelling, made once ([`Self::new`]): every
    /// volume call here addresses one of them.
    aside_app: AppPathBuf,
    original_app: AppPathBuf,
    /// Whether it's a FOLDER, which only a cross-type Overwrite sets aside and
    /// which [`DisplacedDestination::discard`] has to take down as a tree.
    is_directory: bool,
    /// The ledger's record of it, so a force-quit between the rename aside and
    /// the rename that replaces it doesn't leave the user's file wearing a
    /// scratch name with nothing that knows what it is. `None` for an operation
    /// that never named its destination volume, which production always does.
    record: Option<TrackedRecord>,
    state: Arc<WriteOperationState>,
}

/// Hand-rolled: the operation's state has no `Debug`, and the two paths are the
/// whole story.
impl std::fmt::Debug for DisplacedDestination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplacedDestination")
            .field("aside", &self.aside.path())
            .field("original", &self.original)
            .field("is_directory", &self.is_directory)
            .finish()
    }
}

/// Renames whatever is at `original` to a `.cmdr-temp-<uuid>` sibling, so the
/// name is free for whatever replaces it. `is_directory` is what the caller
/// knows sits there.
///
/// `Ok(None)` ⇒ nothing was there, so there is nothing to put back.
///
/// ❗ The record is VOLUME-SPACE homed, ❌ never worked out from the path: these
/// paths are the destination volume's own, and a direct SMB session's `/photos`
/// is not this Mac's. `RecordHome::volume_space` is what keeps the sweep's
/// `std::fs` off them.
///
/// ❗ **Classify before acting.** A rename that fails on the transport
/// ([`VolumeError::transport_fault`]) may have been performed by the server, so
/// the user's file may already wear the aside's name: the record stays, the
/// share is waited for, and one stat of the aside settles what happened (there:
/// set aside; gone: run the rename again). A wait that ends without the share
/// settles what it can (`settle_unfinished_aside`) and leaves the record to
/// the share's next arrival otherwise, whose sweep puts the file back. Retiring
/// it on the error left the user's file under a scratch name nothing knew.
pub(crate) async fn displace_destination(
    state: &Arc<WriteOperationState>,
    volume: &Arc<dyn Volume>,
    original: &Path,
    is_directory: bool,
) -> Result<Option<DisplacedDestination>, PathedVolumeError> {
    // A folder aside goes as a tree once its replacement lands
    // ([`DisplacedDestination::discard`]), so one with a drive mounted inside it
    // is refused before it's set aside.
    if is_directory {
        super::super::super::mount_boundary::refuse_a_volume_mount_inside(&**volume, original, state)
            .await
            .map_err(|refusal| PathedVolumeError::refused(original, refusal))?;
    }
    let aside = StagingTemp::mint_aside(original, uuid::Uuid::new_v4(), state.liveness_token());
    // On disk before the rename below puts the original here (`track_in`). The
    // rename goes ahead whether or not the line made it.
    let record = match state.dest_volume_id() {
        Some(volume_id) => Some(
            in_flight_temps::track_in(
                state,
                RecordHome::volume_space(volume_id),
                ItemKind::VolumeAside {
                    destination: original.to_path_buf(),
                },
                aside.path(),
            )
            .await
            .0,
        ),
        None => None,
    };
    let displaced = DisplacedDestination::new(aside, original, is_directory, record, state);
    let volumes = [volume];
    let mut outages = 0;
    loop {
        let started_on = StartedOn::now(&volumes);
        let mut error = match volume
            .rename(&displaced.original_app, &displaced.aside_app, false)
            .await
        {
            Ok(()) => return Ok(Some(displaced)),
            Err(VolumeError::NotFound(_)) => {
                displaced.forget();
                return Ok(None);
            }
            Err(refused) if refused.transport_fault().is_none() => {
                displaced.forget();
                return Err(PathedVolumeError::new(PathRole::Destination, original, refused));
            }
            Err(cut_off) => cut_off,
        };
        let mut started_on = started_on;
        // The server may have performed it: wait, then ask the aside.
        let ended = loop {
            match wait_after_failure(state, &error, &volumes, &started_on, &mut outages).await {
                OutageEnd::Back => {}
                ended => break ended,
            }
            started_on = StartedOn::now(&volumes);
            match volume.get_metadata(&displaced.aside_app).await {
                Ok(_) => return Ok(Some(displaced)),
                Err(VolumeError::NotFound(_)) => break OutageEnd::Back,
                Err(stat) if stat.transport_fault().is_some() => error = stat,
                // The share answers but won't say: nothing proves the rename
                // either way, so it ends as a give-up would.
                Err(stat) => {
                    error = stat;
                    break OutageEnd::NotWaitable;
                }
            }
        };
        if ended == OutageEnd::Back {
            // The aside isn't there, so the rename never happened: run it again.
            continue;
        }
        settle_unfinished_aside(displaced, volume).await;
        let failure = PathedVolumeError::new(PathRole::Destination, original, error);
        return Err(match ended {
            OutageEnd::Cancelled => failure.into_cancelled(),
            _ => {
                let lost = lost_connection(ended, failure.volume_error(), &volumes);
                failure.with_lost_connection(lost)
            }
        });
    }
}

/// A rename aside the transport cut off, whose wait ended without settling it:
/// one stat decides what's left to do. Gone: it never happened, so nothing is
/// claimed. There: the user's file is on the aside with nothing replacing it,
/// so it goes back home now. Can't tell (the share is still out): the record
/// waits for the share's next arrival, whose sweep does the same.
async fn settle_unfinished_aside(displaced: DisplacedDestination, volume: &Arc<dyn Volume>) {
    match volume.get_metadata(&displaced.aside_app).await {
        Err(VolumeError::NotFound(_)) => displaced.forget(),
        Ok(_) => {
            if let Some(kept_at) = displaced.restore(volume).await {
                log::warn!(
                    target: "copy",
                    "an Overwrite's aside couldn't go back to its name, so it's kept at {}",
                    LogPrivate::file_path(&kept_at)
                );
            }
        }
        Err(_) => displaced.keep_for_arrival(),
    }
}

/// Drops a record for a rename that never happened, so nothing is claimed to be
/// on disk that isn't.
fn retire(state: &Arc<WriteOperationState>, record: Option<&TrackedRecord>) {
    if let Some(record) = record {
        in_flight_temps::retire(state, record);
    }
}

impl DisplacedDestination {
    /// Holds `aside` as the place `original` goes, both spelled for the volume once.
    fn new(
        aside: StagingTemp,
        original: &Path,
        is_directory: bool,
        record: Option<TrackedRecord>,
        state: &Arc<WriteOperationState>,
    ) -> Self {
        Self {
            aside_app: AppPath::legacy_unchecked(aside.path()).into_owned(),
            original_app: AppPath::legacy_unchecked(original).into_owned(),
            aside,
            original: original.to_path_buf(),
            is_directory,
            record,
            state: Arc::clone(state),
        }
    }

    /// Records where a file→file Overwrite's landing is about to set the user's
    /// original at `original` aside, for a replacement of `replacement_size`
    /// bytes: on disk before this resolves (`in_flight_temps::hold_aside`), so
    /// a crash after the rename aside leaves a record the sweep settles. The
    /// caller does the rename, classifying its answer the way the rest of its
    /// landing does (`staged_write_landing.rs`).
    ///
    /// `home` is the staged write's own ledger home: a remote zip edit's
    /// operation names no single destination volume, so it can't come from the
    /// state. ❗ [`NotOnDisk`] means the record isn't on disk, and the original
    /// must ❌ not be renamed.
    pub(crate) async fn hold_for_swap(
        state: &Arc<WriteOperationState>,
        home: Option<RecordHome>,
        original: &Path,
        replacement_size: u64,
    ) -> Result<Self, NotOnDisk> {
        let aside = StagingTemp::mint_aside(original, uuid::Uuid::new_v4(), state.liveness_token());
        let record = in_flight_temps::hold_aside(state, home, aside.path(), original, replacement_size).await?;
        Ok(Self::new(aside, original, false, record, state))
    }

    /// Where the original goes (or went).
    pub(crate) fn aside_path(&self) -> &Path {
        self.aside.path()
    }

    /// Nothing was set aside after all: its record goes, so nothing is claimed
    /// to be on disk that isn't.
    pub(crate) fn forget(self) {
        retire(&self.state, self.record.as_ref());
    }

    /// Whether it was set aside can't be told while the share is out: the
    /// record waits for the share's next arrival, whose sweep settles it.
    pub(crate) fn keep_for_arrival(self) {
        if let Some(record) = self.record {
            in_flight_temps::keep_for_arrival(&self.state, record);
        }
    }

    /// The replacement landed, so the entry it replaced goes. Best-effort: a
    /// leftover wears the recognizable `.cmdr-temp-<uuid>` name and becomes
    /// visible in the pane once the operation ends.
    ///
    /// A folder goes as a tree, and only here: the user answered Overwrite on a
    /// prompt naming both types, and what replaced it has fully landed.
    pub(crate) async fn discard(self, volume: &Arc<dyn Volume>) {
        let removed = if self.is_directory {
            remove_tree(volume, &self.aside_app, TreeRemoval::UserChoseOverwriteAcrossTypes)
                .await
                .map_err(|e| e.error)
        } else {
            volume.delete(&self.aside_app).await
        };
        match removed {
            Ok(()) => retire(&self.state, self.record.as_ref()),
            Err(e) => {
                log::warn!(
                    target: "copy",
                    "couldn't remove the displaced destination at {}: {e}. It keeps its record.",
                    LogPrivate::path(self.aside.path()), e = LogError(&e)
                );
                if let Some(record) = self.record {
                    in_flight_temps::keep_for_arrival(&self.state, record);
                }
            }
        }
    }

    /// The replacement couldn't take the name, so the user's file goes back to
    /// it: one rename, classified as [`restore`](Self::restore) classifies it,
    /// but with no rescue. `Some(self)` when it couldn't go back: the aside
    /// keeps the user's file and its record, for the caller to hand to the
    /// volume's next sweep, which puts it back once the name is free (or keeps
    /// it beside whatever took the name). A swap's landing wants exactly that:
    /// its new bytes leave temp space under a ` (recovered)` name, and the
    /// original, still recognizable as the aside, goes home later on its own.
    #[must_use = "an original that couldn't go back must be kept for the volume's next sweep"]
    pub(crate) async fn put_back(self, volume: &Arc<dyn Volume>) -> Option<Self> {
        let back = match volume.rename(&self.aside_app, &self.original_app, false).await {
            Ok(()) => true,
            Err(cut_off) if cut_off.transport_fault().is_some() => matches!(
                volume.get_metadata(&self.aside_app).await,
                Err(VolumeError::NotFound(_))
            ),
            Err(e) => {
                log::warn!(
                    target: "copy",
                    "couldn't put {} back at {}: {e}",
                    LogPrivate::path(self.aside.path()),
                    LogPrivate::path(&self.original), e = LogError(&e)
                );
                false
            }
        };
        if back {
            retire(&self.state, self.record.as_ref());
            None
        } else {
            Some(self)
        }
    }

    /// The replacement never landed, so the user's file comes home.
    ///
    /// `None` when it did. `Some(kept_at)` when even THAT rename refused: the
    /// bytes then wear a ` (recovered)` name beside where they belong (or, if
    /// nothing landed at all, still the aside's), and the caller has to name
    /// that path in the failure the user reads, because it is the only place
    /// their file is.
    ///
    /// ❗ A rename back that fails on the transport may have been performed, so
    /// one stat of the aside classifies it before anything acts on a guess: gone
    /// means it's home; there, on a share that answers, the rename runs once
    /// more; a share that doesn't answer keeps the record for its next arrival,
    /// and the bytes are where the aside is. A rescue on the raw error would
    /// report a file that was already home as kept under its scratch name.
    pub(crate) async fn restore(self, volume: &Arc<dyn Volume>) -> Option<PathBuf> {
        let mut result = volume.rename(&self.aside_app, &self.original_app, false).await;
        if let Err(cut_off) = &result
            && cut_off.transport_fault().is_some()
        {
            match volume.get_metadata(&self.aside_app).await {
                Err(VolumeError::NotFound(_)) => result = Ok(()),
                Ok(_) => result = volume.rename(&self.aside_app, &self.original_app, false).await,
                Err(_) => {
                    let kept_at = self.aside.path().to_path_buf();
                    self.keep_for_arrival();
                    return Some(kept_at);
                }
            }
        }
        match result {
            Ok(()) => {
                retire(&self.state, self.record.as_ref());
                None
            }
            Err(e) => {
                log::warn!(
                    target: "copy",
                    "couldn't put {} back at {}: {e}",
                    LogPrivate::path(self.aside.path()),
                    LogPrivate::path(&self.original), e = LogError(&e)
                );
                let kept_at = rescue_out_of_temp_space(volume.as_ref(), self.aside.path(), &self.original).await;
                // The rescue either gave the bytes a real name or left them on
                // the aside. Still on the aside means the sweep is the next one
                // to try, so the record stays.
                if kept_at == self.aside.path() {
                    if let Some(record) = self.record {
                        in_flight_temps::keep_for_arrival(&self.state, record);
                    }
                } else {
                    retire(&self.state, self.record.as_ref());
                }
                Some(kept_at)
            }
        }
    }
}

/// The cross-type asides one cross-volume copy or move is holding, settled when
/// the operation ends.
///
/// A cross-type Overwrite isn't over when the resolver answers: a folder lands
/// leaf by leaf over the rest of the operation (and a deep clash can sit inside
/// any source), so the entry it replaced has to stay recoverable until the
/// operation knows how it ended. Op-wide rather than per source, because the
/// concurrent driver can ABANDON a source's future at its cancel deadline, and
/// an aside that lived in that future would go with it.
///
/// Each entry remembers whether its replacement LANDED (its whole top-level
/// source finished), which decides what an interrupted operation does with it
/// ([`DisplacedLedger::settle_interrupted`]).
// DEFAULT-OK: an empty ledger is the truth about an operation that has set
// nothing aside yet.
#[derive(Default)]
pub(super) struct DisplacedLedger {
    held: Mutex<Vec<Held>>,
}

struct Held {
    displaced: DisplacedDestination,
    landed: bool,
}

impl DisplacedLedger {
    /// Takes custody of an aside the resolver made.
    pub(super) fn hold(&self, displaced: DisplacedDestination) {
        self.held.lock_ignore_poison().push(Held {
            displaced,
            landed: false,
        });
    }

    /// The top-level source that landed at `dest_root` finished in full, so every
    /// aside at or under it was replaced by something complete.
    pub(super) fn landed_under(&self, dest_root: &Path) {
        for held in self.held.lock_ignore_poison().iter_mut() {
            if held.displaced.original.starts_with(dest_root) {
                held.landed = true;
            }
        }
    }

    /// The operation COMPLETED: every replacement is in, so every aside goes.
    pub(super) async fn discard_all(&self, volume: &Arc<dyn Volume>) {
        for held in self.take() {
            held.displaced.discard(volume).await;
        }
    }

    /// The operation was ROLLED BACK: what replaced each entry is already gone
    /// (the reversal ran first), so each comes home, newest first. One that
    /// can't is kept beside its name under a ` (recovered)` one, and answered.
    pub(super) async fn restore_all(&self, volume: &Arc<dyn Volume>) -> Vec<RecoveredOriginal> {
        let mut recovered = Vec::new();
        for held in self.take().into_iter().rev() {
            restore_into(held.displaced, volume, &mut recovered).await;
        }
        recovered
    }

    /// The operation was STOPPED or FAILED, which keeps everything that landed.
    ///
    /// An aside whose replacement landed in full goes, exactly as it would on
    /// success: the user asked for it to be replaced, and it was. Every other one
    /// comes home, and when a half-built folder already holds its name it is kept
    /// beside it under a ` (recovered)` name instead, which is answered so the
    /// failure can say where it went. Run it AFTER the partial cleanup, so a
    /// name a partial was holding is free again.
    pub(super) async fn settle_interrupted(&self, volume: &Arc<dyn Volume>) -> Vec<RecoveredOriginal> {
        let mut recovered = Vec::new();
        for held in self.take().into_iter().rev() {
            if held.landed {
                held.displaced.discard(volume).await;
            } else {
                restore_into(held.displaced, volume, &mut recovered).await;
            }
        }
        recovered
    }

    fn take(&self) -> Vec<Held> {
        std::mem::take(&mut *self.held.lock_ignore_poison())
    }
}

/// Puts one aside back, noting it in `recovered` when it had to settle for a
/// ` (recovered)` name (or, if even that refused, its scratch one).
async fn restore_into(
    displaced: DisplacedDestination,
    volume: &Arc<dyn Volume>,
    recovered: &mut Vec<RecoveredOriginal>,
) {
    let original = displaced.original.clone();
    if let Some(kept_at) = displaced.restore(volume).await {
        log::warn!(
            target: "copy",
            "kept the entry an Overwrite set aside at {} beside its name, at {}",
            LogPrivate::path(&original),
            LogPrivate::path(&kept_at)
        );
        recovered.push(RecoveredOriginal::new(&original, &kept_at));
    }
}

#[cfg(test)]
#[path = "displaced_destination_tests.rs"]
mod tests;
