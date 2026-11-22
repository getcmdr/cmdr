//! MTP (Media Transfer Protocol) volume implementation.
//!
//! Wraps MTP device storage as a Volume, enabling MTP browsing through
//! the standard file listing pipeline (same icons, sorting, view modes as local files).
//!
//! Split by concern like its SMB sibling: this module holds `MtpVolume` itself
//! and its path conversions, with the `impl Volume` block in `volume_impl`, byte
//! movement in `streams`, error classification in `mapping`, cancellation
//! bridging in `cancel`, and copy scanning in `scan`.

mod cancel;
mod mapping;
mod move_rename;
mod scan;
mod streams;
/// The two instruments a test outside this backend reads it with.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod volume_impl;

/// The shared `cmdr_fs::volume::conformance` promises, answered by a device.
#[cfg(all(test, feature = "virtual-device"))]
mod conformance_test;
/// `Volume::delete` stopping at one node, which MTP implements rather than inherits.
#[cfg(all(test, feature = "virtual-device"))]
mod delete_test;
/// What a volume answers once its phone's session is gone.
#[cfg(all(test, feature = "virtual-device"))]
mod disconnected_test;
/// What this backend tells the host's seams, and how often.
#[cfg(all(test, feature = "virtual-device"))]
mod host_seam_test;
/// A rename that also changes folder, beside a file holding the old name.
#[cfg(all(test, feature = "virtual-device"))]
mod move_rename_test;
/// `MtpVolume`'s identity and the path conversions every operation starts with.
#[cfg(test)]
mod path_test;
/// The `read_range` hardware benchmark. Ignored by default; needs a real phone,
/// so it takes no virtual-device gate.
#[cfg(test)]
mod read_bench;
/// The direct and windowed read paths, against a virtual device.
#[cfg(all(test, feature = "virtual-device"))]
mod read_range_test;
/// The `VolumeReadStream` → chunk-stream adapter, with no device behind it.
#[cfg(test)]
mod streams_test;
/// A folder holding two items with one name, through the `Volume` surface.
#[cfg(all(test, feature = "virtual-device"))]
mod twins_test;
/// What this volume declares about itself, and the watch-coverage gate.
#[cfg(all(test, feature = "virtual-device"))]
mod volume_impl_test;

/// The typed paths a cell hands the volume: a row's own path, and a name under a folder.
#[cfg(test)]
mod test_paths;

/// The `VolumeReadStream` → chunk-stream adapter the upload path feeds `mtp-rs`.
///
/// Published under the test gate for the cells that assert on its per-chunk
/// progress and its cancellation, which sit beside the app's transfer pipeline.
/// ❌ Not `cfg(test)` alone: that's set only while this crate builds its OWN test
/// target, so a consumer's test build would see it vanish.
#[cfg(any(test, feature = "testing"))]
pub use streams::volume_read_stream_to_chunk_stream;

use crate::connection::MtpConnectionManager;
use crate::connection::storage_path::{storage_path, storage_url, url_of};
use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor, listed_row_name, listed_text};
use std::borrow::Cow;
use std::sync::Arc;

/// A volume backed by an MTP device storage.
///
/// Every operation goes through the connection manager that attached this
/// storage, which is why the volume holds one rather than looking one up: the
/// manager owns the PTP session, the caches, and the per-device priority gate.
/// All methods are natively async, over async USB bulk transfers.
pub struct MtpVolume {
    /// Display name (typically the storage description like "Internal storage")
    name: String,
    /// The session layer this storage is served by.
    manager: Arc<MtpConnectionManager>,
    /// MTP device ID (for example, "mtp-20-5")
    pub(super) device_id: String,
    /// Storage ID within the device
    pub(super) storage_id: u32,
    /// The storage's URL, `mtp://{device_id}/{storage_id}`: this volume's root.
    root: AppPathBuf,
    /// Volume ID for listing cache lookups (format: "{device_id}:{storage_id}").
    volume_id: String,
}

impl MtpVolume {
    /// Creates a new MTP volume for a specific device storage.
    ///
    /// # Arguments
    /// * `manager` - The session layer that holds this device's PTP session
    /// * `device_id` - The MTP device ID (format: "mtp-{bus}-{address}")
    /// * `storage_id` - The storage ID within the device
    /// * `name` - Display name for the storage (for example, "Internal shared storage")
    pub fn new(manager: Arc<MtpConnectionManager>, device_id: &str, storage_id: u32, name: &str) -> Self {
        let volume_id = format!("{}:{}", device_id, storage_id);
        Self {
            name: name.to_string(),
            manager,
            device_id: device_id.to_string(),
            storage_id,
            root: root_of(device_id, storage_id),
            volume_id,
        }
    }

    /// Converts a Volume path to an MTP inner path.
    ///
    /// The path can be in several formats:
    /// - MTP URL: `mtp://mtp-0-1/65537` or `mtp://mtp-0-1/65537/DCIM/Camera`
    /// - Absolute path: `/DCIM/Camera`
    /// - Relative path: `DCIM/Camera`
    ///
    /// The MTP API expects paths relative to the storage root (for example,
    /// `DCIM/Camera`). Text work on the path's own spelling, split on `/` alone, so
    /// it answers the same on every OS.
    pub(super) fn to_mtp_path(&self, path: &AppPath) -> String {
        mtp_path_of_text(&device_text(path))
    }

    /// `path`'s storage path, the inner spelling (`/DCIM/Camera`, `/` for the root),
    /// or `None` for one no object can sit at (a `..` name).
    pub(super) fn storage_path_of(&self, path: &AppPath) -> Option<AppPathBuf> {
        storage_path(&self.to_mtp_path(path))
    }

    /// Normalizes any caller-supplied path on this volume to the canonical
    /// absolute MTP URL (`mtp://{device_id}/{storage_id}[/inner/path]`). A URL
    /// comes back as it is; `None` for a path no object can sit at.
    ///
    /// `notify_mutation` reports its PARENT path in this spelling, and so does
    /// the event loop. The app's listing cache folds it onto
    /// [`Volume::listing_path`](cmdr_fs::volume::Volume::listing_path) (the inner
    /// spelling) before matching, so a pane holding either spelling is found.
    ///
    /// The per-ENTRY paths inside a listing are the inner form
    /// (`/Documents/notes.txt`), so the `Removed` patch matches entries by NAME
    /// (see `cmdr-listing`'s `remove_entry_by_name`).
    fn to_url_path(&self, path: &AppPath) -> Option<AppPathBuf> {
        if device_text(path).starts_with("mtp://") {
            return Some(path.to_owned());
        }
        url_of(&self.device_id, self.storage_id, &self.storage_path_of(path)?)
    }
}

/// [`MtpVolume::to_mtp_path`] over a path's text, where the URL, absolute, relative,
/// and empty spellings a `std::path` caller still sends all reduce.
fn mtp_path_of_text(path_str: &str) -> String {
    // Handle MTP URLs (mtp://device-id/storage-id/optional/path)
    if let Some(without_scheme) = path_str.strip_prefix("mtp://") {
        // Parse: mtp://mtp-0-1/65537/DCIM/Camera -> DCIM/Camera
        // The format is: mtp://{device_id}/{storage_id}/{path}, so skip
        // device_id/storage_id/.
        let parts: Vec<&str> = without_scheme.splitn(3, '/').collect();
        // parts[0] = device_id (like "mtp-0-1")
        // parts[1] = storage_id (like "65537")
        // parts[2] = inner path (like "DCIM/Camera") or absent for root
        return if parts.len() >= 3 {
            parts[2].to_string()
        } else {
            String::new() // Root of storage
        };
    }

    // Handle empty or root paths
    if path_str.is_empty() || path_str == "/" || path_str == "." {
        return String::new();
    }

    // Strip leading slash if present
    path_str.strip_prefix('/').unwrap_or(path_str).to_string()
}

/// The volume root, `mtp://{device_id}/{storage_id}`. Every id `mtp_ids::device_id_for`
/// mints is a plain authority (a slug and a digest); a hand-made one holding a `/`
/// reads as `parse` reads it.
fn root_of(device_id: &str, storage_id: u32) -> AppPathBuf {
    storage_url(device_id, storage_id)
        .or_else(|| AppPathBuf::parse(&format!("mtp://{device_id}/{storage_id}"), PathFlavor::Posix).ok())
        .unwrap_or_else(|| storage_url("mtp-unnamed", storage_id).expect("a plain authority makes a storage URL"))
}

/// The text of `path` as the session layer reads it: [`device_spelling`].
fn device_text(path: &AppPath) -> Cow<'_, str> {
    device_spelling(path.as_str())
}

/// One name as a device spells it (`listed_text`, the inverse of the listed-name door,
/// #1310), or `None` for a name no device lists (a Linux name that isn't text, joined onto
/// a phone path): ❌ never its lossy spelling, which could match a sibling the device
/// really lists as `caf\u{FFFD}.txt`. What a lookup by name, a new folder's name, or an
/// upload's name takes.
fn device_name(name: &str) -> Option<Cow<'_, str>> {
    listed_text(name)
}

/// App text (a path, or one name) as a device spells it: a literal escape-range code
/// point the app escaped as the device lists it, and an escape no device spelled (joined in
/// from a Linux disk) as the native bytes' lossy text, the text a `std::path` view of it
/// gave (`listed_row_name`). A name compared with a listing's `FileEntry.name` goes through
/// here, since a device lists the literal spelling.
fn device_spelling(text: &str) -> Cow<'_, str> {
    listed_row_name(text)
}

/// A storage path (the session layer's literal text, `/DCIM/x`) as the app spells it, so a
/// device name holding an escape-range code point is the value the app hands back for its
/// row (#1310). `None` for text no app path holds.
fn app_spelling_of(storage_path: &AppPath) -> Option<AppPathBuf> {
    AppPathBuf::from_listed_text(storage_path.as_str(), PathFlavor::Posix).ok()
}
