//! Settling what a landing full re-read owes the patches that landed while it
//! read (`patch_log.rs`, `DETAILS.md` § "Read generations"): check each owed
//! name against the disk and patch it again, or read the whole folder once
//! more.
//!
//! The check is a read of its own ([`Inner::start_read`]): patches that land
//! while it stats get noted, and it skips any name one of them wrote, since
//! that patch saw a newer disk than its stat did.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::RwLockIgnorePoison;
use cmdr_fs::volume::{Volume, VolumeError};

use crate::Inner;
use crate::patch_log::{Observed, Owed, Recheck};
use crate::rt::{now, time};

/// How long one pass of stats may take before what's still unanswered counts as
/// "can't tell". Two passes at most, so a dying mount holds a check up for
/// about four seconds, never for a run of per-stat timeouts.
const STAT_PASS_BUDGET: Duration = Duration::from_secs(2);

/// How settling a landing's patches went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    /// Patched again, a re-read scheduled, or nothing was owed.
    Done,
    /// The landing owed a full re-read and nothing serves the listing's volume
    /// to read through: only the host can read the folder again.
    NeedsHostRead,
}

/// What a stat of one owed name found.
#[derive(Debug)]
enum Seen {
    Present(Box<FileEntry>),
    Gone,
    /// Couldn't tell (a refusal, a dropped connection, no path to stat, out of
    /// time): after one retry, the patch's own observation stands.
    Unknown,
}

impl Inner {
    /// Settles what a landing read owed (`Recheck`), with no lock held.
    pub(crate) async fn settle_patches(self: &Arc<Self>, listing_id: &str, recheck: Recheck) -> Settled {
        let owed = match recheck {
            Recheck::Nothing => return Settled::Done,
            Recheck::Reread => return self.reread_once_more(listing_id).await,
            Recheck::Names(owed) => owed,
        };
        let Some((volume_id, dir)) = self.get_listing_volume_id_and_path(listing_id) else {
            return Settled::Done;
        };
        let check = self.start_read([listing_id]);
        let since = Some(check.generation());

        let resolved = self.volumes.resolve(&volume_id, &dir).await;
        let is_routed = resolved.is_routed();
        let mut seen: Vec<Seen> = match resolved.volume {
            // Nothing to stat through (the host reads this folder itself): every
            // patch's own observation stands. A change after it sends its own event.
            None => owed.iter().map(|_| Seen::Unknown).collect(),
            Some(volume) if volume.stat_lists_the_parent() => {
                drop(check);
                self.spawn_full_refresh(volume_id, dir, vec![listing_id.to_owned()]);
                return Settled::Done;
            }
            Some(volume) => {
                let paths = self.check_paths(listing_id, &owed);
                let mut seen = self.stat_pass(&volume, &paths).await;
                let unknown: Vec<usize> = (0..seen.len()).filter(|&i| matches!(seen[i], Seen::Unknown)).collect();
                if !unknown.is_empty() {
                    // One retry for what couldn't tell, then its patch stands.
                    let retry_paths: Vec<Option<String>> = unknown.iter().map(|&i| paths[i].clone()).collect();
                    let retried = self.stat_pass(&volume, &retry_paths).await;
                    for (i, answer) in unknown.into_iter().zip(retried) {
                        seen[i] = answer;
                    }
                }
                seen
            }
        };

        // ONE enrich for what the stats found, as `observe_children` does. A
        // routed volume's rows have no index to ask. A trusted observation was
        // enriched when its patch landed.
        if !is_routed {
            let mut slots: Vec<usize> = Vec::new();
            let mut found: Vec<FileEntry> = Vec::new();
            for (slot, answer) in seen.iter_mut().enumerate() {
                if matches!(answer, Seen::Present(_))
                    && let Seen::Present(entry) = std::mem::replace(answer, Seen::Unknown)
                {
                    slots.push(slot);
                    found.push(*entry);
                }
            }
            if !found.is_empty() {
                self.index.enrich(&volume_id, &mut found);
            }
            for (slot, entry) in slots.into_iter().zip(found) {
                seen[slot] = Seen::Present(Box::new(entry));
            }
        }

        for (owed, answer) in owed.into_iter().zip(seen) {
            match answer {
                Seen::Present(entry) => self.upsert_entry(listing_id, *entry, since),
                Seen::Gone => {
                    let _ = self.remove_entry_by_name_since(listing_id, &owed.name, since);
                }
                Seen::Unknown => match owed.observed {
                    Observed::Present(entry) => self.upsert_entry(listing_id, *entry, since),
                    Observed::Gone { .. } => {
                        let _ = self.remove_entry_by_name_since(listing_id, &owed.name, since);
                    }
                },
            }
        }
        drop(check);
        Settled::Done
    }

    /// One more full re-read of `listing_id`, for a landing that owed more than
    /// a check can settle.
    async fn reread_once_more(self: &Arc<Self>, listing_id: &str) -> Settled {
        let Some((volume_id, dir)) = self.get_listing_volume_id_and_path(listing_id) else {
            return Settled::Done;
        };
        if self.volumes.resolve(&volume_id, &dir).await.volume.is_none() {
            return Settled::NeedsHostRead;
        }
        log::debug!(target: "listing", "listing {listing_id}: a re-read owes more than a check, reading again");
        self.spawn_full_refresh(volume_id, dir, vec![listing_id.to_owned()]);
        Settled::Done
    }

    /// The path to stat for each owed name: the listing's own row when it holds
    /// one by that name (so the stat goes through the row's spelling), else the
    /// patch's.
    fn check_paths(&self, listing_id: &str, owed: &[Owed]) -> Vec<Option<String>> {
        let wanted: HashSet<&str> = owed.iter().map(|owed| owed.name.as_str()).collect();
        let held: HashMap<String, String> = {
            let cache = self.cache.read_ignore_poison();
            cache
                .get(listing_id)
                .map(|listing| {
                    listing
                        .entries()
                        .iter()
                        .filter(|row| wanted.contains(row.name.as_str()))
                        .map(|row| (row.name.clone(), row.path.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };
        owed.iter()
            .map(|owed| {
                held.get(&owed.name)
                    .cloned()
                    .or_else(|| owed.observed.path().map(str::to_owned))
            })
            .collect()
    }

    /// Stats every path at once, each on a task of its own, and waits at most
    /// [`STAT_PASS_BUDGET`] for the answers. A stat still running then is left to
    /// finish on its own, never dropped mid-call, and counts as "can't tell".
    async fn stat_pass(&self, volume: &Arc<dyn Volume>, paths: &[Option<String>]) -> Vec<Seen> {
        let stats: Vec<_> = paths
            .iter()
            .map(|path| {
                let path = AppPath::from_normalized(path.as_deref()?).ok()?.to_owned();
                let volume = Arc::clone(volume);
                Some(self.runtime.spawn(async move { stat(volume.as_ref(), &path).await }))
            })
            .collect();
        let deadline = now() + STAT_PASS_BUDGET;
        let mut seen = Vec::with_capacity(stats.len());
        for stat in stats {
            let answer = match stat {
                Some(stat) => match time::timeout(deadline.saturating_duration_since(now()), stat).await {
                    Ok(Ok(answer)) => answer,
                    _ => Seen::Unknown,
                },
                None => Seen::Unknown,
            };
            seen.push(answer);
        }
        seen
    }
}

/// What a stat of `path` on `volume` says.
async fn stat(volume: &dyn Volume, path: &AppPath) -> Seen {
    match volume.get_metadata(path).await {
        Ok(entry) => Seen::Present(Box::new(entry)),
        Err(VolumeError::NotFound(_)) => Seen::Gone,
        Err(_) => Seen::Unknown,
    }
}
