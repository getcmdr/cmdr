//! Where a copy or move lands, resolved once for the routes: anchored on its volume,
//! then routed (a `.zip`, a snapshot) or the drive itself.

use std::borrow::Cow;
use std::sync::Arc;

use cmdr_fs::app_path::AppPathBuf;
use cmdr_fs::volume::AnchorRefusal;

use cmdr_fs::volume::{RoutedKind, Volume};

use super::{dest_volume_missing, destination_takes_no_writes, host};
use crate::types::WriteOperationError;

/// Turns the destination a caller sent into the path the destination volume
/// accepts: home shortcut expanded (local only), then anchored at the volume's
/// root, as a typed app path.
///
/// The transfer dialog's destination box is VOLUME-RELATIVE (`/photos`) because
/// the volume is a separate dropdown next to it, while a pane sends the absolute
/// path it displays. `cmdr_fs::volume::anchor_at_root` folds both into the
/// absolute form by the app path's own arithmetic (❌ never `Path::join`, which on
/// Windows puts a `\` into a POSIX volume's path), and it's idempotent, so neither
/// caller has to know which one it holds.
/// Skipping it is what made a move into an SMB subfolder fail instantly
/// (ERR-XCP5Q): `SmbVolume` reads `/photos` as an absolute path outside its mount
/// and answers `NotFound` rather than guessing.
///
/// `~` expands for local volumes only, per the "never tilde-expand MTP/network
/// paths" rule: on a share, `~` is an ordinary folder name.
///
/// While escapes aren't live (`cmdr_fs::app_path::ESCAPES_LIVE`) the text's
/// escape-range code points are LITERAL, each name spelled through the listed-name
/// door, so the place is the one the `std::path` anchor named, byte for byte.
///
/// # Errors
///
/// [`AnchorRefusal`]: text naming a place on another volume (its own scheme, drive,
/// or UNC root), or holding a `..`, which the app resolves on its volume before a
/// start sends the box (`commands/wire_path.rs::transfer_destination`).
pub fn resolve_dest_path(dest_volume: &Arc<dyn Volume>, dest_path: &str) -> Result<AppPathBuf, AnchorRefusal> {
    let expanded: Cow<'_, str> = if dest_volume.local_path().is_some() {
        Cow::Owned(cmdr_fs::tilde::expand_tilde(dest_path))
    } else {
        Cow::Borrowed(dest_path)
    };
    let root = dest_volume.root();
    match cmdr_fs::volume::anchor_at_root(root, &literal_names(&expanded)) {
        // A local volume's destination may come as the OS spells it (a native `C:\x\dst` an
        // agent or a suite hands over): read through the native door and anchored again,
        // as the `std::path` anchor took it. On macOS and Linux a native path is app text
        // already, so this answers nothing the anchor above didn't.
        Err(refused) if dest_volume.local_path().is_some() => {
            match AppPathBuf::from_native_unicode(std::path::Path::new(expanded.as_ref())) {
                Ok(native) => cmdr_fs::volume::anchor_at_root(root, native.as_str()),
                Err(_) => Err(refused),
            }
        }
        anchored => anchored,
    }
}

/// `text` with each name spelled as a listing spells it while escapes aren't live
/// (`AppName::from_listed_text`): an escape-range code point is literal text.
/// Separators, and the empty, `.`, and `..` names the anchor answers itself, stay
/// as they are. Borrowed for text holding no escape-range code point (nearly all).
fn literal_names(text: &str) -> Cow<'_, str> {
    if cmdr_fs::app_path::ESCAPES_LIVE || !text.chars().any(cmdr_fs::app_path::is_escape) {
        return Cow::Borrowed(text);
    }
    let names: Vec<Cow<'_, str>> = text
        .split('/')
        .map(|name| match cmdr_fs::app_path::AppName::from_listed_text(name) {
            Ok(Cow::Owned(spelled)) => Cow::Owned(spelled.into_string()),
            _ => Cow::Borrowed(name),
        })
        .collect();
    Cow::Owned(names.join("/"))
}

/// A destination the anchor refused, as a write start reports it: the place the
/// box named isn't one on this volume, as the app's own resolve words it
/// (`commands/wire_path.rs::DestinationRefusal::into_write_error`).
pub(super) fn no_such_destination(dest_path: &str) -> WriteOperationError {
    WriteOperationError::DestinationNotFound {
        path: dest_path.to_string(),
    }
}

/// A transfer's destination, resolved: the volume that serves it, the route it took
/// (`None` for the drive itself), and the path as the routes read it.
pub(super) struct ResolvedDestination {
    pub(super) volume: Arc<dyn Volume>,
    pub(super) routed: Option<RoutedKind>,
    pub(super) path: AppPathBuf,
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
        let path = resolve_dest_path(&drive, &dest_path).map_err(|_| no_such_destination(&dest_path))?;
        let resolved = volumes.resolve(dest_volume_id, &path).await;
        let Some(volume) = resolved.volume else {
            return Err(dest_volume_missing(self.host(), dest_volume_id, path.as_str()).await);
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
