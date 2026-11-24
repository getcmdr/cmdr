//! The bridge from `notify`'s std channel to the live loop's, and what a message means
//! for the watch: changes to forward, noise to log, or the watch's end (#1363).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cmdr_fs::ignore_poison::IgnorePoison;
use tokio::sync::mpsc;

use super::super::super::FsChangeEvent;
use super::{OS_API, WatchedRoots, translate_notify_event};

/// Whether a watch the OS reports lost is GONE, so the watcher has to say so and stop.
///
/// - **Windows: yes.** The vendored `notify` emits an `Err` only as a watch ends (a
///   pulled drive, a dropped `subst` target, a re-arm that failed) and unwatches right
///   after, and a watched root that goes away arrives as a `Remove` naming the root
///   before the same unwatch (`vendor/notify/CMDR-PATCH.md`). One recursive watch over
///   the volume root is all there is (`WATCHES_VOLUME_ROOT`), so either one means
///   nothing watches the volume any more (#1363).
/// - **Linux: no.** inotify's errors name a subdirectory it couldn't add a watch for
///   (the rest keep watching), and a removed BRANCH root is an ordinary removal the
///   reconciler applies.
pub(super) const ROOT_LOSS_ENDS_THE_WATCH: bool = cfg!(target_os = "windows");

/// What one message from `notify` means for the watch.
#[derive(Debug)]
enum Delivery {
    /// Changes to forward.
    Changes(Vec<FsChangeEvent>),
    /// An error that leaves the watch running: logged, nothing forwarded.
    Noise,
    /// The watch is over; nothing more will arrive.
    Ended,
}

/// Classify one message from `notify`: see [`ROOT_LOSS_ENDS_THE_WATCH`] for when a
/// loss ends the watch.
fn deliver(
    result: notify::Result<notify::Event>,
    counter: &AtomicU64,
    watched_roots: &std::sync::Mutex<Vec<PathBuf>>,
    end_on_root_loss: bool,
) -> Delivery {
    match result {
        Err(e) if end_on_root_loss => {
            log::warn!("Watcher: the {OS_API} watch ended on a read error ({e}), so it will be re-armed");
            Delivery::Ended
        }
        Err(e) => {
            log::debug!("DriveWatcher: notify error: {e}");
            Delivery::Noise
        }
        Ok(event) if end_on_root_loss && removes_a_watched_root(&event, &watched_roots.lock_ignore_poison()) => {
            log::warn!("Watcher: the {OS_API} watch's root went away, so the watch ended");
            Delivery::Ended
        }
        Ok(event) => Delivery::Changes(translate_notify_event(event, counter, watched_roots)),
    }
}

/// Whether `event` reports a watched ROOT itself removed. ❌ Never forward it as a
/// removal: the root of a whole-volume watch is the volume, and deleting its row would
/// empty the index of a drive that may only be unplugged.
fn removes_a_watched_root(event: &notify::Event, watched_roots: &[PathBuf]) -> bool {
    matches!(event.kind, notify::EventKind::Remove(_))
        && event
            .paths
            .iter()
            .any(|path| watched_roots.iter().any(|root| same_root(root, path)))
}

/// Whether `a` and `b` spell the same root the way Windows compares names: case- and
/// separator-insensitive, a trailing separator ignored. `notify` reports the path it was
/// given, but a spelling that differs by `\` or case must not slip a root's removal
/// through as a delete. Only asked where a root loss ends the watch (Windows).
fn same_root(a: &std::path::Path, b: &std::path::Path) -> bool {
    let spell = |path: &std::path::Path| {
        path.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    };
    spell(a) == spell(b)
}

/// The bridge from `notify`'s std channel to the loop's, run on a blocking thread.
pub(super) struct Forward {
    pub(super) running: Arc<AtomicBool>,
    pub(super) counter: Arc<AtomicU64>,
    pub(super) watched_roots: WatchedRoots,
    /// Raised before the channel closes when the OS ended the watch.
    pub(super) ended: Arc<AtomicBool>,
    pub(super) end_on_root_loss: bool,
}

impl Forward {
    /// Forward until `notify` stops, the watcher stops, the loop goes away, or the OS
    /// ends the watch. On the last, the flag goes up BEFORE `event_sender` drops, so a
    /// loop that sees the channel close always sees why.
    pub(super) fn run(
        self,
        notify_rx: &std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
        event_sender: &mpsc::UnboundedSender<FsChangeEvent>,
    ) {
        while let Ok(result) = notify_rx.recv() {
            if !self.running.load(Ordering::Relaxed) {
                return;
            }
            let events = match deliver(result, &self.counter, &self.watched_roots, self.end_on_root_loss) {
                Delivery::Changes(events) => events,
                Delivery::Noise => continue,
                Delivery::Ended => {
                    self.ended.store(true, Ordering::Release);
                    self.running.store(false, Ordering::Relaxed);
                    return;
                }
            };
            for ev in events {
                // Unbounded send never blocks (Fix 2: no backpressure into
                // the notify backend); the event loop caps memory instead.
                if event_sender.send(ev).is_err() {
                    return; // receiver dropped
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::note_watched;
    use super::*;

    /// What the forward bridge put on the loop's channel and left behind, after
    /// `messages` from `notify`.
    struct Forwarded {
        paths: Vec<String>,
        ended: bool,
        running: bool,
    }

    fn forward(messages: Vec<notify::Result<notify::Event>>, end_on_root_loss: bool) -> Forwarded {
        let root = PathBuf::from("/vol");
        let running = Arc::new(AtomicBool::new(true));
        let ended = Arc::new(AtomicBool::new(false));
        let watched_roots = WatchedRoots::default();
        note_watched(&watched_roots, &root);
        let forward = Forward {
            running: Arc::clone(&running),
            counter: Arc::new(AtomicU64::new(1)),
            watched_roots,
            ended: Arc::clone(&ended),
            end_on_root_loss,
        };
        let (notify_tx, notify_rx) = std::sync::mpsc::channel();
        for message in messages {
            notify_tx.send(message).unwrap();
        }
        drop(notify_tx);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        forward.run(&notify_rx, &event_tx);
        drop(event_tx);
        let mut paths = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            paths.push(event.path);
        }
        Forwarded {
            paths,
            ended: ended.load(Ordering::Acquire),
            running: running.load(Ordering::Relaxed),
        }
    }

    fn created(path: PathBuf) -> notify::Result<notify::Event> {
        Ok(notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::Any)).add_path(path))
    }

    fn removed(path: PathBuf) -> notify::Result<notify::Event> {
        Ok(notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(path))
    }

    /// What the vendored `notify` sends as a `ReadDirectoryChangesW` watch dies (a
    /// pulled drive: `ERROR_NOT_READY`), just before it unwatches.
    fn fatal_read_error() -> notify::Result<notify::Event> {
        Err(notify::Error::io(std::io::Error::other("the device is not ready")).add_path(PathBuf::from("/vol")))
    }

    /// #1363: the error was logged at debug and the loop went on reading a channel
    /// nothing would ever write to again, so the drive's index read live and stopped
    /// changing. The flag is up when the channel closes, and nothing after the error
    /// is forwarded.
    #[test]
    fn a_read_error_ends_the_watch_and_says_so_before_the_channel_closes() {
        let forwarded = forward(
            vec![
                created(PathBuf::from("/vol/a.txt")),
                fatal_read_error(),
                created(PathBuf::from("/vol/b.txt")),
            ],
            true,
        );

        assert_eq!(
            forwarded.paths,
            ["/vol/a.txt"],
            "what came before the error still lands"
        );
        assert!(
            forwarded.ended,
            "the loop learns the watch ended, not that it was stopped"
        );
        assert!(!forwarded.running, "and the watcher reports itself not running");
    }

    /// A watched root that goes away is the watch's end too, ❌ never a removal of the
    /// volume's root row.
    #[test]
    fn a_watched_root_that_goes_away_ends_the_watch_and_is_not_forwarded() {
        let forwarded = forward(
            vec![removed(PathBuf::from("/vol/gone")), removed(PathBuf::from("/vol"))],
            true,
        );

        assert_eq!(
            forwarded.paths,
            ["/vol/gone"],
            "a folder under the root is an ordinary removal"
        );
        assert!(forwarded.ended);
    }

    /// inotify's errors name a subdirectory it couldn't watch, and the rest keeps
    /// watching; a removed branch root is a removal the reconciler applies.
    #[test]
    fn where_a_loss_does_not_end_the_watch_the_stream_goes_on() {
        let forwarded = forward(
            vec![
                fatal_read_error(),
                removed(PathBuf::from("/vol")),
                created(PathBuf::from("/vol/b.txt")),
            ],
            false,
        );

        assert_eq!(forwarded.paths, ["/vol", "/vol/b.txt"]);
        assert!(!forwarded.ended);
        assert!(forwarded.running);
    }

    /// Windows names one folder however its case and separators are spelled.
    #[test]
    fn a_root_spelled_differently_is_still_the_root() {
        assert!(same_root(&PathBuf::from("C:/Data/"), &PathBuf::from(r"c:\data")));
        assert!(same_root(&PathBuf::from("/vol"), &PathBuf::from("/vol/")));
        assert!(!same_root(&PathBuf::from("C:/Data"), &PathBuf::from("C:/Data/x")));
    }

    #[test]
    fn only_windows_ends_the_watch_on_a_root_loss() {
        assert_eq!(ROOT_LOSS_ENDS_THE_WATCH, cfg!(target_os = "windows"));
    }

    /// The real OS path, end to end through the vendored `notify` (#1363): a watched
    /// folder deleted under a live `ReadDirectoryChangesW` watch completes its read with
    /// `ERROR_ACCESS_DENIED` on a delete-pending handle, `notify` reports the root
    /// removed and unwatches, and the backend says the watch ended and closes the
    /// channel. The refused-read path (`start_read` failing) has no reliable trigger on
    /// a hosted runner; it's on the real-Windows checklist (`watch/DETAILS.md`).
    #[cfg(target_os = "windows")]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_watched_folder_deleted_on_windows_ends_the_watch_for_real() {
        let parent = tempfile::tempdir().expect("temp dir");
        let root = parent.path().join("watched");
        std::fs::create_dir(&root).expect("create the watched folder");
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let mut backend = super::super::start(&root, 0, event_tx).expect("the watch starts");
        let ended = backend.ended_flag();

        std::fs::remove_dir(&root).expect("delete the watched folder");
        let closed = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while event_rx.recv().await.is_some() {}
        })
        .await;

        assert!(closed.is_ok(), "the channel closes once the watched folder goes");
        assert!(ended.load(Ordering::Acquire), "as an ended watch, not a stop");
        backend.stop();
    }
}
