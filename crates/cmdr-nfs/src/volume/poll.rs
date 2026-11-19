//! Polling for changes: the one place this backend breaks "subscribe, don't
//! poll", because NFSv3 has nothing to subscribe to.
//!
//! One `GETATTR` per directory a pane shows on the volume, every
//! [`POLL_EVERY`], and a [`DirectoryChange::FullRefresh`] only when the
//! directory's `(mtime, ctime)` moved since the pane listed it: renaming,
//! adding, or removing a name inside a folder bumps both on every server.
//!
//! - ❗ **Nothing with no pane open on the volume, and nothing while it isn't
//!   `Connected`.** The store is asked which directories are shown
//!   (`ListingHost::open_listing_paths`, an in-memory read); an empty answer
//!   sends nothing, and the poller stops once it has stayed empty for a tick.
//!   A volume that's down sends nothing either: ❌ never a `GETATTR` on top of
//!   the reconnect backoff, which already decides when the server is asked.
//! - ❗ **One `GETATTR` per directory per tick, ❌ never per entry**, pipelined
//!   on the one connection. The handle comes from the cache the listing filled,
//!   so a steady tick costs no `LOOKUP`.
//! - **A change for a directory no pane shows any more is dropped**: the store
//!   is asked again after the round, and only what it still shows is reported.
//! - **The stamp a change is measured against is the LISTING's**, from its
//!   first page (`Nfs3Client::list_dir`), so a change between the listing and
//!   the next tick is never mistaken for the baseline.
//! - The round runs through `noting`, so a poll that finds the server gone is
//!   the detector when nobody else is asking: the pane hears it within the
//!   silence ladder, and the backoff takes over.
//!
//! `listing_watch_coverage` stays `None`: a poll can miss a change that comes
//! and goes inside one tick, so it can't vouch for a pre-flight.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::http_session::{HttpBackend, HttpConnectionState};
use cmdr_fs::volume::remote_paths::RemoteRoot;
use cmdr_fs::volume::{DirectoryChange, VolumeError};
use futures_util::future::join_all;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use super::handles::{Follow, Stamp};
use super::reconnect::NfsClient;
use super::{NfsVolume, NfsVolumeInner};

/// How often a shown directory is asked about. 3 s feels live, and a `GETATTR`
/// is about 100 bytes each way, which a NAS answers from its inode cache.
pub(super) const POLL_EVERY: Duration = Duration::from_secs(3);

/// What one shown directory looked like, last time anyone asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    /// There, with this stamp.
    At(Stamp),
    /// Gone from under its name: reported once, ❌ not every tick.
    Gone,
}

/// One directory the poller remembers.
struct Remembered {
    /// Its last look, or `None` when its listing carried no stamp and the next
    /// tick takes the baseline.
    seen: Option<Seen>,
    /// When a listing last noted it. A directory the store doesn't show (yet)
    /// is kept for a tick after this: the engine files a listing only after
    /// the listing call returned.
    noted: Instant,
}

/// One folder the store shows, with its server path (`None` off this volume).
struct Shown {
    /// The store's own spelling, what a change is reported under.
    path: PathBuf,
    remote: Option<String>,
}

/// The poller's state, behind one lock: whether it runs, and what it saw.
#[derive(Default)]
pub(super) struct Poller {
    state: Mutex<PollState>,
}

#[derive(Default)]
struct PollState {
    /// Cancels the running poller. `None` when none runs: the next listing
    /// starts one.
    running: Option<CancellationToken>,
    /// What each shown directory looked like, by its server path (`/photos`):
    /// the listing names it by its app path, the store by its own spelling,
    /// and both come down to this one.
    seen: HashMap<String, Remembered>,
    /// When a listing last succeeded, so the poller doesn't stop in the gap
    /// between a listing and the store filing it.
    last_noted: Option<Instant>,
}

impl NfsVolume {
    /// A pane's listing of the folder at server path `remote` just succeeded,
    /// its folder stamped `stamp` as the listing's first page saw it: the
    /// baseline the next tick compares against, and the poller's cue to start
    /// if it isn't running.
    pub(super) fn listing_opened(&self, remote: String, stamp: Option<Stamp>) {
        self.inner.note_listing(remote, stamp);
    }
}

impl NfsVolumeInner {
    fn note_listing(self: &Arc<Self>, remote: String, stamp: Option<Stamp>) {
        let now = Instant::now();
        let mut state = self.poller.state.lock_ignore_poison();
        state.seen.insert(
            remote,
            Remembered {
                seen: stamp.map(Seen::At),
                noted: now,
            },
        );
        state.last_noted = Some(now);
        self.start_polling_under(&mut state);
    }

    /// Starts the poller if none runs, for a volume panes may already show
    /// under its id (a redial that replaced another instance): with nothing
    /// shown it stops itself a tick later, having sent nothing.
    pub(super) fn start_polling(self: &Arc<Self>) {
        let mut state = self.poller.state.lock_ignore_poison();
        state.last_noted = Some(Instant::now());
        self.start_polling_under(&mut state);
    }

    fn start_polling_under(self: &Arc<Self>, state: &mut PollState) {
        if state.running.is_some() {
            return;
        }
        let stop = CancellationToken::new();
        state.running = Some(stop.clone());
        self.host
            .runtime()
            .spawn(poll_until_stopped(Arc::downgrade(self), stop));
    }

    /// Stops the poller and forgets what it saw: the volume is leaving.
    pub(super) fn stop_polling(&self) {
        let mut state = self.poller.state.lock_ignore_poison();
        if let Some(stop) = state.running.take() {
            stop.cancel();
        }
        state.seen.clear();
    }

    /// One tick. Answers `false` when the poller should end: nothing has been
    /// shown for a whole tick, or this instance was retired.
    async fn poll_once(self: &Arc<Self>) -> bool {
        if self.session.connection().is_retired() {
            self.poller.state.lock_ignore_poison().running = None;
            return false;
        }
        let volume = NfsVolume::polling(Arc::clone(self));
        // Each shown folder beside its server path; `None` for one off this
        // volume, which has nothing to ask.
        let shown: Vec<Shown> = self
            .host
            .listings()
            .open_listing_paths(&self.volume_id)
            .into_iter()
            .map(|path| Shown {
                remote: volume.root.to_remote_path(&path),
                path,
            })
            .collect();
        {
            let now = Instant::now();
            let mut state = self.poller.state.lock_ignore_poison();
            state.seen.retain(|remote, remembered| {
                shown.iter().any(|one| one.remote.as_ref() == Some(remote)) || now - remembered.noted < POLL_EVERY
            });
            if shown.is_empty() {
                let quiet = state.last_noted.is_none_or(|noted| now - noted >= POLL_EVERY);
                if quiet {
                    state.running = None;
                }
                return !quiet;
            }
        }
        // ❌ Never on top of the backoff: a volume that's down is the
        // reconnect loop's to ask about.
        if self.session.connection().state() != HttpConnectionState::Connected {
            return true;
        }
        let Some(client) = self.session.installed_client().await else {
            return true;
        };
        let round = async {
            let looks = join_all(shown.iter().map(|one| volume.look(&client, one.remote.as_deref()))).await;
            // The first lost-server answer is the round's, so `noting` sees it.
            if let Some(lost) = looks
                .iter()
                .find_map(|look| look.as_ref().err().filter(|err| is_lost(err)))
            {
                return Err(lost.clone());
            }
            Ok(looks)
        };
        let Ok(looks) = self.noting(round).await else {
            return true;
        };
        self.report_moved(&shown, looks);
        true
    }

    /// Compares each look with what was remembered, and reports a
    /// [`DirectoryChange::FullRefresh`] for each directory that moved and a
    /// pane STILL shows.
    ///
    /// Two store spellings of one folder share its server path, so its stamp
    /// is compared once and both hear the change.
    fn report_moved(&self, shown: &[Shown], looks: Vec<Result<Seen, VolumeError>>) {
        let still_shown = self.host.listings().open_listing_paths(&self.volume_id);
        let now = Instant::now();
        let mut moved_remotes: Vec<&str> = Vec::new();
        {
            let mut state = self.poller.state.lock_ignore_poison();
            let mut compared: Vec<&str> = Vec::new();
            for (one, look) in shown.iter().zip(looks) {
                // Anything but an answer (a refusal, a path off this volume)
                // says nothing about whether the folder changed.
                let (Some(remote), Ok(look)) = (one.remote.as_deref(), look) else {
                    continue;
                };
                if compared.contains(&remote) {
                    continue;
                }
                compared.push(remote);
                let remembered = state
                    .seen
                    .entry(remote.to_owned())
                    .or_insert(Remembered { seen: None, noted: now });
                if remembered.seen.replace(look).is_some_and(|before| before != look) {
                    moved_remotes.push(remote);
                }
            }
        }
        let moved: Vec<PathBuf> = shown
            .iter()
            .filter(|one| {
                one.remote
                    .as_deref()
                    .is_some_and(|remote| moved_remotes.contains(&remote))
            })
            .filter(|one| still_shown.contains(&one.path))
            .map(|one| one.path.clone())
            .collect();
        for path in moved {
            self.host
                .listings()
                .directory_changed(&self.volume_id, &path, DirectoryChange::FullRefresh);
        }
    }
}

impl NfsVolume {
    /// A short-lived handle over `inner`, for the walks the poller needs. It
    /// names nothing and is never registered.
    fn polling(inner: Arc<NfsVolumeInner>) -> Self {
        Self {
            name: String::new(),
            root: RemoteRoot::new(inner.params().app_root(), Path::new("/")),
            inner,
        }
    }

    /// One shown directory's stamp now: one `GETATTR` on the handle the cache
    /// holds for it (a `STALE` one is looked up once more, fresh).
    /// `remote` is `None` for a shown folder off this volume, which answers
    /// `NotFound` without asking.
    async fn look(&self, client: &NfsClient, remote: Option<&str>) -> Result<Seen, VolumeError> {
        let Some(remote) = remote else {
            return Err(VolumeError::NotFound(String::new()));
        };
        let attrs = self
            .on_resolved(client, remote, Follow::Always, false, |found| {
                let nfs = client.nfs.clone();
                async move { nfs.getattr(&found.handle).await }
            })
            .await;
        match attrs {
            Ok(attrs) => Ok(Seen::At((attrs.mtime, attrs.ctime))),
            Err(VolumeError::NotFound(_)) => Ok(Seen::Gone),
            Err(err) => Err(err),
        }
    }
}

/// Whether `err` says the server is gone, the one answer `noting` acts on.
fn is_lost(err: &VolumeError) -> bool {
    matches!(err, VolumeError::DeviceDisconnected(_))
}

/// The poller's loop: a tick every [`POLL_EVERY`] until `stop`, the volume
/// going away, or a tick that finds nothing left to poll.
async fn poll_until_stopped(inner: Weak<NfsVolumeInner>, stop: CancellationToken) {
    let mut ticks = tokio::time::interval_at(Instant::now() + POLL_EVERY, POLL_EVERY);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => return,
            _ = ticks.tick() => {}
        }
        let Some(inner) = inner.upgrade() else { return };
        let keep_going = tokio::select! {
            biased;
            () = stop.cancelled() => return,
            keep_going = inner.poll_once() => keep_going,
        };
        if !keep_going {
            return;
        }
    }
}

#[cfg(test)]
#[path = "poll_test.rs"]
mod poll_test;
