//! The `network` volume: the Servers hub at `smb://` and, under it, each server's share
//! list at `smb://<server>`, one folder per share.
//!
//! The hub itself never lists here (the frontend draws it), so `smb://` answers
//! `NotADirectory`. A share list resolves its server from the path
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
//! Read-only, unwatched, unindexed, and no export: a server's list has nothing to write,
//! watch, walk, or copy out. Its paths are no filesystem paths
//! (`supports_local_fs_access` is `false`), so Quick Look and every `std::fs` fast path
//! leave them alone.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::share_meta::ShareMeta;
use cmdr_fs::volume::{BackendKind, ListingProgress, Volume, VolumeError};
use cmdr_smb::{ShareListError, ShareListResult};
use tokio_util::sync::CancellationToken;

use crate::network::NetworkHost;
use crate::network::share_list_path::{KnownHosts, NetworkPath, ShareListAddress, resolve};

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

    /// What the share cache holds for `host`, by the cache rule a listing with no
    /// credentials follows. ❌ Never a dial, ❌ never the Keychain.
    fn cached(&self, host: &NetworkHost) -> Option<ShareListResult>;

    /// Lists `host`'s shares, signing in as a listing does. The one call that dials.
    fn list<'a>(
        &'a self,
        host: &'a NetworkHost,
        freshness: Freshness,
    ) -> Pin<Box<dyn Future<Output = Result<ShareListResult, ShareListError>> + Send + 'a>>;
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
}

impl SmbServerVolume {
    pub(crate) fn new(shares: Arc<dyn ServerShares>) -> Self {
        Self {
            root: AppPathBuf::parse(NETWORK_ROOT, PathFlavor::Posix).expect("`smb://` is an app path"),
            shares,
        }
    }

    /// The server a share list's `path` names, or `NotFound` when nothing known goes by it.
    fn server_named(&self, path: &AppPath, address: &ShareListAddress) -> Result<NetworkHost, VolumeError> {
        resolve(address, &self.shares.known_hosts()).ok_or_else(|| VolumeError::NotFound(path.as_str().to_string()))
    }

    async fn list(&self, path: &AppPath, reader: Reader) -> Result<Vec<FileEntry>, VolumeError> {
        let address = match NetworkPath::of(path.as_str()) {
            NetworkPath::ShareList(address) => address,
            NetworkPath::Hub => return Err(VolumeError::NotADirectory(path.as_str().to_string())),
            NetworkPath::InShare(_) => return Err(VolumeError::NotConnected(path.as_str().to_string())),
            NetworkPath::Elsewhere => return Err(VolumeError::NotFound(path.as_str().to_string())),
        };
        let host = self.server_named(path, &address)?;
        let listed = match reader {
            Reader::Other => self
                .shares
                .cached(&host)
                .ok_or_else(|| VolumeError::NotConnected(path.as_str().to_string()))?,
            Reader::Pane => self
                .shares
                .list(&host, Freshness::Cached)
                .await
                .map_err(|error| listing_refusal(path, error))?,
            Reader::Refresh => self
                .shares
                .list(&host, Freshness::Fresh)
                .await
                .map_err(|error| listing_refusal(path, error))?,
        };
        Ok(listed
            .shares
            .iter()
            .map(|share| share_entry(path.as_str(), share))
            .collect())
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
            NetworkPath::ShareList(address) => {
                let host = self.server_named(path, &address)?;
                Ok(FileEntry::new(host.name, text.to_string(), true, false))
            }
            NetworkPath::InShare(at) => {
                // A share's row, from the cache its share list read. Inside a share is
                // the mount's to answer.
                let not_connected = || VolumeError::NotConnected(text.to_string());
                if at.inside {
                    return Err(not_connected());
                }
                let host = self.server_named(path, &at.server)?;
                let listed = self.shares.cached(&host).ok_or_else(not_connected)?;
                listed
                    .shares
                    .iter()
                    .find(|listed| listed.name == at.share)
                    .map(|listed| share_entry(&at.share_list, listed))
                    .ok_or_else(not_connected)
            }
            NetworkPath::Elsewhere => Err(VolumeError::NotFound(text.to_string())),
        }
    }
}

/// A share's folder row on its server's share list at `share_list` (either spelling,
/// with or without the scheme root's trailing `/`): `smb://<key>/<share>`.
fn share_entry(share_list: &str, share: &cmdr_smb::ShareInfo) -> FileEntry {
    FileEntry {
        share_meta: Some(ShareMeta {
            description: share.comment.clone().filter(|comment| !comment.trim().is_empty()),
        }),
        ..FileEntry::new(
            share.name.clone(),
            format!("{}/{}", share_list.trim_end_matches('/'), share.name),
            true,
            false,
        )
    }
}

/// A listing the server refused, in the volume's vocabulary.
///
/// Interim: the sign-in classes read as `PermissionDenied` until the typed share-list
/// refusal (a `VolumeError` arm the frontend turns into a Sign in action) replaces this.
fn listing_refusal(path: &AppPath, error: ShareListError) -> VolumeError {
    let path = path.as_str().to_string();
    match error {
        ShareListError::AuthRequired { .. }
        | ShareListError::SigningRequired { .. }
        | ShareListError::AuthFailed { .. } => VolumeError::PermissionDenied {
            path,
            raw_os_error: None,
        },
        ShareListError::Timeout { message } => VolumeError::ConnectionTimeout(message.into()),
        ShareListError::SignatureInvalid { message } => VolumeError::SignatureInvalid(message.into()),
        ShareListError::NegotiationInvalid { message } => VolumeError::NegotiationInvalid(message.into()),
        ShareListError::EncryptionUnavailable { .. } => VolumeError::EncryptionUnavailable(path),
        ShareListError::EncryptionRequired { .. } => VolumeError::EncryptionRequired(path),
        ShareListError::HostUnreachable { message }
        | ShareListError::ProtocolError { message }
        | ShareListError::ResolutionFailed { message }
        | ShareListError::MissingDependency { message, .. } => VolumeError::IoError {
            message: message.into(),
            raw_os_error: None,
        },
    }
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
/// listing's sign-in ladder (`commands::network::list_host_shares`).
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

impl ServerShares for AppServerShares {
    fn known_hosts(&self) -> KnownHosts {
        use crate::network::manual_servers::{all, create_network_host};
        KnownHosts {
            fresh: crate::network::fresh_discovered_hosts(),
            saved: all(&self.app)
                .iter()
                .map(|entry| create_network_host(&entry.address, entry.port))
                .collect(),
            cached: crate::network::cached_discovered_hosts(),
        }
    }

    fn cached(&self, host: &NetworkHost) -> Option<ShareListResult> {
        let hostname = Self::hostname_of(host);
        let guest =
            crate::commands::network::guest_attempt_for(&self.app, &hostname, host.ip_address.as_deref(), host.port);
        crate::network::smb_client::cached_listing(&host.id, guest)
    }

    fn list<'a>(
        &'a self,
        host: &'a NetworkHost,
        freshness: Freshness,
    ) -> Pin<Box<dyn Future<Output = Result<ShareListResult, ShareListError>> + Send + 'a>> {
        Box::pin(async move {
            if freshness == Freshness::Fresh {
                crate::network::smb_client::invalidate_cache(&host.id);
            }
            let hostname = Self::hostname_of(host);
            let listed = crate::commands::network::ListedHost {
                host_id: &host.id,
                hostname: &hostname,
                ip_address: host.ip_address.as_deref(),
                port: host.port,
            };
            crate::commands::network::list_host_shares(&self.app, listed, None, None).await
        })
    }
}

/// Registers the `network` volume. Once, at startup: it lists nothing until a pane opens
/// a share list, so registering costs no dial.
pub(crate) fn register(app: &tauri::AppHandle) {
    let shares = Arc::new(AppServerShares { app: app.clone() });
    crate::file_system::volume::manager::get_volume_manager()
        .register(NETWORK_VOLUME_ID, Arc::new(SmbServerVolume::new(shares)));
}

#[cfg(test)]
#[path = "share_list_volume_test.rs"]
mod tests;
