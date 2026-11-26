//! The `network` volume: a server's share list as folder rows, who may dial, and the
//! answers a read-only, unwatched, unindexed volume owes.

use std::collections::HashMap;
use std::sync::Mutex;

use cmdr_fs::app_path::{AppPathBuf, PathFlavor};
use cmdr_listing::ListingId;
use cmdr_listing::testing::{read_to_end, request};
use cmdr_smb::{AuthMode, ShareInfo};

use super::*;
use crate::file_system::volume::manager::get_volume_manager;
use crate::ignore_poison::IgnorePoison;
use crate::network::HostSource;

/// A server's shares, scripted: what a dial answers and what the cache holds, with every
/// dial counted.
#[derive(Default)]
struct FakeShares {
    hosts: KnownHosts,
    dialed: HashMap<String, Result<ShareListResult, ShareListError>>,
    /// The account each scripted dial reports it listed as.
    accounts: HashMap<String, ShareListAccount>,
    cache: HashMap<String, ShareListResult>,
    dials: Mutex<Vec<(String, Freshness)>>,
    cache_reads: Mutex<usize>,
}

impl FakeShares {
    fn dials(&self) -> Vec<(String, Freshness)> {
        self.dials.lock_ignore_poison().clone()
    }
}

impl ServerShares for FakeShares {
    fn known_hosts(&self) -> KnownHosts {
        self.hosts.clone()
    }

    fn cached(&self, host: &NetworkHost) -> Option<ShareListResult> {
        *self.cache_reads.lock_ignore_poison() += 1;
        self.cache.get(&host.id).cloned()
    }

    fn list<'a>(
        &'a self,
        host: &'a NetworkHost,
        freshness: Freshness,
    ) -> Pin<Box<dyn Future<Output = Listed> + Send + 'a>> {
        self.dials.lock_ignore_poison().push((host.id.clone(), freshness));
        let result = self.dialed.get(&host.id).cloned().unwrap_or_else(|| {
            Err(ShareListError::HostUnreachable {
                message: "not scripted".to_string(),
            })
        });
        let account = self.accounts.get(&host.id).cloned();
        Box::pin(async move { Listed { result, account } })
    }
}

fn host(name: &str, port: u16) -> NetworkHost {
    NetworkHost {
        id: format!("id-{}", name.to_lowercase()),
        name: name.to_string(),
        hostname: Some(format!("{}.local", name.to_lowercase())),
        ip_address: Some("192.168.50.20".to_string()),
        port,
        source: HostSource::Discovered,
    }
}

fn share(name: &str, comment: Option<&str>) -> ShareInfo {
    ShareInfo {
        name: name.to_string(),
        is_disk: true,
        comment: comment.map(str::to_string),
    }
}

fn listed(shares: Vec<ShareInfo>) -> ShareListResult {
    ShareListResult {
        shares,
        auth_mode: AuthMode::GuestAllowed,
        from_cache: false,
    }
}

/// `Testnas` on 445 knows two shares, and a dial answers them.
fn testnas() -> FakeShares {
    let mut shares = FakeShares {
        hosts: KnownHosts {
            fresh: vec![host("Testnas", 445)],
            ..KnownHosts::default()
        },
        ..FakeShares::default()
    };
    shares.dialed.insert(
        "id-testnas".to_string(),
        Ok(listed(vec![
            share("archive", Some("Old photos")),
            share("public", None),
        ])),
    );
    shares
}

/// `text` as the app-path grammar spells it (a scheme root ends in `/`).
fn app(text: &str) -> AppPathBuf {
    AppPathBuf::parse(text, PathFlavor::Posix).expect("an app path")
}

fn volume(shares: FakeShares) -> (SmbServerVolume, Arc<FakeShares>) {
    let shares = Arc::new(shares);
    (
        SmbServerVolume::new(Arc::clone(&shares) as Arc<dyn ServerShares>),
        shares,
    )
}

/// ❗ A pane opening a share list gets one folder per share, each under the share
/// list's path, with the server's description as a fact.
#[tokio::test]
async fn a_pane_lists_a_servers_shares_as_folders() {
    let (volume, shares) = volume(testnas());

    let entries = volume
        .list_directory_for_pane(&app("smb://testnas"), None, None)
        .await
        .expect("the share list lists");

    let rows: Vec<(&str, &str, bool)> = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.path.as_str(), entry.is_directory))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("archive", "smb://testnas/archive", true),
            ("public", "smb://testnas/public", true)
        ]
    );
    assert_eq!(
        entries[0].share_meta,
        Some(ShareMeta {
            description: Some("Old photos".to_string())
        })
    );
    assert_eq!(entries[1].share_meta, Some(ShareMeta { description: None }));
    // A share draws as stacked folders, ❌ not the plain `dir` folder.
    assert!(entries.iter().all(|entry| entry.icon_id == SHARE_ICON_ID));
    assert_eq!(shares.dials(), vec![("id-testnas".to_string(), Freshness::Cached)]);
}

/// ⌘R on a share list re-reads past the cache, as the hub's ⌘R does.
#[tokio::test]
async fn a_refresh_lists_past_the_cache() {
    let (volume, shares) = volume(testnas());

    volume
        .list_directory_for_refresh(&app("smb://testnas"))
        .await
        .expect("the refresh lists");

    assert_eq!(shares.dials(), vec![("id-testnas".to_string(), Freshness::Fresh)]);
}

/// ❗ #324: nothing but a pane the person pointed at a server dials it. An agent's
/// `list_dir` (and any other plain read) gets `NotConnected` while nothing is cached.
/// ❗ Also the Keychain pin: `ServerShares::list` is the only way to the stored-password
/// read (`share_list_sign_in::list_for_pane`), so no `list` means no read.
#[tokio::test]
async fn a_read_that_is_no_pane_never_dials() {
    let (volume, shares) = volume(testnas());

    let plain = volume.list_directory(&app("smb://testnas"), None).await;
    let agent = cmdr_listing::entries_as_stored(&volume, &app("smb://testnas"), None).await;

    assert!(matches!(plain, Err(VolumeError::NotConnected(_))), "got {plain:?}");
    assert!(matches!(agent, Err(VolumeError::NotConnected(_))), "got {agent:?}");
    assert!(shares.dials().is_empty(), "no dial: {:?}", shares.dials());
}

/// What a pane already listed (or the hub's prefetch cached) is fair to share: the
/// cache answers a plain read, still with no dial.
#[tokio::test]
async fn a_read_that_is_no_pane_answers_from_the_cache() {
    let mut shares = testnas();
    shares
        .cache
        .insert("id-testnas".to_string(), listed(vec![share("archive", None)]));
    let (volume, shares) = volume(shares);

    let entries = volume
        .list_directory(&app("smb://testnas"), None)
        .await
        .expect("the cache answers");

    assert_eq!(entries.len(), 1);
    assert!(shares.dials().is_empty());
}

/// The hub never lists here: the frontend draws it.
#[tokio::test]
async fn the_hub_is_no_directory_here() {
    let (volume, shares) = volume(testnas());

    let listed = volume.list_directory_for_pane(&app("smb://"), None, None).await;

    assert!(matches!(listed, Err(VolumeError::NotADirectory(_))), "got {listed:?}");
    assert!(shares.dials().is_empty());
}

/// A share is mounted, ❌ never listed as a path on this volume.
#[tokio::test]
async fn a_share_is_never_listed_here() {
    let (volume, shares) = volume(testnas());

    let listed = volume
        .list_directory_for_pane(&app("smb://testnas/archive"), None, None)
        .await;

    assert!(matches!(listed, Err(VolumeError::NotConnected(_))), "got {listed:?}");
    assert!(shares.dials().is_empty());
}

/// A server nothing knows by that name is no folder, and nothing is dialed for it.
#[tokio::test]
async fn an_unknown_server_is_not_found_and_never_dialed() {
    let (volume, shares) = volume(testnas());

    let listed = volume.list_directory_for_pane(&app("smb://nobody"), None, None).await;

    assert!(matches!(listed, Err(VolumeError::NotFound(_))), "got {listed:?}");
    assert!(shares.dials().is_empty());
}

/// A server that wants an account answers the typed share-list refusal, naming the
/// server, ❌ never an empty list and ❌ never a folder's `PermissionDenied`.
#[tokio::test]
async fn a_refused_listing_is_the_typed_share_list_refusal() {
    let mut shares = testnas();
    shares.dialed.insert(
        "id-testnas".to_string(),
        Err(ShareListError::AuthRequired {
            message: "guest refused".to_string(),
        }),
    );
    let (volume, _) = volume(shares);

    let listed = volume.list_directory_for_pane(&app("smb://testnas"), None, None).await;

    let Err(VolumeError::ShareListRefused(refusal)) = listed else {
        panic!("not the share-list refusal: {listed:?}");
    };
    assert_eq!(refusal.server, "Testnas");
    assert!(matches!(refusal.error, ShareListError::AuthRequired { .. }));
}

/// `auth_required` and `auth_failed` stay apart: someone who never typed a password
/// isn't told theirs is wrong.
#[tokio::test]
async fn a_rejected_account_stays_a_rejected_account() {
    let mut shares = testnas();
    shares.dialed.insert(
        "id-testnas".to_string(),
        Err(ShareListError::AuthFailed {
            message: "rejected".to_string(),
        }),
    );
    let (volume, _) = volume(shares);

    let listed = volume.list_directory_for_pane(&app("smb://testnas"), None, None).await;

    assert!(
        matches!(&listed, Err(VolumeError::ShareListRefused(refusal)) if matches!(refusal.error, ShareListError::AuthFailed { .. })),
        "got {listed:?}"
    );
}

/// The account a pane listing used is what its `listing-complete` reports, and a refusal
/// drops it (nobody is signed in any more).
#[tokio::test]
async fn the_listed_account_is_remembered_per_server() {
    let mut shares = testnas();
    shares.accounts.insert(
        "id-testnas".to_string(),
        ShareListAccount::User {
            username: "testuser".to_string(),
        },
    );
    let (volume, _) = volume(shares);
    let path = app("smb://testnas");
    assert_eq!(volume.listed_account(&path), None, "nothing listed yet");

    volume.list_directory_for_pane(&path, None, None).await.expect("lists");

    assert_eq!(
        volume.listed_account(&path),
        Some(ShareListAccount::User {
            username: "testuser".to_string()
        })
    );
    assert_eq!(volume.listed_account(&app("smb://")), None, "the hub has no account");
}

/// A non-pane read (Ask Cmdr's `list_dir`) answers from the cache and changes no
/// account: only the person's own listing says who the pane is signed in as.
#[tokio::test]
async fn a_read_that_is_no_pane_leaves_the_account_alone() {
    let mut shares = testnas();
    shares
        .cache
        .insert("id-testnas".to_string(), listed(vec![share("public", None)]));
    let (volume, _) = volume(shares);
    let path = app("smb://testnas");

    volume.list_directory(&path, None).await.expect("cached");

    assert_eq!(volume.listed_account(&path), None);
}

/// About, a size walk, or a presence check asks `get_metadata`: memory and the cache
/// only, ❌ never a dial.
#[tokio::test]
async fn metadata_never_dials() {
    let mut shares = testnas();
    shares.cache.insert(
        "id-testnas".to_string(),
        listed(vec![share("archive", Some("Old photos"))]),
    );
    let (volume, shares) = volume(shares);

    let hub = volume.get_metadata(&app("smb://")).await.expect("the hub is a folder");
    let server = volume
        .get_metadata(&app("smb://testnas"))
        .await
        .expect("a known server is a folder");
    let cached_share = volume
        .get_metadata(&app("smb://testnas/archive"))
        .await
        .expect("a cached share is a folder");
    let uncached_share = volume.get_metadata(&app("smb://testnas/other")).await;
    let inside = volume.get_metadata(&app("smb://testnas/archive/photos")).await;
    let unknown = volume.get_metadata(&app("smb://nobody")).await;

    assert!(hub.is_directory && server.is_directory && cached_share.is_directory);
    assert_eq!(server.name, "Testnas");
    assert_eq!(cached_share.icon_id, SHARE_ICON_ID);
    assert_eq!(
        cached_share.share_meta,
        Some(ShareMeta {
            description: Some("Old photos".to_string())
        })
    );
    assert!(
        matches!(uncached_share, Err(VolumeError::NotConnected(_))),
        "got {uncached_share:?}"
    );
    assert!(matches!(inside, Err(VolumeError::NotConnected(_))), "got {inside:?}");
    assert!(matches!(unknown, Err(VolumeError::NotFound(_))), "got {unknown:?}");
    assert!(shares.dials().is_empty());
}

/// ❗ Every answer a read-only, unwatched, unindexed, non-exporting volume owes. The
/// trait's defaults aren't all "no": `supports_local_fs_access` defaults to `true`,
/// which would hand `smb://<server>/<share>` to Quick Look and `std::fs` as a local path.
#[tokio::test]
async fn it_answers_as_a_read_only_unwatched_unindexed_volume() {
    let (volume, _) = volume(testnas());
    let path = &app("smb://testnas");

    let capabilities = volume.capabilities();
    assert!(!capabilities.backend_can_write);
    assert!(!capabilities.can_export);
    assert!(!capabilities.can_share_links);
    assert!(!capabilities.can_be_indexed, "no drive index arms on it");
    assert!(!capabilities.renames_can_copy);
    assert!(!capabilities.has_os_mount_fallback);
    assert!(!capabilities.can_store_xattrs);
    assert!(!volume.supports_local_fs_access());
    assert!(!volume.paths_are_os_visible());
    assert!(!volume.can_watch_listings(), "no change watch arms on it");
    assert!(volume.watch_listing(path).is_none());
    assert_eq!(
        volume.listing_watch_coverage(path),
        WatchCoverage::EveryWriter,
        "nothing writes a share list, so an unforced refresh may skip the dial"
    );
    assert!(!volume.has_trash());
    assert!(!volume.supports_streaming());
    assert!(!volume.routes_over_a_parent());
    assert!(volume.connection_state().is_none());
    assert_eq!(
        volume.root().as_str(),
        "smb:///",
        "the hub, as the app-path grammar spells it"
    );
    assert_eq!(volume.name(), crate::volume_listing::SERVERS_VOLUME_NAME);
    assert!(matches!(
        volume.create_directory(&app("smb://testnas/new")).await,
        Err(VolumeError::NotSupported)
    ));
    assert!(matches!(
        volume.delete(&app("smb://testnas/archive")).await,
        Err(VolumeError::NotSupported)
    ));
}

/// What a share list holds, read off the app's listing engine.
fn listed_rows(listing: &ListingId) -> Vec<(String, bool)> {
    crate::listing_host::listings()
        .entries(listing)
        .expect("the listing is open")
        .into_iter()
        .map(|entry| (entry.name, entry.is_directory))
        .collect()
}

/// ❗ The listing engine serves a share list with no share-specific code: a pane's
/// open on `smb://<server>` completes with the shares as folder rows. Proves the engine
/// takes a path under a scheme root with an empty authority (`smb://`) as on the volume.
#[tokio::test]
async fn the_engine_lists_a_share_list_as_folder_rows() {
    let (volume, shares) = volume(testnas());
    let volume_id = cmdr_fs::testing::unique_test_id("share-list-engine");
    get_volume_manager().register(&volume_id, Arc::new(volume));
    let listing_id = uuid::Uuid::new_v4().to_string();

    let read = read_to_end(
        crate::listing_host::listings(),
        &request(&listing_id, &volume_id, "smb://testnas/"),
    )
    .await;
    let rows = listed_rows(&ListingId::new(listing_id.clone()));
    crate::listing_host::listings().end(&ListingId::new(listing_id));
    get_volume_manager().unregister(&volume_id);

    read.expect("the pane's open completes");
    assert_eq!(rows, vec![("archive".to_string(), true), ("public".to_string(), true)]);
    assert_eq!(shares.dials().len(), 1, "the pane's open dials once");
}

/// ❗ #324 and the Keychain-once rule: the unforced refresh a delete or trash sends to
/// BOTH panes never dials a share list (a dial can read the stored password), while a
/// forced one (⌘R, MCP `refresh`) still re-reads past the cache.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_forced_refresh_dials_an_open_share_list() {
    let (volume, shares) = volume(testnas());
    let volume_id = cmdr_fs::testing::unique_test_id("share-list-refresh");
    get_volume_manager().register(&volume_id, Arc::new(volume));
    let listing_id = uuid::Uuid::new_v4().to_string();
    read_to_end(
        crate::listing_host::listings(),
        &request(&listing_id, &volume_id, "smb://testnas/"),
    )
    .await
    .expect("the pane's open completes");

    let unforced = crate::commands::file_system::refresh_listing(listing_id.clone(), false).await;
    let after_unforced = shares.dials();
    let forced = crate::commands::file_system::refresh_listing(listing_id.clone(), true).await;
    let after_forced = shares.dials();
    crate::listing_host::listings().end(&ListingId::new(listing_id));
    get_volume_manager().unregister(&volume_id);

    assert!(!unforced.timed_out && !forced.timed_out);
    assert_eq!(
        after_unforced,
        vec![("id-testnas".to_string(), Freshness::Cached)],
        "the unforced refresh adds no dial"
    );
    assert_eq!(
        after_forced,
        vec![
            ("id-testnas".to_string(), Freshness::Cached),
            ("id-testnas".to_string(), Freshness::Fresh)
        ],
        "the forced refresh re-reads past the cache"
    );
}

/// ❗ The root stays `smb://` whichever server a pane lists, so a restored `smb://a` tab
/// is never "moved" onto `smb://b`: the root history would read a root that changed as
/// the volume moving.
#[tokio::test]
async fn a_restored_share_list_tab_keeps_its_server_after_two_were_listed() {
    let mut shares = testnas();
    shares.hosts.fresh.push(host("Attic", 445));
    shares
        .dialed
        .insert("id-attic".to_string(), Ok(listed(vec![share("music", None)])));
    let (volume, _) = volume(shares);
    get_volume_manager().register(NETWORK_VOLUME_ID, Arc::new(volume));

    for server in ["smb://testnas/", "smb://attic/"] {
        let listing_id = uuid::Uuid::new_v4().to_string();
        read_to_end(
            crate::listing_host::listings(),
            &request(&listing_id, NETWORK_VOLUME_ID, server),
        )
        .await
        .expect("the share list lists");
        crate::listing_host::listings().end(&ListingId::new(listing_id));
        // What a volume-facts read records as each volume's live root.
        let _ = crate::location::resolve::VolumeFacts::now().await;
    }
    let answer = crate::location::last_roots::restored_tab_path(NETWORK_VOLUME_ID, "smb://testnas");
    get_volume_manager().unregister(NETWORK_VOLUME_ID);

    assert_eq!(answer, crate::location::last_roots::RestoredTabPath::Keep);
}

/// The real thing, against the Docker SMB fixtures: a share list of the guest fixture
/// lists its shares through the engine.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_share_list_of_the_fixture_lists_through_the_engine() {
    /// The fixture's shares, dialed as guest, the way a listing with no account does.
    struct FixtureShares {
        port: u16,
    }

    impl ServerShares for FixtureShares {
        fn known_hosts(&self) -> KnownHosts {
            KnownHosts {
                saved: vec![crate::network::manual_servers::create_network_host(
                    "127.0.0.1",
                    self.port,
                )],
                ..KnownHosts::default()
            }
        }

        fn cached(&self, _host: &NetworkHost) -> Option<ShareListResult> {
            None
        }

        fn list<'a>(
            &'a self,
            host: &'a NetworkHost,
            _freshness: Freshness,
        ) -> Pin<Box<dyn Future<Output = Listed> + Send + 'a>> {
            Box::pin(async move {
                let result = crate::network::smb_client::list_shares(
                    &host.id,
                    "127.0.0.1",
                    None,
                    host.port,
                    None,
                    crate::network::smb_client::GuestAttempt::Try,
                    None,
                    Some(0),
                )
                .await;
                Listed { result, account: None }
            })
        }
    }

    let port = std::env::var("SMB_CONSUMER_GUEST_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10480);
    let volume_id = cmdr_fs::testing::unique_test_id("share-list-fixture");
    get_volume_manager().register(
        &volume_id,
        Arc::new(SmbServerVolume::new(Arc::new(FixtureShares { port }))),
    );
    let listing_id = uuid::Uuid::new_v4().to_string();

    let read = read_to_end(
        crate::listing_host::listings(),
        &request(&listing_id, &volume_id, &format!("smb://127.0.0.1:{port}/")),
    )
    .await;
    let rows = listed_rows(&ListingId::new(listing_id.clone()));
    crate::listing_host::listings().end(&ListingId::new(listing_id));
    get_volume_manager().unregister(&volume_id);

    read.expect("the fixture's share list lists");
    assert!(!rows.is_empty(), "the guest fixture has shares");
    assert!(
        rows.iter().all(|(_, is_dir)| *is_dir),
        "every share is a folder row: {rows:?}"
    );
}
