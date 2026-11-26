//! The `network` volume: the Servers list at `smb://` and, under it, each server's share
//! list at `smb://<server>`, one folder per share.
//!
//! `smb://` lists one folder per server (`servers_listing.rs`), the same for every reader:
//! its rows come from memory, the stores, and the mount table, so ❌ nothing there dials
//! or reads the Keychain. A row sits at the path it leads to: an SMB server's share-list
//! path, a one-place server's place (another scheme), an S3 account's root. It stays
//! current with no poll: `servers_relist.rs` re-lists it on every input change while a
//! pane shows it. A share list resolves its server from the path
//! (`share_list_path.rs`) and lists its shares as folder entries, each carrying the
//! server's description of it as [`ShareMeta`]. A share is MOUNTED, ❌ never entered
//! here: a path inside one answers `NotConnected`.
//!
//! ❗ **Only a pane the person pointed at a share list dials** (#324: listing signs in
//! to the server, as a guest where it lets one in). The pane's open is
//! `Volume::list_directory_for_pane` and its ⌘R `list_directory_for_refresh`; every
//! other reader (Ask Cmdr's `list_dir`, About, a size walk) reaches
//! `list_directory_with_cancel` and gets the share cache, else `NotConnected`, ❌ never a
//! dial.
//!
//! An unforced refresh (the one a delete or trash sends to both panes) never re-reads
//! it: the volume answers `WatchCoverage::EveryWriter`, so only ⌘R dials past the cache.
//!
//! Read-only, unwatched, unindexed, and no export: a server's list has nothing to write,
//! watch, walk, or copy out. Its paths are no filesystem paths
//! (`supports_local_fs_access` is `false`), so Quick Look and every `std::fs` fast path
//! leave them alone.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::share_list::ShareListRefusal;
use cmdr_fs::share_meta::{SavedShareMeta, ShareMeta};
use cmdr_fs::volume::{BackendKind, ListingProgress, SupportedDates, Volume, VolumeError, WatchCoverage};
use cmdr_smb::{ShareListError, ShareListResult};
use tokio_util::sync::CancellationToken;

use crate::ignore_poison::IgnorePoison;
use crate::network::NetworkHost;
use crate::network::servers_listing::{ServerRow, ServerSources, server_entry};
use crate::network::share_list_path::{KnownHosts, NetworkPath, ShareListAddress, resolve};
pub(crate) use crate::network::share_list_sign_in::{Listed, ShareListAccount};

/// The id the Servers hub and every share list are registered under.
pub(crate) const NETWORK_VOLUME_ID: &str = "network";

/// The `network` volume's root: the hub's path, which every share list sits under. The
/// app-path grammar spells it `smb:///`.
///
/// ❗ Constant, ❌ never the listed server's: the root history records every root a
/// volume answers, and a root that moved would rebase restored `smb://a` tabs onto `b`.
const NETWORK_ROOT: &str = "smb://";

/// Whether a listing may answer from the share cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freshness {
    /// The cache answers when it holds the list (a pane opening the share list).
    Cached,
    /// The cache is dropped first (⌘R on a share list re-reads, as the hub's ⌘R does).
    Fresh,
}

/// Where the share lists come from: the servers the app knows and their shares. The
/// app's answer is [`AppServerShares`]; a test hands in its own.
pub(crate) trait ServerShares: Send + Sync {
    /// The servers a share-list path may name. Memory and the manual store only.
    fn known_hosts(&self) -> KnownHosts;

    /// What the Servers list's rows are built from: the saved stores, discovery's fresh
    /// hosts, the volume list's states, and the kernel's mount table. ❌ Never a dial,
    /// ❌ never the Keychain, ❌ never a `statfs` per mount.
    fn server_sources(&self) -> ServerSources;

    /// A share list's listing changed what its server's row says (the account it signed
    /// in as): the Servers list re-lists if a pane shows it.
    fn rows_may_have_changed(&self) {}

    /// What the share cache holds for `host`, by the cache rule a listing with no
    /// credentials follows. ❌ Never a dial, ❌ never the Keychain.
    fn cached(&self, host: &NetworkHost) -> Option<ShareListResult>;

    /// Lists `host`'s shares, signing in the way a pane's listing does
    /// (`share_list_sign_in`), and says who it listed as. The one call that dials, and
    /// the one that may read the stored password (once per session).
    fn list<'a>(
        &'a self,
        host: &'a NetworkHost,
        freshness: Freshness,
    ) -> Pin<Box<dyn Future<Output = Listed> + Send + 'a>>;
}

/// Who a read is for, which decides whether it may dial.
#[derive(Debug, Clone, Copy)]
enum Reader {
    /// A pane the person opened: the cache, else a dial.
    Pane,
    /// ⌘R on an open share list: a dial, past the cache.
    Refresh,
    /// Anyone else: the cache, else `NotConnected`.
    Other,
}

/// The `network` volume. See the module docs.
pub(crate) struct SmbServerVolume {
    root: AppPathBuf,
    shares: Arc<dyn ServerShares>,
    /// Who each server's last pane listing signed in as, by host id: what that
    /// listing's `listing-complete` reports. Only a pane listing writes it.
    listed_as: Mutex<HashMap<String, ShareListAccount>>,
}

impl SmbServerVolume {
    pub(crate) fn new(shares: Arc<dyn ServerShares>) -> Self {
        Self {
            root: AppPathBuf::parse(NETWORK_ROOT, PathFlavor::Posix).expect("`smb://` is an app path"),
            shares,
            listed_as: Mutex::new(HashMap::new()),
        }
    }

    /// The account the last pane listing of the share list at `path` signed in as, from
    /// memory only; `None` for any other path, or when no listing said (a cached list).
    pub(crate) fn listed_account(&self, path: &AppPath) -> Option<ShareListAccount> {
        let NetworkPath::ShareList(address) = NetworkPath::of(path.as_str()) else {
            return None;
        };
        let host = resolve(&address, &self.shares.known_hosts())?;
        self.listed_as.lock_ignore_poison().get(&host.id).cloned()
    }

    /// The server the share list at `path` lists under, as its listing resolves it (so by
    /// the same host id the share cache and the session credential are keyed by), or `None`
    /// for any other path or a server nothing knows. Memory and the stores only.
    pub(crate) fn share_list_host(&self, path: &str) -> Option<NetworkHost> {
        let NetworkPath::ShareList(address) = NetworkPath::of(path) else {
            return None;
        };
        resolve(&address, &self.shares.known_hosts())
    }

    /// A pane listing (`freshness` says whether past the cache), with who it listed as
    /// noted for its `listing-complete`.
    async fn sign_in_and_list(&self, host: &NetworkHost, freshness: Freshness) -> Result<ShareListResult, VolumeError> {
        let Listed { result, account } = self.shares.list(host, freshness).await;
        let mut listed_as = self.listed_as.lock_ignore_poison();
        match (&result, account) {
            (Ok(_), Some(account)) => {
                listed_as.insert(host.id.clone(), account);
            }
            // A cached list doesn't say; the last listing's answer stands.
            (Ok(_), None) => {}
            (Err(_), _) => {
                listed_as.remove(&host.id);
            }
        }
        drop(listed_as);
        self.shares.rows_may_have_changed();
        result.map_err(|error| listing_refusal(host, error))
    }

    /// The Servers list's rows, now. The listed accounts are a snapshot: the lock is
    /// held for the clone only, ❌ never across a read of the sources.
    fn server_rows(&self) -> Vec<ServerRow> {
        let listed_as = self.listed_as.lock_ignore_poison().clone();
        self.shares.server_sources().rows(&listed_as)
    }

    /// The saved shares on the share list at `address`, by lowercase share name. Memory,
    /// the stores, and the mount table only.
    fn saved_shares(&self, address: &ShareListAddress) -> HashMap<String, SavedShareMeta> {
        let listed_as = self.listed_as.lock_ignore_poison().clone();
        self.shares.server_sources().saved_shares(&listed_as, address)
    }

    /// The share rows of `listed`, each with its saved facts.
    fn share_entries(&self, path: &str, address: &ShareListAddress, listed: &ShareListResult) -> Vec<FileEntry> {
        let saved = self.saved_shares(address);
        listed
            .shares
            .iter()
            .map(|share| share_entry(path, share, saved.get(&share.name.to_lowercase()).cloned()))
            .collect()
    }

    /// A share list's rows from the share cache alone, its saved facts read afresh: what a
    /// re-list hands a pane showing it when a share is saved, pinned, forgotten, mounted,
    /// or unmounted. `None` for any other path, or when the cache holds nothing (the
    /// rows on screen stay). ❌ Never a dial, never the Keychain.
    pub(crate) fn cached_share_list_entries(&self, path: &str) -> Option<Vec<FileEntry>> {
        let NetworkPath::ShareList(address) = NetworkPath::of(path) else {
            return None;
        };
        let host = resolve(&address, &self.shares.known_hosts())?;
        let listed = self.shares.cached(&host)?;
        Some(self.share_entries(path, &address, &listed))
    }

    /// The Servers list as the folder entries `smb://` lists. Memory only.
    pub(crate) fn server_entries(&self) -> Vec<FileEntry> {
        self.server_rows().into_iter().map(server_entry).collect()
    }

    /// The Servers row at `path`, whatever scheme the row's path is on.
    fn server_entry_at(&self, path: &AppPath) -> Option<FileEntry> {
        self.server_rows()
            .into_iter()
            .find(|row| {
                AppPathBuf::parse(&row.path, PathFlavor::Posix).is_ok_and(|row_path| row_path.as_str() == path.as_str())
            })
            .map(server_entry)
    }

    /// The server a share list's `path` names, or `NotFound` when nothing known goes by it.
    fn server_named(&self, path: &AppPath, address: &ShareListAddress) -> Result<NetworkHost, VolumeError> {
        resolve(address, &self.shares.known_hosts()).ok_or_else(|| VolumeError::NotFound(path.as_str().to_string()))
    }

    async fn list(&self, path: &AppPath, reader: Reader) -> Result<Vec<FileEntry>, VolumeError> {
        let address = match NetworkPath::of(path.as_str()) {
            NetworkPath::ShareList(address) => address,
            // Memory only, so every reader gets it (#324 has nothing to gate here).
            NetworkPath::Hub => return Ok(self.server_entries()),
            NetworkPath::InShare(_) => return Err(VolumeError::NotConnected(path.as_str().to_string())),
            NetworkPath::Elsewhere => return Err(VolumeError::NotFound(path.as_str().to_string())),
        };
        // The i18n capture's seam: a person's listing refused as the test said, with no lookup or dial.
        #[cfg(feature = "playwright-e2e")]
        if !matches!(reader, Reader::Other)
            && let Some(error) = crate::commands::e2e::stubbed_share_list_refusal()
        {
            return Err(VolumeError::ShareListRefused(Box::new(ShareListRefusal {
                server: address.key,
                error,
            })));
        }
        let host = self.server_named(path, &address)?;
        let listed = match reader {
            Reader::Other => self
                .shares
                .cached(&host)
                .ok_or_else(|| VolumeError::NotConnected(path.as_str().to_string()))?,
            Reader::Pane => self.sign_in_and_list(&host, Freshness::Cached).await?,
            Reader::Refresh => self.sign_in_and_list(&host, Freshness::Fresh).await?,
        };
        Ok(self.share_entries(path.as_str(), &address, &listed))
    }

    /// What `path` is, from memory and the share cache only.
    fn metadata(&self, path: &AppPath) -> Result<FileEntry, VolumeError> {
        let text = path.as_str();
        match NetworkPath::of(text) {
            NetworkPath::Hub => Ok(FileEntry::new(
                crate::volume_listing::SERVERS_VOLUME_NAME.to_string(),
                text.to_string(),
                true,
                false,
            )),
            // A server's share list is its Servers row; one nothing lists any more (a
            // host whose row went) still resolves as a folder by its name.
            NetworkPath::ShareList(address) => match self.server_entry_at(path) {
                Some(row) => Ok(row),
                None => {
                    let host = self.server_named(path, &address)?;
                    Ok(FileEntry::new(host.name, text.to_string(), true, false))
                }
            },
            NetworkPath::InShare(at) => {
                // A share's row, from the cache its share list read. Inside a share is
                // the mount's to answer.
                let not_connected = || VolumeError::NotConnected(text.to_string());
                if at.inside {
                    return Err(not_connected());
                }
                let host = self.server_named(path, &at.server)?;
                let listed = self.shares.cached(&host).ok_or_else(not_connected)?;
                let share = listed
                    .shares
                    .iter()
                    .find(|listed| listed.name == at.share)
                    .ok_or_else(not_connected)?;
                let saved = self.saved_shares(&at.server).remove(&share.name.to_lowercase());
                Ok(share_entry(&at.share_list, share, saved))
            }
            // A one-place server's row sits at its place, on another scheme.
            NetworkPath::Elsewhere => self
                .server_entry_at(path)
                .ok_or_else(|| VolumeError::NotFound(text.to_string())),
        }
    }
}

/// The icon id a share's row carries. The frontend draws it as the platform's own
/// folder icon twice, stacked (`stacked-icons.ts::stackedIconBase`), so it reads as
/// "shares" on every OS and never needs an OS fetch of its own.
pub(crate) const SHARE_ICON_ID: &str = "network:share";

/// A share's folder row on its server's share list at `share_list` (either spelling,
/// with or without the scheme root's trailing `/`): `smb://<key>/<share>`.
/// `saved` is the person's saved place for it, when they saved one.
fn share_entry(share_list: &str, share: &cmdr_smb::ShareInfo, saved: Option<SavedShareMeta>) -> FileEntry {
    FileEntry {
        icon_id: SHARE_ICON_ID.to_string(),
        share_meta: Some(ShareMeta {
            description: share.comment.clone().filter(|comment| !comment.trim().is_empty()),
            saved,
        }),
        ..FileEntry::new(
            share.name.clone(),
            format!("{}/{}", share_list.trim_end_matches('/'), share.name),
            true,
            false,
        )
    }
}

/// A listing the server refused: the typed share-list refusal, naming the server by
/// the name the Servers list knows it by. A sign-in class is the pane's sign-in
/// (`ShareListError::wants_sign_in`), the rest the server's answer.
fn listing_refusal(host: &NetworkHost, error: ShareListError) -> VolumeError {
    VolumeError::ShareListRefused(Box::new(ShareListRefusal {
        server: host.name.clone(),
        error,
    }))
}

impl Volume for SmbServerVolume {
    fn name(&self) -> &str {
        crate::volume_listing::SERVERS_VOLUME_NAME
    }

    fn root(&self) -> &AppPath {
        &self.root
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::ShareList
    }

    /// ❗ The trait's default is `true`, which would let Quick Look and every `std::fs`
    /// fast path read `smb://<server>/<share>` as a local path.
    fn supports_local_fs_access(&self) -> bool {
        false
    }

    /// ❗ `EveryWriter`, with no watch armed: nothing in the app writes a share list, so
    /// the unforced `refresh_listing` a delete or trash sends to both panes skips the
    /// re-read, which would dial the server and could read the stored password (#324).
    /// A share a server's admin adds shows on ⌘R (a forced refresh) or the next open.
    /// The Servers list is kept current by its own re-list (`servers_relist.rs`), so
    /// the answer holds there too.
    fn listing_watch_coverage(&self, _path: &AppPath) -> WatchCoverage {
        WatchCoverage::EveryWriter
    }

    /// The Servers list's date is each server's LAST USE: sorting by it ranks the never-used
    /// last and isn't held in name order by "Folders always by name" (every row is a folder
    /// standing for a server). A share list carries no dates at all.
    fn supported_dates<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = SupportedDates> + Send + 'a>> {
        let dates = match NetworkPath::of(path.as_str()) {
            NetworkPath::Hub => SupportedDates::LAST_USE,
            _ => SupportedDates::NONE,
        };
        Box::pin(std::future::ready(dates))
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(self.list(path, Reader::Other))
    }

    fn list_directory_for_pane<'a>(
        &'a self,
        path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
        _cancel: Option<&'a CancellationToken>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(self.list(path, Reader::Pane))
    }

    fn list_directory_for_refresh<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(self.list(path, Reader::Refresh))
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        Box::pin(async move { self.metadata(path) })
    }
}

/// The app's servers and shares: discovery, the manual store, the share cache, and the
/// pane listing's sign-in ladder (`share_list_sign_in::list_for_pane`).
pub(crate) struct AppServerShares {
    app: tauri::AppHandle,
}

impl AppServerShares {
    /// The host as a listing dials it: by its hostname, else its `.local` name.
    fn hostname_of(host: &NetworkHost) -> String {
        host.hostname
            .clone()
            .unwrap_or_else(|| crate::network::service_name_to_hostname(&host.name))
    }
}

/// The servers a share-list path may name, as the app knows them: discovery, the manual
/// store, and the display cache. Memory and the manual store only.
pub(crate) fn app_known_hosts(app: &tauri::AppHandle) -> KnownHosts {
    use crate::network::manual_servers::{all, create_network_host};
    KnownHosts {
        fresh: crate::network::fresh_discovered_hosts(),
        saved: all(app)
            .iter()
            .map(|entry| create_network_host(&entry.address, entry.port))
            .collect(),
        // The kernel's mount-table snapshot: no call per mount, nothing on the network.
        mounted: crate::network::smb_share_parent::mounted_hosts(&crate::volumes::smb_mounts().unwrap_or_default()),
        cached: crate::network::cached_discovered_hosts(),
    }
}

impl ServerShares for AppServerShares {
    fn known_hosts(&self) -> KnownHosts {
        app_known_hosts(&self.app)
    }

    fn server_sources(&self) -> ServerSources {
        let hosts = crate::network::fresh_discovered_hosts();
        let saved = crate::saved_servers::listing(
            crate::network::manual_servers::all(&self.app),
            crate::network::known_shares::get_all_known_shares(),
            &hosts,
        );
        // Each saved place's standing in the registry, the switcher's own read.
        let volume_states = crate::server_volumes::server_places()
            .into_iter()
            .map(|place| (place.id, place.state))
            .collect();
        // The kernel's mount-table snapshot: no call per mount, so a dead share can't
        // stall the Servers list.
        let smb_mounts = crate::volumes::smb_mounts()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, mount)| mount)
            .collect();
        ServerSources {
            saved,
            hosts,
            volume_states,
            smb_mounts,
        }
    }

    fn rows_may_have_changed(&self) {
        crate::network::servers_relist::request();
    }

    fn cached(&self, host: &NetworkHost) -> Option<ShareListResult> {
        let hostname = Self::hostname_of(host);
        let guest = crate::network::smb_account_ladder::guest_attempt_for(
            &self.app,
            &hostname,
            host.ip_address.as_deref(),
            host.port,
        );
        crate::network::smb_client::cached_listing(&host.id, guest)
    }

    fn list<'a>(
        &'a self,
        host: &'a NetworkHost,
        freshness: Freshness,
    ) -> Pin<Box<dyn Future<Output = Listed> + Send + 'a>> {
        Box::pin(async move {
            if freshness == Freshness::Fresh {
                crate::network::smb_client::invalidate_cache(&host.id);
            }
            crate::network::share_list_sign_in::list_for_pane(&self.app, host).await
        })
    }
}

/// The account the share list at `path` on `volume_id` signed in as, when that's the
/// `network` volume ([`SmbServerVolume::listed_account`]). Memory only.
pub(crate) fn listed_account(volume_id: &str, path: &AppPath) -> Option<ShareListAccount> {
    if volume_id != NETWORK_VOLUME_ID {
        return None;
    }
    let volume = crate::file_system::volume::manager::get_volume_manager().get(volume_id)?;
    volume.as_any().downcast_ref::<SmbServerVolume>()?.listed_account(path)
}

/// The server the share list at `path` lists under ([`SmbServerVolume::share_list_host`]),
/// for the frontend's sign-in to a server it holds no host for (one saved without contact,
/// which discovery never lists). Memory and the stores only.
pub(crate) fn share_list_host(path: &str) -> Option<NetworkHost> {
    let volume = crate::file_system::volume::manager::get_volume_manager().get(NETWORK_VOLUME_ID)?;
    volume.as_any().downcast_ref::<SmbServerVolume>()?.share_list_host(path)
}

/// The saved server whose Servers row opens the share list at `path`, by its id, or `None`
/// (no share list, or a server nobody saved). Memory, the stores, and the mount table only.
pub(crate) fn saved_server_of_share_list(app: &tauri::AppHandle, path: &str) -> Option<String> {
    let NetworkPath::ShareList(address) = NetworkPath::of(path) else {
        return None;
    };
    AppServerShares { app: app.clone() }
        .server_sources()
        .saved_row_opening(&address)
}

/// Registers the `network` volume and subscribes its Servers list's re-list. Once, at
/// startup: it dials nothing until a pane opens a share list, and the re-list does
/// nothing until a pane shows the Servers list.
pub(crate) fn register(app: &tauri::AppHandle) {
    let shares = Arc::new(AppServerShares { app: app.clone() });
    crate::file_system::volume::manager::get_volume_manager()
        .register(NETWORK_VOLUME_ID, Arc::new(SmbServerVolume::new(shares)));
    crate::network::servers_relist::subscribe(app);
}

#[cfg(test)]
#[path = "share_list_volume_test.rs"]
mod tests;
