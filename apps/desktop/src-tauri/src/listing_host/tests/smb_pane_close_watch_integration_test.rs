//! Closing a pane's listing leaves the volume's own watcher alone.
//!
//! `Listings::end` is what the pane-close IPC calls; this runs against a real
//! `cmdr-smb` session because the watcher whose lifetime is at stake is that
//! backend's.
//!
//! Same gating as every SMB integration cell: `#[ignore]`d by default, so start
//! the containers with `./apps/desktop/test/smb-servers/start.sh` and run
//! `cargo nextest run smb_integration --run-ignored all`.

use crate::operations_host::backend_suites::smb_test_support::*;
use cmdr_fs::app_path;
use cmdr_smb::volume::ConnectionState;

/// Regression: closing a pane's listing must NOT tear down the SMB watcher.
///
/// The watcher's lifetime is the VOLUME's (spawned at `connect_smb_volume`,
/// canceled only by `on_unmount` / reconnect), not a pane's. The index relies on
/// this: it must keep receiving change events while the volume's index is live,
/// even with no pane showing the share. `Listings::end` (the pane-close IPC)
/// only drops a listing-cache entry and its FSEvents `WatchedDirectory` (SMB has
/// none), so it can't reach the watcher. This test pins that: after a pane close,
/// the volume is still watched.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_pane_close_does_not_kill_index_watcher() {
    use crate::listing_host::listings;
    use cmdr_listing::ListingId;

    let vol = make_docker_volume().await;
    assert_eq!(vol.session_state(), ConnectionState::Direct);
    assert_eq!(
        vol.listing_watch_coverage(app_path!("/")),
        WatchCoverage::EveryWriter,
        "watcher must be alive right after connect",
    );

    // Simulate a pane closing its listing. Even for listing ids that were never
    // registered, this exercises the close path; the point is that NOTHING in it
    // cancels the volume-scoped SMB watcher.
    listings().end(&ListingId::new("some-pane-listing-id"));
    listings().end(&ListingId::new("another-pane-listing-id"));

    assert_eq!(
        vol.listing_watch_coverage(app_path!("/")),
        WatchCoverage::EveryWriter,
        "pane close must NOT tear down the volume's index watcher",
    );
}
