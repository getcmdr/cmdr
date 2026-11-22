//! Where a copy or move lands, resolved once for the routes: anchored on its volume,
//! then routed (a `.zip`, a snapshot) or the drive itself.

use std::sync::Arc;

use cmdr_fs::volume::{RoutedKind, Volume};

use super::{dest_volume_missing, destination_takes_no_writes, host, resolve_dest_path};
use crate::types::WriteOperationError;

/// A transfer's destination, resolved: the volume that serves it, the route it took
/// (`None` for the drive itself), and the path as the routes read it.
pub(super) struct ResolvedDestination {
    pub(super) volume: Arc<dyn Volume>,
    pub(super) routed: Option<RoutedKind>,
    pub(super) path: String,
}

impl crate::Operations {
    /// A copy's or move's destination, anchored on its volume ([`resolve_dest_path`]) and
    /// then resolved: a `.zip`-crossing one routes to the managed archive-edit driver.
    /// Refuses a volume nothing registers and a route that takes no writes (a snapshot).
    ///
    /// ❗ Anchored BEFORE the resolve: the transfer dialog's box is volume-relative, and a
    /// `.zip` boundary is confirmed by the place the path names, so `/z/box.zip` on a
    /// drive rooted at `C:/` (or a Mac volume at `/Volumes/USB`) confirmed nothing and a
    /// copy into the zip fell through to the local engine as `destination_not_a_folder`
    /// (#1369). On a root at `/` the two spellings coincide, which is why only Windows
    /// showed it.
    pub(super) async fn resolve_destination(
        &self,
        dest_volume_id: &str,
        dest_path: String,
    ) -> Result<ResolvedDestination, WriteOperationError> {
        let volumes: &dyn host::OperationVolumes = self.host().volumes();
        let Some(drive) = volumes.get(dest_volume_id) else {
            return Err(dest_volume_missing(self.host(), dest_volume_id, &dest_path).await);
        };
        let anchored = resolve_dest_path(&drive, dest_path.clone());
        let resolved = volumes.resolve(dest_volume_id, &anchored).await;
        let path = anchored.into_os_string().into_string().unwrap_or(dest_path);
        let Some(volume) = resolved.volume else {
            return Err(dest_volume_missing(self.host(), dest_volume_id, &path).await);
        };
        if resolved.routed.is_some_and(|kind| !kind.takes_writes()) {
            return Err(destination_takes_no_writes(&volume));
        }
        Ok(ResolvedDestination {
            volume,
            routed: resolved.routed,
            path,
        })
    }
}
