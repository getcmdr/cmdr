//! What keeps something alive while a pane shows a directory.
//!
//! A [`ListingObserver`] hears one call when a listing enters the listing cache
//! and one when it leaves, in registration order. Cmdr's observers and their
//! order: the app's `listing_host/DETAILS.md` § "The observers".
//!
//! **Arming belongs to the BACKEND, ❌ never to a frontend subscription**:
//! `host/DETAILS.md` § "Observers".
//!
//! The host hands its observers to the engine's builder, in order
//! (`ListingsBuilder::observer`).

use cmdr_fs::app_path::AppPath;
use std::path::Path;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::Volume;

use crate::Inner;
use crate::listing_id::ListingId;

/// A listing that just entered the cache, as an observer hears it. The facts an
/// observer needs ride along, so none of them reads the cache back.
pub struct OpenedListing<'a> {
    /// The listing's id; [`ListingObserver::listing_closed`] names it again.
    pub id: &'a ListingId,
    /// The volume the pane asked for, which keys the listing.
    pub volume_id: &'a str,
    /// The volume that served the read (for a routed path, the routed one).
    pub volume: &'a dyn Volume,
    /// The directory as the read listed it.
    pub path: &'a Path,
    /// The directory in the volume's listing spelling, the one the cache keys on.
    pub stored_path: &'a Path,
    /// The engine that opened it, for [`Self::still_open`].
    pub(crate) engine: &'a Inner,
}

impl OpenedListing<'_> {
    /// Whether the listing is still in the engine that opened it. A close can
    /// race the open and reach every observer first, so an observer that keeps a
    /// record per open listing asks before keeping one.
    pub fn still_open(&self) -> bool {
        self.engine.contains(self.id.as_str())
    }
}

/// A subsystem that cares which directories a pane has open.
///
/// Called in registration order, outside every engine lock: on open after the
/// cache insert, on close after the cache removal. ❗ Both run on the open and
/// close paths, so an observer does cheap, non-blocking work and detaches the
/// rest: `git/arming.rs` decides from the path alone and hands the subscribe to
/// the blocking pool.
pub trait ListingObserver: Send + Sync {
    /// The observer's stable name (`"git-portal"`), which registration dedupes on.
    fn id(&self) -> &'static str;

    /// A pane opened a listing.
    fn listing_opened(&self, opened: &OpenedListing<'_>);

    /// The listing left the cache. The path isn't passed back: an observer that
    /// armed something remembers what, so a close releases what its open took.
    fn listing_closed(&self, id: &ListingId);
}

impl Inner {
    /// Asks the serving volume what it keeps alive while the listing is shown
    /// (`Volume::watch_listing`), then tells every observer, in order, that a
    /// listing opened.
    pub(crate) fn listing_opened(&self, opened: &OpenedListing<'_>) {
        self.keep_volume_watch(opened);
        for observer in self.observers() {
            observer.listing_opened(opened);
        }
    }

    /// Tells every observer, in order, that `id` left the cache, after letting
    /// go of what its volume kept for it.
    ///
    /// ❗ Call it AFTER the cache removal, never before: an observer's own detached
    /// arm reconciles against cache membership, so releasing while the entry is
    /// still there lets a racing arm re-take what the close just gave back.
    pub(crate) fn listing_closed(&self, id: &ListingId) {
        let released = self.volume_watches.lock_ignore_poison().remove(id.as_str());
        drop(released); // outside the lock: a backend's release may do work
        for observer in self.observers() {
            observer.listing_closed(id);
        }
    }

    /// Holds the volume's [`ListingWatch`](cmdr_fs::volume::ListingWatch) for
    /// the listing's life. A close racing this open may already have run, so
    /// the hold is kept only while the listing is still in the cache.
    fn keep_volume_watch(&self, opened: &OpenedListing<'_>) {
        let Some(hold) = opened.volume.watch_listing(&AppPath::legacy_unchecked(opened.path)) else {
            return;
        };
        let replaced = self
            .volume_watches
            .lock_ignore_poison()
            .insert(opened.id.as_str().to_string(), hold);
        drop(replaced);
        if !opened.still_open() {
            let released = self.volume_watches.lock_ignore_poison().remove(opened.id.as_str());
            drop(released);
        }
    }
}
