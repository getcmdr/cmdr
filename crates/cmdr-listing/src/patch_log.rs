//! Which names an incremental patch touched while a full re-read of the
//! listing was in flight, so the re-read's landing can stat them again
//! (`DETAILS.md` § "Read generations"). Without it, a re-read that started
//! before a patch and lands after it silently undoes the patch (#1342).

use std::collections::HashMap;
use std::sync::Arc;

use cmdr_fs::app_path::{AppName, AppPath, PathArithmetic};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{Volume, VolumeError};
use futures_util::StreamExt;

use crate::Inner;
use crate::cached_listing::CachedListing;
use crate::reread::ReadGeneration;

/// How many stats a recheck keeps in flight at once: enough to hide a network
/// volume's round trip, few enough not to crowd out the pane's own requests.
const RECHECK_STATS_IN_FLIGHT: usize = 16;

/// How many names one listing tracks. Past it, a landing re-reads the whole
/// directory once more instead of statting: a burst that big is cheaper as one
/// read, and correctness never rests on the bound.
pub(crate) const MAX_TRACKED_PATCHES: usize = 1_024;

/// The patched names, each stamped with the last read generation handed out
/// when its patch landed. A read with generation `G` started before every patch
/// stamped `G` or later, so those are the ones its landing may undo.
#[derive(Debug, Default)]
pub(crate) struct PatchLog {
    names: HashMap<String, ReadGeneration>,
    /// The newest stamp a full log turned away.
    dropped: Option<ReadGeneration>,
}

/// What a landing read owes the patches that landed while it was reading.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recheck {
    /// No patch landed while it read: the normal case, which costs nothing.
    Nothing,
    /// Stat these names again and patch them with what's on disk now.
    Names(Vec<String>),
    /// More patched than the log holds: read the whole directory once more.
    Reread,
}

impl PatchLog {
    /// How many names it tracks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// Notes a patch to `name`, stamped `stamp`.
    pub(crate) fn note(&mut self, name: &str, stamp: ReadGeneration) {
        if let Some(stamped) = self.names.get_mut(name) {
            *stamped = (*stamped).max(stamp);
            return;
        }
        if self.names.len() >= MAX_TRACKED_PATCHES {
            self.dropped = self.dropped.max(Some(stamp));
            return;
        }
        self.names.insert(name.to_owned(), stamp);
    }

    /// What a read with `generation` owes, as it lands. Forgets every name only
    /// that read or an older one could undo; a newer read still in flight keeps
    /// the ones stamped after it started.
    pub(crate) fn landing(&mut self, generation: ReadGeneration) -> Recheck {
        let overflowed = self.dropped.take().is_some_and(|dropped| dropped >= generation);
        let owed: Vec<String> = if overflowed {
            Vec::new()
        } else {
            self.names
                .iter()
                .filter(|(_, stamp)| **stamp >= generation)
                .map(|(name, _)| name.clone())
                .collect()
        };
        self.names.retain(|_, stamp| *stamp > generation);
        if overflowed {
            Recheck::Reread
        } else if owed.is_empty() {
            Recheck::Nothing
        } else {
            Recheck::Names(owed)
        }
    }
}

impl CachedListing {
    /// Whether a full re-read of this listing may be in flight, so a patch
    /// must be noted. False in the normal case, where noting costs nothing.
    pub(crate) fn tracks_patches(&self) -> bool {
        self.started_generation > self.published_generation
    }

    /// Notes a patch to `name` if a re-read may be in flight. Call it under the
    /// cache write lock with [`Inner::read_generation_now`] as `now`.
    pub(crate) fn note_patch(&mut self, name: &str, now: ReadGeneration) {
        if self.tracks_patches() {
            self.patch_log.note(name, now);
        }
    }
}

/// How settling a landing's patches went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    /// Patched again, or a re-read scheduled, or nothing was owed.
    Done,
    /// Something was owed, but nothing serves the listing's volume to stat or
    /// re-read through: only the host can read the directory again.
    NoVolume,
}

/// What a stat of one owed name found.
enum Seen {
    Present(Box<FileEntry>),
    Gone(String),
    /// Couldn't tell (a refusal, a dropped connection): only a re-read settles it.
    Unknown,
}

impl Inner {
    /// Settles what a landing read owed (`Recheck`), with no lock held: stats
    /// each owed name and patches it with what's on disk now, or schedules one
    /// more full re-read when the log overflowed or a stat couldn't tell.
    pub(crate) async fn settle_patches(self: &Arc<Self>, listing_id: &str, recheck: Recheck) -> Settled {
        let names = match recheck {
            Recheck::Nothing => return Settled::Done,
            Recheck::Names(names) => Some(names),
            Recheck::Reread => None,
        };
        let Some((volume_id, path)) = self.get_listing_volume_id_and_path(listing_id) else {
            return Settled::Done;
        };
        let resolved = self.volumes.resolve(&volume_id, &path).await;
        let is_routed = resolved.is_routed();
        let Some(volume) = resolved.volume else {
            return Settled::NoVolume;
        };
        let Some(names) = names else {
            log::debug!(target: "listing", "listing {listing_id}: too many patches during a re-read, reading again");
            self.spawn_full_refresh(volume_id, path, vec![listing_id.to_owned()]);
            return Settled::Done;
        };

        let volume = &volume;
        let path: &AppPath = &path;
        let seen: Vec<Seen> = futures_util::stream::iter(names)
            .map(|name| stat_child(volume, path, name))
            .buffer_unordered(RECHECK_STATS_IN_FLIGHT)
            .collect()
            .await;

        let mut present = Vec::new();
        let mut unknown = false;
        for seen in seen {
            match seen {
                Seen::Present(entry) => present.push(*entry),
                Seen::Gone(name) => self.notify_removed(listing_id, &name),
                Seen::Unknown => unknown = true,
            }
        }
        if !present.is_empty() {
            // ONE enrich for the batch, as `observe_children` does. A routed
            // volume's rows have no index to ask.
            if !is_routed {
                self.index.enrich(&volume_id, &mut present);
            }
            for entry in present {
                // An upsert that carries Finder tags forward.
                self.notify_added(listing_id, entry);
            }
        }
        if unknown {
            self.spawn_full_refresh(volume_id, path.to_owned(), vec![listing_id.to_owned()]);
        }
        Settled::Done
    }
}

/// Stats `name` in `dir` on `volume`.
async fn stat_child(volume: &Arc<dyn Volume>, dir: &AppPath, name: String) -> Seen {
    let Ok(child) = AppName::from_app_spelling(&name).map(|child| dir.join(child)) else {
        return Seen::Unknown;
    };
    let Ok(child) = child else {
        return Seen::Unknown;
    };
    match volume.get_metadata(&child).await {
        Ok(entry) => Seen::Present(Box::new(entry)),
        Err(VolumeError::NotFound(_)) => Seen::Gone(name),
        Err(_) => Seen::Unknown,
    }
}
