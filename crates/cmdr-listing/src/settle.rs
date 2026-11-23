//! Settling what a landing full re-read owes the patches that landed while it
//! read (`patch_log.rs`, `DETAILS.md` § "Read generations"): stat each owed
//! name again and patch it with what's on disk now, or read the whole
//! directory once more.

use std::sync::Arc;

use cmdr_fs::app_path::{AppName, AppPath, PathArithmetic};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{Volume, VolumeError};
use futures_util::StreamExt;

use crate::Inner;
use crate::patch_log::Recheck;

/// How many stats a recheck keeps in flight at once: enough to hide a network
/// volume's round trip, few enough not to crowd out the pane's own requests.
const RECHECK_STATS_IN_FLIGHT: usize = 16;

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
