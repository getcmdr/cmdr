//! Opening a directory by a path that may not be its volume's own spelling.
//!
//! A pane's path usually came out of a listing, so it's the volume's exact
//! spelling. One that didn't (typed, pasted, a restored tab, a favorite, an MCP
//! `nav_to_path`, or a pane carried over from the macOS kernel mount, which
//! decomposes every name) misses on a backend that matches names byte-for-byte
//! (SMB, ERR-VETBX). So a pane listing asks as given and, only on `NotFound`,
//! asks the volume where that path is stored (`Volume::find_stored_spelling`)
//! and lists THAT. The listing then carries the stored spelling: its cache key,
//! its watcher key, and every child path are the volume's own bytes, and the
//! pane adopts the spelling from `listing-complete`.
//!
//! Ask Cmdr's live `list_dir` read opens a folder by a model-typed path, which
//! is the same seam, so it lists through [`entries_as_stored`].
//!
//! Paths from outside Cmdr that name FILES (a Finder drag-in, a paste of files
//! copied in Finder) are respelled once where they enter, by
//! [`stored_spellings`], before any operation sees them.
//!
//! ❌ Only at those two seams. A delete walker, a copy scan, or a refresh lists
//! paths that came out of a listing, and there a miss means the directory is
//! gone: resolving it could hand the walker a look-alike twin.

use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::Inner;
use crate::pipeline::ReadBy;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{ListingProgress, Volume, VolumeError};

/// A directory listing, and the path it lists as its volume spells it.
pub(crate) struct Listed {
    /// `path` as asked, or the volume's own spelling when that one missed.
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
}

impl Listed {
    /// The volume's spelling when it differs from what was asked, for the
    /// frontend to adopt; `None` for the common case, a path that listed as given.
    pub(crate) fn stored_spelling_of(&self, asked: &Path) -> Option<PathBuf> {
        (self.path != asked).then(|| self.path.clone())
    }
}

/// Lists `path` for a pane, falling back to the volume's stored spelling of it
/// when `path` as given isn't there.
///
/// Costs nothing extra on the happy path. A miss costs the resolve (a round trip
/// or two, plus a parent listing per wrong component, free when a pane already
/// shows that parent), then the listing itself. `cancel` reaches both.
pub(crate) async fn list_as_stored(
    volume: &dyn Volume,
    path: &Path,
    on_progress: Option<&(dyn Fn(ListingProgress) + Sync)>,
    cancel: Option<&CancellationToken>,
) -> Result<Listed, VolumeError> {
    let missed = match volume
        .list_directory_with_cancel(&AppPath::legacy_unchecked(path), on_progress, cancel)
        .await
    {
        Ok(entries) => {
            return Ok(Listed {
                path: path.to_path_buf(),
                entries,
            });
        }
        Err(e @ VolumeError::NotFound(_)) => e,
        Err(e) => return Err(e),
    };
    let Some(stored) = volume
        .find_stored_spelling(&AppPath::legacy_unchecked(path), cancel)
        .await?
    else {
        return Err(missed);
    };
    log::debug!(
        "list_as_stored: {} is stored as {}",
        LogPrivate::dir_path(path),
        LogPrivate::dir_path(&stored)
    );
    let entries = volume.list_directory_with_cancel(&stored, on_progress, cancel).await?;
    Ok(Listed {
        path: stored.as_std_legacy().into_owned(),
        entries,
    })
}

/// The entries of the folder at `path`, opened the way a pane opens a typed
/// path ([`list_as_stored`]), for a reader outside the engine that doesn't keep
/// a listing (Ask Cmdr's live `list_dir`).
///
/// # Errors
///
/// The volume's own error for `path` as given when no other spelling exists, or
/// the error of the resolve or of the second listing.
pub async fn entries_as_stored(
    volume: &dyn Volume,
    path: &Path,
    cancel: Option<&CancellationToken>,
) -> Result<Vec<FileEntry>, VolumeError> {
    list_as_stored(volume, path, None, cancel)
        .await
        .map(|listed| listed.entries)
}

/// Each of `paths` in the spelling `volume` stores it under, for paths that
/// entered from outside Cmdr: a Finder drag-in or a paste of files copied in
/// Finder, both spelled by the macOS kernel mount.
///
/// Called ONCE where those paths enter (`commands::file_system::stored_spellings`),
/// so the scan preview, the conflict check, the transfer, and its journal all
/// carry one spelling. A path with no other stored spelling, or with two
/// look-alikes and no exact match, stays as given: every operation then means
/// those exact bytes, so nothing ever lands on a guessed twin.
///
/// An all-ASCII path is never asked about: it has one Unicode form, and the kernel
/// mount keeps the server's case, so these paths can only differ in form. That
/// keeps a drop of a thousand plain names from costing a thousand round trips.
pub async fn stored_spellings(volume: &dyn Volume, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut stored = Vec::with_capacity(paths.len());
    for path in paths {
        if path.as_os_str().is_ascii() {
            stored.push(path);
            continue;
        }
        match volume
            .find_stored_spelling(&AppPath::legacy_unchecked(&path), None)
            .await
        {
            Ok(Some(spelling)) => {
                log::debug!(
                    "stored_spellings: {} is stored as {}",
                    LogPrivate::path(&path),
                    LogPrivate::path(&spelling)
                );
                stored.push(spelling.as_std_legacy().into_owned());
            }
            Ok(None) => stored.push(path),
            Err(e) => {
                log::debug!(
                    "stored_spellings: keeping {} as given: {}",
                    LogPrivate::path(&path),
                    LogError(&e)
                );
                stored.push(path);
            }
        }
    }
    stored
}

impl Inner {
    /// Re-reads every pane listing on `volume_id` through the backend now serving
    /// it, landing each on its stored spelling.
    ///
    /// For a backend swap, the macOS kernel mount handing a share to a direct SMB
    /// connection: the open listings were read by the OLD backend, so their paths
    /// and every entry path carry the kernel's spelling (decomposed), which the new
    /// byte-exact backend misses. Until they're re-read, opening, copying, or
    /// deleting an accented entry in an open pane goes nowhere. A listing whose own
    /// path changed spelling is re-keyed, so the watcher's reports reach it, and its
    /// pane is told (`listing-respelled`); the entries arrive as an ordinary diff.
    ///
    /// Fire-and-forget, one task per listing, each holding its directory's refresh
    /// turn so it can't interleave with a watcher's re-read of the same directory.
    pub(crate) fn respell_listings_on_volume(self: &std::sync::Arc<Self>, volume_id: &str) {
        for (listing_id, path) in self.find_listings_on_volume(volume_id) {
            // A strong handle: the respell ends.
            let engine = std::sync::Arc::clone(self);
            let volume_id = volume_id.to_string();
            self.runtime
                .spawn(async move { engine.respell_listing(volume_id, listing_id, path).await });
        }
    }

    /// Re-reads a just-cached pane listing when the backend that read it no longer
    /// serves `volume_id`.
    ///
    /// `respell_listings_on_volume` sees only the listings cached when the swap
    /// lands, and a pane landing on a kernel-mounted share starts the upgrade at the
    /// same moment it starts listing (`network::smb_pane_upgrade`), so the swap
    /// routinely lands mid-read. Checked after the cache insert, this closes that
    /// window: a swap before the insert is seen here, one after it sees the listing.
    /// Not for a routed listing (an archive), whose volume is never the registered one.
    pub(crate) fn respell_if_read_by_a_replaced_backend(
        self: &std::sync::Arc<Self>,
        volume_id: &str,
        listing_id: &str,
        path: &Path,
        read_by: &std::sync::Arc<dyn Volume>,
    ) {
        let serving = self.volumes.get(volume_id);
        if serving.is_some_and(|serving| !std::sync::Arc::ptr_eq(&serving, read_by)) {
            log::debug!(
                "listing {listing_id} was read by a backend replaced mid-read; re-reading {}",
                LogPrivate::dir_path(path)
            );
            // A strong handle: the respell ends.
            let engine = std::sync::Arc::clone(self);
            let (volume_id, listing_id, path) = (volume_id.to_string(), listing_id.to_string(), path.to_path_buf());
            self.runtime
                .spawn(async move { engine.respell_listing(volume_id, listing_id, path).await });
        }
    }

    /// One listing's respell; `pub(super)` so a test can await it directly.
    pub(crate) async fn respell_listing(&self, volume_id: String, listing_id: String, path: PathBuf) {
        let _turn = self.refresh_turn(&volume_id, &path).await;
        let resolved = self.volumes.resolve(&volume_id, &path).await;
        let is_routed = resolved.is_routed();
        let Some(volume) = resolved.volume else {
            return;
        };
        let generation = self.next_read_generation();
        let listed = match list_as_stored(volume.as_ref(), &path, None, None).await {
            Ok(listed) => listed,
            Err(e) => {
                // The pane keeps what it has; its next navigation or refresh asks again.
                log::debug!(
                    "respell_listing: couldn't re-read {}: {}",
                    LogPrivate::dir_path(&path),
                    LogError(&e)
                );
                return;
            }
        };
        let Listed { path: stored, entries } = listed;
        let read_by = ReadBy::Volume {
            volume: &volume,
            routed: is_routed,
        };
        let prepared = self.prepare(read_by, &volume_id, &stored, entries).await;
        // The diff tells the pane what came and went, but it matches rows by name, so
        // it can't see every path changing spelling. The replace step writes the
        // entries whole whatever the diff holds, so the new spellings land, and the
        // pane refetches its rows when it adopts the new spelling.
        self.update_listing_entries(&listing_id, prepared, generation);
        if stored != path {
            self.rekey_listing(&listing_id, &volume_id, &stored);
        }
    }

    /// Moves `listing_id` to `stored`, the same directory in its volume's spelling,
    /// and tells its pane. A no-op when the pane closed meanwhile.
    fn rekey_listing(&self, listing_id: &str, volume_id: &str, stored: &Path) {
        use cmdr_fs::ignore_poison::RwLockIgnorePoison;

        // Built before taking the cache: it reads the volume registry.
        let key = self.listing_path(volume_id, stored);
        {
            let mut cache = self.cache.write_ignore_poison();
            let Some(listing) = cache.get_mut(listing_id) else {
                return;
            };
            listing.path = key;
        }
        log::info!("listing {listing_id} respelled to the volume's own spelling");
        self.events
            .respelled(&crate::listing_id::ListingId::new(listing_id), stored);
    }
}

#[cfg(test)]
#[path = "foreign_path_test.rs"]
mod foreign_path_test;
