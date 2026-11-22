//! The app's half of the listing engine's tests: each adapter against the real
//! subsystem it answers from, and the cells whose point is an app piece (the
//! FSEvents watch, the volume registry's routing, a backend crate, the events
//! policy). The engine's own cells build an engine of their own, in
//! `crates/cmdr-listing/`.
//!
//! These share the test binary's ONE engine (`listing_host::listings()`) unless
//! a cell builds its own, so they keep unique ids (`TestListing` derives one).

mod adapters_test;
mod archive_watch_integration_test;
mod caching_reaper_test;
mod foreign_path_test;
mod lifecycle_order_test;
// `cmdr-local`'s backend against the app's real host: the coverage answer
// following the OS watcher, the panes' name order, and the composer a
// local mutation patches the pane through.
mod local_host_test;
#[cfg(feature = "virtual-mtp")]
mod mtp_listing_path_test;
mod overlay_refresh_test;
// A pane close drops a listing, and must not reach the volume's own watcher.
// Docker-gated, over a real `cmdr-smb` session, because the SMB watcher's
// lifetime is what the regression was about.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod smb_pane_close_watch_integration_test;
mod streaming_reaction_test;
mod streaming_test;
pub(crate) mod support;
