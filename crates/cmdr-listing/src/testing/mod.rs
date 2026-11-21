//! Fixtures for driving the engine in a test: an engine of the test's own
//! ([`TestEngine`]), one fake per seam, listings put straight into a cache
//! ([`TestListing`]), and volumes whose answers a test scripts.
//!
//! A test inside the engine builds its own [`TestEngine`] and shares nothing.
//! A test in the host that drives the host's one installed engine uses the
//! same [`TestListing`] over that engine, where the unique id and the end on
//! drop are what keep tests apart.

mod engine;
mod events;
mod listing;
mod observers;
mod volumes;

#[cfg(test)]
pub(crate) use engine::replace_entries;
pub use engine::{
    FakeMountTable, FakeVolumes, FixedLocale, RecordingLeases, TestEngine, TestEngineBuilder, collator_for, decorate,
    full_refresh, hold_diffs, pending_changes, publish_replacement, read_to_end, request,
};
pub use events::RecordingEvents;
pub use listing::{TestListing, TestListingGuard};
pub use observers::{Heard, RecordingObserver, StaticOverlay};
pub use volumes::{ScriptedListVolume, SpelledVolume, WatchCoverageVolume};

/// `text`, a canonical app path a test names (a `const` the `app_path!` literal form can't take).
#[cfg(test)]
pub(crate) fn app(text: &str) -> &cmdr_fs::app_path::AppPath {
    cmdr_fs::app_path::AppPath::from_normalized(text).expect("a canonical app path")
}

/// `name` inside the canonical app path `dir`, joined by the app path's own join, so a
/// test builds the path a volume lists on every OS (`Path::join` adds `\` on Windows).
#[cfg(test)]
pub(crate) fn app_child(dir: &str, name: &str) -> cmdr_fs::app_path::AppPathBuf {
    use cmdr_fs::app_path::{AppName, PathArithmetic};
    let name = AppName::from_app_spelling(name).expect("one plain name");
    app(dir).join(name).expect("joins")
}

/// A read's cancel token and stall watch, as `Listings::streaming_state` hands
/// it out.
pub use crate::streaming::StreamingListingState;
