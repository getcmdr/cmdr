//! Where one source name lands in a destination folder: on the entry that's
//! already there, in THAT entry's own spelling, or as a new name spelled the way
//! the destination wants new names.
//!
//! Every engine asks this for every name it's about to write, top level and deep,
//! so the two rules live here once:
//!
//! - **A look-alike is taken, never free.** A byte-exact destination misses a
//!   name held in another Unicode form, and a write that believed it free would
//!   stand a second, identical-looking entry beside the user's
//!   (`../../look_alike.rs`). So it's a conflict like any other, and the path
//!   handed back is the stored entry's: an Overwrite replaces THAT entry, a merge
//!   walks into THAT folder, and the share ends with one.
//! - **Only a free name gets respelled** ([`Volume::spell_new_name`]): an entry
//!   that exists is only ever addressed by its stored bytes.
//!
//! A listed name joins its folder as a typed app path ([`ListedName`], the
//! app path's own join, in the leaf `listed_name.rs` beside the walk's
//! crossings back to `std::path`). `DETAILS.md` § "Look-alike names and
//! new-name spelling" and § "Listed names are untrusted".

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, ListedFolder};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::listed_name::{ListedName, into_std, join_under, on_volume, std_of};
use std::borrow::Cow;

use super::super::super::look_alike::{LookAlike, look_alike_in};
use super::super::super::state::WriteOperationState;
use super::super::super::types::WriteOperationError;
use super::super::dest_name_index::{DestLookup, DestNameIndex};
use super::super::transfer_driver::{FetchFut, NameAtDest};
use super::super::transfer_probe::{DriverPhase, OperationProbe};
use super::super::volume_outage::through_outages;
use super::transfer_error::{AtPath, WriteFailure};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{Volume, VolumeError};

/// Where a name lands.
pub(super) enum Landing {
    /// Nothing holds the name: write a new entry at this path, already spelled
    /// for the destination.
    Free(AppPathBuf),
    /// `entry` holds the name, at `path`, its OWN spelling, which may differ
    /// from the source's.
    Taken { path: AppPathBuf, entry: Box<FileEntry> },
}

impl Landing {
    /// The path to write, and what sits there if anything.
    pub(super) fn into_parts(self) -> (AppPathBuf, Option<FileEntry>) {
        match self {
            Self::Free(path) => (path, None),
            Self::Taken { path, entry } => (path, Some(*entry)),
        }
    }

    /// [`Self::into_parts`], the path in the `std::path` spelling the engine's
    /// bodies still take ([`into_std`]).
    pub(super) fn into_std_parts(self) -> (PathBuf, Option<FileEntry>) {
        let (path, entry) = self.into_parts();
        (into_std(path), entry)
    }
}

/// What the caller already knows about the destination folder.
#[derive(Clone, Copy)]
pub(super) enum DestFolder<'a> {
    /// This operation created the folder, so nothing the user had can be in it:
    /// every name is free, and nothing is asked.
    CreatedByUs,
    /// One listing of the folder, indexed. A byte-exact hit and a look-alike are
    /// settled in memory; only a name the listing can't settle is probed.
    Listed(&'a DestNameIndex),
    /// No listing: one probe per name, and a listing only when that probe misses
    /// a non-ASCII name on a byte-exact volume.
    Unlisted,
}

/// Whether a free name takes the destination's spelling.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NewName {
    /// A copy makes a new entry, so its name is spelled for the destination.
    Respell,
    /// A same-volume move keeps the entry it moves, bytes and all.
    Keep,
}

/// The conflict pre-check every serial engine (`copy_serial.rs`,
/// `move_cross.rs`, `move_same.rs`) hands the async driver as its
/// `dest_meta_fetcher`: [`name_at_destination`] for each top-level name, with the
/// step named on the operation's probe BEFORE the await. A `get_metadata` on a
/// wedged share returns to nobody, so a dump has to be able to name it as the
/// step in progress rather than leave the driver reading `starting`.
pub(super) fn top_level_precheck(
    state: &Arc<WriteOperationState>,
    dest_volume: &Arc<dyn Volume>,
    probe: Option<Arc<OperationProbe>>,
    new_name: NewName,
) -> impl for<'a> FnMut(&'a Path) -> FetchFut<'a> + use<> {
    let state = Arc::clone(state);
    let dest_volume = Arc::clone(dest_volume);
    move |dest: &Path| -> FetchFut<'_> {
        let state = Arc::clone(&state);
        let dest_volume = Arc::clone(&dest_volume);
        let probe = probe.clone();
        Box::pin(async move {
            if let Some(probe) = probe {
                probe.set_driver_phase_at(DriverPhase::PreparingNext, None, dest);
            }
            name_at_destination(&state, &dest_volume, dest, new_name).await
        })
    }
}

/// What the destination holds at one TOP-LEVEL name: [`where_it_lands`] with no
/// listing, for the path the driver would write.
///
/// ❗ **Only `NotFound` means free.** A `ConnectionTimeout`, a
/// `DeviceSessionReset`, a `PermissionDenied` (anything else) is the
/// destination refusing to answer, and reading that as "nothing is there" is
/// the whole bug: the item skips the resolver, the Skip/Stop policy is never
/// consulted, and the landing then clears whatever the probe was asked about.
/// So it fails THAT item, at the destination path. A merge child keeps the same
/// discipline one level down, through the same [`where_it_lands`].
///
/// ❌ No retry here. Per-file retry belongs to `retry.rs`, inside
/// `stream_pipe_file`, and a second layer above it would multiply the wait a
/// user sits through on a dead link (`transfer/CLAUDE.md`). A share that's GONE
/// is the one exception, and it isn't a retry: the probe waits for the share to
/// come back, on the deadline every waiter on it shares, and asks again
/// (`../volume_outage.rs`).
async fn name_at_destination(
    state: &Arc<WriteOperationState>,
    dest_volume: &Arc<dyn Volume>,
    dest: &Path,
    new_name: NewName,
) -> Result<NameAtDest, WriteOperationError> {
    // A name the destination refuses never reaches the conflict flow: no probe
    // (it'd find the sibling a stripping driver lands it on), no resolver, so
    // nothing is set aside or replaced for it. `copy_single_path` refuses it
    // before any write, and the driver asks about it there.
    if destination_refuses(state, dest) {
        return Ok(NameAtDest::Free(dest.to_path_buf()));
    }
    // Pathed, so a share that stays gone is worded as the lost connection it is.
    let landing = through_outages(state, &[dest_volume], || async move {
        landing_at(dest_volume, dest, DestFolder::Unlisted, new_name)
            .await
            .at_destination(dest)
    })
    .await;
    match landing.map_err(|e| WriteFailure::from(e).error)? {
        Landing::Free(path) => Ok(NameAtDest::Free(into_std(path))),
        Landing::Taken { path, entry } => Ok(NameAtDest::Taken {
            path: into_std(path),
            size: entry.size.unwrap_or(0),
        }),
    }
}

/// Whether the destination refuses `dest`'s own name
/// (`crate::name_rules::DestinationNames`, resolved as the transfer starts).
pub(super) fn destination_refuses(state: &WriteOperationState, dest: &Path) -> bool {
    dest.file_name()
        .is_some_and(|name| state.destination_names.refuses(name))
}

/// Where a TOP-LEVEL destination path the driver hands in lands: its last name
/// under its parent through [`where_it_lands`], or, for a source with no name
/// of its own, the destination folder itself, which no listing of a parent
/// describes (one probe, unless this operation made it).
pub(super) async fn landing_at(
    dest_volume: &Arc<dyn Volume>,
    dest: &Path,
    folder: DestFolder<'_>,
    new_name: NewName,
) -> Result<Landing, VolumeError> {
    // Split the way the driver's `Path` splits it (a leading `//` is no UNC root
    // on macOS or Linux), each half in the bridge's spelling.
    if let (Some(dir), Some(native)) = (dest.parent(), dest.file_name()) {
        let spelled = on_volume(Path::new(native));
        let name = ListedName::native(native, spelled.as_str())?;
        return where_it_lands(dest_volume, &on_volume(dir), &name, folder, new_name).await;
    }
    let dest = on_volume(dest);
    if matches!(folder, DestFolder::CreatedByUs) {
        return Ok(Landing::Free(dest.into_owned()));
    }
    match dest_volume.get_metadata(&dest).await {
        Ok(entry) => Ok(Landing::Taken {
            path: dest.into_owned(),
            entry: Box::new(entry),
        }),
        Err(VolumeError::NotFound(_)) => Ok(Landing::Free(dest.into_owned())),
        Err(e) => Err(e),
    }
}

/// Where `name` lands in `dest_dir`.
///
/// `Err(AmbiguousName)` when the folder holds two look-alikes and neither is
/// spelled as asked: which one the write means is a guess, and a guess that
/// overwrites is data loss. Any other `Err` is a probe or listing that couldn't
/// answer, and fails the item: ❌ never "nothing is there", which hands the
/// name to a fresh write whose landing clears what the probe was asked about.
///
/// ❗ `name` is a [`ListedName`] because it usually comes off a SOURCE listing,
/// which a hostile server or device writes: a raw `../x` or `/x` joined here
/// would land outside `dest_dir`. A look-alike's stored name, read off the
/// DESTINATION's listing, passes the same check before it's joined.
pub(super) async fn where_it_lands(
    dest_volume: &Arc<dyn Volume>,
    dest_dir: &AppPath,
    name: &ListedName<'_>,
    folder: DestFolder<'_>,
    new_name: NewName,
) -> Result<Landing, VolumeError> {
    let under = ListedFolder::new(dest_dir);
    let asked = join_under(&under, &name.app)?;
    let free = |asked: AppPathBuf| -> Result<Landing, VolumeError> {
        let spelled = match (new_name, name.text) {
            (NewName::Respell, Some(text)) => match dest_volume.spell_new_name(text) {
                Cow::Borrowed(same) if same == text => asked,
                spelled => join_under(&under, &AppName::from_listed_text(&spelled)?)?,
            },
            _ => asked,
        };
        Ok(Landing::Free(spelled))
    };
    let look_alike_at = |entry: Box<FileEntry>| -> Result<Landing, VolumeError> {
        Ok(Landing::Taken {
            path: join_under(&under, &AppName::from_listed_text(&entry.name)?)?,
            entry,
        })
    };
    let ambiguous = |asked: &AppPath| VolumeError::AmbiguousName(std_of(asked).display().to_string());

    let may_be_a_look_alike = match folder {
        DestFolder::CreatedByUs => return free(asked),
        DestFolder::Listed(index) => match index.lookup(name.text) {
            DestLookup::Absent => return free(asked),
            DestLookup::Present(entry) => return Ok(Landing::Taken { path: asked, entry }),
            DestLookup::LookAlike(entry) => return look_alike_at(entry),
            DestLookup::Ambiguous => return Err(ambiguous(&asked)),
            // The listing already ruled out a look-alike; only a case-only match
            // (or an alias it can't enumerate) is left for the backend to call.
            DestLookup::Unknown => false,
        },
        DestFolder::Unlisted => true,
    };

    match dest_volume.get_metadata(&asked).await {
        Ok(entry) => Ok(Landing::Taken {
            path: asked,
            entry: Box::new(entry),
        }),
        Err(VolumeError::NotFound(_)) => {
            let Some(text) = name.text.filter(|_| may_be_a_look_alike) else {
                return free(asked);
            };
            match look_alike_in(dest_volume.as_ref(), &std_of(dest_dir), text).await? {
                LookAlike::None => free(asked),
                LookAlike::One(entry) => look_alike_at(entry),
                LookAlike::Several => Err(ambiguous(&asked)),
            }
        }
        Err(e) => Err(e),
    }
}
