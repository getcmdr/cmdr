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
    /// What the Servers list's rows are built from; a test swaps it to change them.
    servers: Mutex<ServerSources>,
    /// How many times a listing said the rows may have changed.
    rows_changes: Mutex<usize>,
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

    fn server_sources(&self) -> ServerSources {
        self.servers.lock_ignore_poison().clone()
    }

    fn rows_may_have_changed(&self) {
        *self.rows_changes.lock_ignore_poison() += 1;
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

/// The Servers list's date is a LAST USE, so sorting by "Last used" ranks the never-used
/// last and isn't held in name order by "Folders always by name"; a share list has no dates.
#[tokio::test]
async fn the_servers_list_dates_are_last_uses_and_a_share_list_has_none() {
    let (volume, _) = volume(testnas());
    assert_eq!(volume.supported_dates(&app("smb://")).await, SupportedDates::LAST_USE);
    assert_eq!(
        volume.supported_dates(&app("smb://testnas")).await,
        SupportedDates::NONE
    );
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
            description: Some("Old photos".to_string()),
            saved: None,
        })
    );
    assert_eq!(
        entries[1].share_meta,
        Some(ShareMeta {
            description: None,
            saved: None
        })
    );
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

/// `Testnas` saved with two shares, `archive` (mounted as `test_user_1`, pinned) and
/// `media` (saved as `test_user_2`, not mounted), beside the discovered host.
fn testnas_saved_with_shares() -> ServerSources {
    use crate::saved_servers::{SavedPlace, SavedServer, ServerNameSource};
    use crate::volumes::SmbMountInfo;
    use cmdr_fs::server_meta::ServerProtocol;
    let place = |name: &str, user: &str, pinned: bool| SavedPlace {
        volume_id: format!("smb-testnas-{name}"),
        name: name.to_string(),
        pinned,
        connected: false,
        app_root: format!("smb://testnas.local/{name}"),
        username: Some(user.to_string()),
        auto_reconnect: None,
    };
    ServerSources {
        saved: vec![SavedServer {
            id: "manual-testnas".to_string(),
            protocol: ServerProtocol::Smb,
            display_name: "Testnas".to_string(),
            name_source: ServerNameSource::User,
            address: "testnas.local".to_string(),
            username: None,
            pinned: false,
            last_connected_at: None,
            auto_reconnect: None,
            places: vec![
                place("archive", "test_user_9", true),
                place("media", "test_user_2", false),
            ],
            awaiting_first_connect: false,
            saved_from_mount: false,
        }],
        hosts: vec![host("Testnas", 445)],
        smb_mounts: vec![SmbMountInfo {
            server: "testnas.local".to_string(),
            share: "archive".to_string(),
            subpath: None,
            username: Some("test_user_1".to_string()),
            port: 445,
        }],
        ..ServerSources::default()
    }
}

/// ❗ A saved share's row carries its place: connected as the LIVE mount's account while
/// mounted (❌ not the saved one), saved as its saved account otherwise, its pin, and the
/// volume id Enter, Pin, and Forget share act on. A share nobody saved carries none.
#[tokio::test]
async fn a_saved_share_carries_its_place_on_the_share_list() {
    use cmdr_fs::server_meta::ServerStatus;
    let mut shares = testnas();
    shares.dialed.insert(
        "id-testnas".to_string(),
        Ok(listed(vec![
            share("archive", None),
            share("media", None),
            share("public", None),
        ])),
    );
    let (volume, _) = volume(with_servers(shares, testnas_saved_with_shares()));

    let entries = volume
        .list_directory_for_pane(&app("smb://testnas"), None, None)
        .await
        .expect("the share list lists");
    let saved: Vec<Option<SavedShareMeta>> = entries
        .iter()
        .map(|entry| entry.share_meta.as_ref().and_then(|meta| meta.saved.clone()))
        .collect();

    assert_eq!(
        saved,
        vec![
            Some(SavedShareMeta {
                volume_id: "smb-testnas-archive".to_string(),
                status: ServerStatus::Connected,
                account: Some(ShareListAccount::User {
                    username: "test_user_1".to_string()
                }),
                pinned: true,
            }),
            Some(SavedShareMeta {
                volume_id: "smb-testnas-media".to_string(),
                status: ServerStatus::Saved,
                account: Some(ShareListAccount::User {
                    username: "test_user_2".to_string()
                }),
                pinned: false,
            }),
            None,
        ]
    );
}

/// A re-list of an open share list reads its saved facts afresh off the share cache, with
/// no dial; with nothing cached it hands nothing (the rows on screen stay).
#[test]
fn a_share_list_relists_from_the_cache_with_fresh_saved_facts() {
    let mut shares = testnas();
    shares
        .cache
        .insert("id-testnas".to_string(), listed(vec![share("media", None)]));
    let (volume, shares) = volume(with_servers(shares, testnas_saved_with_shares()));

    let entries = volume
        .cached_share_list_entries("smb://testnas")
        .expect("the cache holds the list");
    let empty = volume.cached_share_list_entries("smb://othernas");

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]
            .share_meta
            .as_ref()
            .and_then(|meta| meta.saved.as_ref())
            .map(|saved| saved.volume_id.as_str()),
        Some("smb-testnas-media")
    );
    assert!(empty.is_none());
    assert!(volume.cached_share_list_entries("smb://").is_none());
    assert!(shares.dials().is_empty());
}

/// A saved SFTP server and a host mDNS found: the Servers list's two kinds of row.
fn servers() -> ServerSources {
    use crate::saved_servers::{SavedPlace, SavedServer, ServerNameSource};
    use cmdr_fs::server_meta::ServerProtocol;
    let sftp_id = "sftp-testhost-22-test_user_1".to_string();
    let app_root = "sftp://test_user_1@testhost:22".to_string();
    ServerSources {
        saved: vec![SavedServer {
            id: sftp_id.clone(),
            protocol: ServerProtocol::Sftp,
            display_name: "Testhost".to_string(),
            name_source: ServerNameSource::User,
            address: "testhost:22".to_string(),
            username: Some("test_user_1".to_string()),
            pinned: false,
            last_connected_at: Some("2026-09-01T10:00:00Z".to_string()),
            auto_reconnect: Some(true),
            places: vec![SavedPlace {
                volume_id: sftp_id,
                name: "Testhost".to_string(),
                pinned: false,
                connected: false,
                app_root,
                username: Some("test_user_1".to_string()),
                auto_reconnect: Some(true),
            }],
            awaiting_first_connect: false,
            saved_from_mount: false,
        }],
        hosts: vec![host("Testnas", 445)],
        ..ServerSources::default()
    }
}

fn with_servers(shares: FakeShares, sources: ServerSources) -> FakeShares {
    *shares.servers.lock_ignore_poison() = sources;
    shares
}

/// `(name, path, is_directory, icon_id)` per entry.
fn server_rows_of(entries: &[FileEntry]) -> Vec<(String, String, bool, String)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                entry.path.clone(),
                entry.is_directory,
                entry.icon_id.clone(),
            )
        })
        .collect()
}

/// ❗ `smb://` lists every known server as a folder, the same for EVERY reader (a pane,
/// ⌘R, Ask Cmdr's `list_dir`): the rows come from memory, the stores, and the mount
/// table, so there's nothing to gate. ❌ No dial and ❌ no Keychain read for any of them
/// (#324): the share cache and the one call that dials stay untouched.
#[tokio::test]
async fn the_servers_list_is_every_known_server_as_a_folder_for_every_reader() {
    let (volume, shares) = volume(with_servers(testnas(), servers()));
    let hub = app("smb://");

    let pane = volume.list_directory_for_pane(&hub, None, None).await.expect("lists");
    let refresh = volume.list_directory_for_refresh(&hub).await.expect("lists");
    let other = volume.list_directory(&hub, None).await.expect("lists");

    let expected = vec![
        (
            "Testhost".to_string(),
            "sftp://test_user_1@testhost:22".to_string(),
            true,
            crate::network::servers_listing::SERVER_ICON_ID.to_string(),
        ),
        (
            "Testnas".to_string(),
            "smb://testnas".to_string(),
            true,
            crate::network::servers_listing::SMB_SERVER_ICON_ID.to_string(),
        ),
    ];
    assert_eq!(server_rows_of(&pane), expected);
    assert_eq!(server_rows_of(&refresh), expected);
    assert_eq!(server_rows_of(&other), expected);
    assert!(shares.dials().is_empty(), "listing servers dials none of them");
    assert_eq!(*shares.cache_reads.lock_ignore_poison(), 0, "nor reads a share cache");
}

/// Each row carries its server's facts for the cells, and its last use as the date.
#[tokio::test]
async fn a_server_row_carries_its_facts_and_its_last_use() {
    use cmdr_fs::server_meta::{ServerProtocol, ServerStatus};
    let (volume, _) = volume(with_servers(testnas(), servers()));

    let entries = volume.list_directory(&app("smb://"), None).await.expect("lists");

    let sftp = &entries[0];
    let meta = sftp.server_meta.as_ref().expect("a server row carries its facts");
    assert_eq!(meta.protocol, ServerProtocol::Sftp);
    assert_eq!(meta.row_id, "sftp-testhost-22-test_user_1");
    assert_eq!(meta.status, ServerStatus::Saved);
    // 2026-09-01T10:00:00Z.
    assert_eq!(sftp.modified_at, Some(1_788_256_800));
    let nearby = entries[1].server_meta.as_ref().expect("facts");
    assert_eq!(nearby.status, ServerStatus::FoundNearby);
    assert_eq!(entries[1].modified_at, None, "never used: no date");
}

/// ❗ A row's path answers `get_metadata` with that row, whatever scheme it's on (a
/// one-place server's row sits at its place), from memory: ❌ never a dial.
#[tokio::test]
async fn metadata_on_a_server_rows_path_is_that_row() {
    let (volume, shares) = volume(with_servers(testnas(), servers()));

    let sftp = volume
        .get_metadata(&app("sftp://test_user_1@testhost:22"))
        .await
        .expect("a row's path is a folder");
    let smb = volume.get_metadata(&app("smb://testnas")).await.expect("a server");

    assert_eq!(sftp.name, "Testhost");
    assert!(sftp.is_directory && sftp.server_meta.is_some());
    assert_eq!(smb.name, "Testnas");
    assert!(smb.server_meta.is_some(), "a server's share-list path is its row");
    assert!(shares.dials().is_empty());
}

/// A share list's account changing changes its server's row, so the volume says so.
#[tokio::test]
async fn a_share_list_listing_says_the_servers_rows_may_have_changed() {
    let (volume, shares) = volume(testnas());

    volume
        .list_directory_for_pane(&app("smb://testnas"), None, None)
        .await
        .expect("lists");

    assert_eq!(*shares.rows_changes.lock_ignore_poison(), 1);
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
            description: Some("Old photos".to_string()),
            saved: None,
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

/// ❗ The engine serves the Servers list with no Servers-specific code: a pane's open on
/// `smb://` completes with every server as a folder row (one at another scheme's path
/// included), and a host appearing reaches the open listing through the re-list, with
/// ❌ no dial and ❌ no Keychain read on either path (#324).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_engine_lists_the_servers_list_and_a_host_found_relists_it() {
    use crate::network::servers_relist::{AppServersList, ServersList};
    let (volume, shares) = volume(with_servers(testnas(), servers()));
    let shares = Arc::clone(&shares);
    let volume_id = cmdr_fs::testing::unique_test_id("servers-engine");
    get_volume_manager().register(&volume_id, Arc::new(volume));
    let list = AppServersList {
        volume_id: volume_id.clone(),
    };
    let listing_id = uuid::Uuid::new_v4().to_string();
    assert!(!list.is_shown(), "no pane shows it yet");

    let read = read_to_end(
        crate::listing_host::listings(),
        &request(&listing_id, &volume_id, "smb:///"),
    )
    .await;
    let opened = listed_rows(&ListingId::new(listing_id.clone()));
    let shown = list.is_shown();
    shares.servers.lock_ignore_poison().hosts.push(host("Attic", 445));
    list.relist();
    let relisted = listed_rows(&ListingId::new(listing_id.clone()));
    crate::listing_host::listings().end(&ListingId::new(listing_id));
    let shown_after_close = list.is_shown();
    get_volume_manager().unregister(&volume_id);

    read.expect("the pane's open completes");
    let names = |rows: &[(String, bool)]| rows.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&opened), ["Testhost", "Testnas"]);
    assert!(shown, "the open listing shows the Servers list");
    assert_eq!(names(&relisted), ["Attic", "Testhost", "Testnas"]);
    assert!(relisted.iter().all(|(_, is_dir)| *is_dir));
    assert!(!shown_after_close);
    assert!(shares.dials().is_empty(), "neither the open nor the re-list dials");
    assert_eq!(*shares.cache_reads.lock_ignore_poison(), 0);
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

        fn server_sources(&self) -> ServerSources {
            ServerSources::default()
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
