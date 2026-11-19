//! What a successful mutation means for the listing cache.
//!
//! `Volume::notify_mutation` is fire-and-forget: after a create, delete, rename,
//! or `write_from_stream` lands, the backend tells the cache so the pane updates
//! without waiting for a watcher event (SMB and MTP watchers are lossy under
//! load). The trait's default is a no-op — `cmdr-fs` doesn't know this cache
//! exists — so each backend supplies the meaning. This module supplies it for
//! the backends whose paths are real `std::fs` paths.

use std::borrow::Cow;
use std::path::Path;

use super::reading::{ListingDir, located, single_entry};
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::host::listings::ListingHost;
use cmdr_fs::volume::{DirectoryChange, MutationEvent};

/// Patches the listing cache after a mutation on a LOCAL-FS-backed volume: stats
/// the affected entry through `std::fs` and turns it into the right
/// [`DirectoryChange`], which `listings` hands on (in Cmdr, to the composer:
/// index, badges, then panes).
///
/// This is what `Volume::notify_mutation` means for a backend whose paths are
/// real `std::fs` paths. It lives here rather than on the trait because
/// `cmdr-fs` knows nothing about the listing cache; a backend on a protocol that
/// can answer `get_metadata` faster than `std::fs` would (SMB, MTP) builds its
/// own entry and calls its `ListingHost::directory_changed` directly instead.
///
/// `parent_path` is a native path (on Windows either spelling, `C:\x` or the app path's
/// `C:/x`). Fire-and-forget: a failed stat logs and drops the patch, because the mutation
/// it follows has already succeeded and a watcher event may still land.
pub fn patch_listing_after_local_mutation(
    listings: &dyn ListingHost,
    volume_id: &str,
    parent_path: &Path,
    mutation: MutationEvent,
) {
    match located(parent_path) {
        Ok((os, app)) => patch_after_local_mutation(
            listings,
            volume_id,
            parent_path,
            &ListingDir::new(os, app.map(Cow::Owned)),
            mutation,
        ),
        Err(e) => log::warn!(
            "notify_mutation: no local path for {}: {e}",
            LogPrivate::path(parent_path)
        ),
    }
}

/// [`patch_listing_after_local_mutation`] for the folder `parent`, which the listing host
/// hears of as `parent_path`: `ListingHost` still takes `std::path` for an app path, and goes
/// typed with the app's listing host (plan milestone 10c).
pub(crate) fn patch_after_local_mutation(
    listings: &dyn ListingHost,
    volume_id: &str,
    parent_path: &Path,
    parent: &ListingDir<'_>,
    mutation: MutationEvent,
) {
    let restat = |name: &str| match parent.named_child(name) {
        Some((os, row)) => single_entry(&os, row).map_err(|e| {
            log::warn!("notify_mutation: couldn't stat {}: {}", LogPrivate::path(&os), e);
        }),
        None => {
            log::warn!(
                "notify_mutation: no local path for the name {} in {}",
                LogPrivate::name(name),
                LogPrivate::path(parent_path)
            );
            Err(())
        }
    };
    match mutation {
        MutationEvent::Created(ref name) | MutationEvent::Modified(ref name) => {
            if let Ok(entry) = restat(name) {
                let change = if matches!(mutation, MutationEvent::Created(_)) {
                    DirectoryChange::Added(entry)
                } else {
                    DirectoryChange::Modified(entry)
                };
                listings.directory_changed(volume_id, parent_path, change);
            }
        }
        MutationEvent::Deleted(name) => {
            listings.directory_changed(volume_id, parent_path, DirectoryChange::Removed(name));
        }
        MutationEvent::Renamed { from, to } => {
            if let Ok(entry) = restat(&to) {
                listings.directory_changed(
                    volume_id,
                    parent_path,
                    DirectoryChange::Renamed {
                        old_name: from,
                        new_entry: entry,
                    },
                );
            }
        }
    }
}
