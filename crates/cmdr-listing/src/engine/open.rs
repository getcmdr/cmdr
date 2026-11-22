//! A pane opens and closes a listing.

use crate::ListingRequest;
use crate::listing_id::ListingId;

use super::Listings;

impl Listings {
    /// Starts reading `request`'s directory on the host runtime and returns at
    /// once. The cancel token is registered before this returns, so a
    /// [`cancel`](Self::cancel) right after still finds the read. Progress and
    /// the end arrive through the host's `ListingEvents`.
    pub fn start(&self, request: ListingRequest) {
        cmdr_fs::benchmark::reset_epoch();
        cmdr_fs::benchmark::log_event_value("list_directory_start_streaming CALLED", request.path.as_str());
        self.inner.start_streaming_listing(request);
        cmdr_fs::benchmark::log_event("list_directory_start_streaming RETURNING");
    }

    /// Stops waiting on `id`'s read. The read itself is never aborted: it
    /// detaches, and the pane hears `Cancelled`.
    pub fn cancel(&self, id: &ListingId) {
        self.inner.cancel_listing(id.as_str());
    }

    /// Drops `id` from the engine: its rows, its pending diffs, and (after the
    /// removal) every observer's hold on it.
    pub fn end(&self, id: &ListingId) {
        self.inner.list_directory_end(id.as_str());
    }

    /// The panes' heartbeat: stamps every listing in `ids` as still on screen
    /// and returns the ones the engine no longer holds, which the pane re-lists.
    pub fn keep_alive(&self, ids: &[ListingId]) -> Vec<ListingId> {
        let ids: Vec<String> = ids.iter().map(|id| id.as_str().to_string()).collect();
        self.inner
            .keep_listings_alive(&ids)
            .into_iter()
            .map(ListingId::new)
            .collect()
    }
}
