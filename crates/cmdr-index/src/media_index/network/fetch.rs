//! The byte-fetch seam for network enrichment: read one image's compressed bytes off
//! an opted-in network volume, bounded against an indefinitely-blocking transport.
//!
//! ## Byte-fetch decision (plan M1): the app's own session first, OS mount as fallback
//!
//! Media enrichment MUST read image bytes off the wire — unlike `importance/`, which
//! never does. Two fetchers behind one [`ByteFetcher`] seam, picked per pass by
//! `Volume::supports_local_fs_access()` (the same predicate the archive backend uses
//! for its local-vs-remote byte source):
//!
//! - [`VolumeByteFetcher`] for a volume the app holds its OWN transport session to
//!   (a Direct-smb2 `SmbVolume`): reads via `Volume::open_read_stream_for_scan` (over
//!   SMB, small hinted files come from the scan-session connection pool, so the
//!   parallel pass's fan-out reads genuinely overlap).
//!   The OS mount is deliberately avoided there: macOS TCC ("network volumes")
//!   denies `std::fs` on `/Volumes/…` for unsigned dev binaries and can regress
//!   per-binary on rebuilds, while the direct session is the connection Cmdr
//!   already owns, health-checks, and auto-reconnects — and it yields TYPED errors
//!   (`VolumeError::DeviceDisconnected`), so pause-vs-skip classification doesn't
//!   ride errno guesswork.
//! - [`FsByteFetcher`] for mount-only volumes (no Direct session): `std::fs` on the
//!   OS mount path, failures classified by typed errno ([`classify_io_error`]).
//!
//! **Non-blocking discipline.** A network read can block indefinitely on a dead/hung
//! transport. [`FsByteFetcher`] runs the read on a throwaway thread and waits with a
//! timeout; [`VolumeByteFetcher`] wraps the async read in `tokio::time::timeout`. A
//! timeout returns [`FetchError::Disconnected`] rather than wedging the pass.
//! Critically, the fetch happens in the enrich layer — NOT on the serialized Vision
//! OCR worker thread — so a hung transport can never stall OCR of other (local)
//! volumes.

use cmdr_fs::app_path::{AppPath, AppPathBuf, Namer, NotAnAppPath, PathFlavor};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cmdr_fs::os_error::OsErrorClass;

use crate::media_index::MAX_IMAGE_BYTES;

/// Why a byte fetch didn't yield bytes. Typed (never string-matched — `no-string-
/// matching`): the caller branches on the variant to decide pause-vs-skip-vs-fail.
#[derive(Debug)]
pub enum FetchError {
    /// The mount is gone or hung (a timeout, or a non-`NotFound` I/O error). NOT a bad
    /// file — the caller PAUSES the volume (keeps rows, no GC, no `Failed`).
    Disconnected(String),
    /// The file no longer exists at the mount. A vanished source — the caller skips it
    /// (a completed scan's GC collects it), never marks it `Failed`.
    NotFound,
    /// The file is larger than [`MAX_IMAGE_BYTES`]; skipped without reading.
    TooLarge,
    /// The file exists but its BYTES couldn't be read (permission denied, a corrupt
    /// region, an exotic per-file refusal) — a per-file fault, NOT a transport one.
    /// The caller skips it, counts it, and logs the total at pass end; it never
    /// pauses the pass (that's reserved for a typed disconnect) and never marks the
    /// row `Failed` (which is for a good read with a bad decode). The M1 hazard this
    /// variant closes: a TCC `EPERM` on the OS mount classifying as "disconnected"
    /// paused the whole NAS pass after zero images.
    Unreadable(String),
}

/// Reads one image's compressed bytes for enrichment. Behind a trait so the enrich
/// core is testable with a scripted fake (no real mount, no I/O).
pub trait ByteFetcher: Send + Sync {
    /// Read the bytes at `os_path`, giving up after `timeout`. `size_hint` is the
    /// index's last-known file size: the direct fetcher forwards it to
    /// `Volume::open_read_stream_with_hint` (SMB's one-round-trip compound read for
    /// small files) and short-circuits an over-cap file without reading; a stale
    /// hint is harmless (the read self-corrects).
    fn fetch(&self, os_path: &str, size_hint: Option<u64>, timeout: Duration) -> Result<Vec<u8>, FetchError>;
}

/// The production fetcher: `std::fs::read` on the OS mount path, on a throwaway thread
/// bounded by a timeout so a hung mount can't block the pass.
pub(crate) struct FsByteFetcher;

impl ByteFetcher for FsByteFetcher {
    fn fetch(&self, os_path: &str, _size_hint: Option<u64>, timeout: Duration) -> Result<Vec<u8>, FetchError> {
        let (tx, rx) = mpsc::channel();
        let path = os_path.to_string();
        // A detached reader thread: if the mount hangs, we abandon it on timeout rather
        // than blocking. At most one lingering thread per hung mount (we pause on the
        // first timeout), which self-clears when the mount recovers or the app exits.
        thread::Builder::new()
            .name("media-net-fetch".into())
            .spawn(move || {
                let _ = tx.send(read_bounded(&path));
            })
            .map_err(|e| FetchError::Disconnected(format!("spawn fetch thread: {e}")))?;

        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            // A timeout is the hung-mount signal: treat as disconnected (pause), not a
            // bad file.
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(FetchError::Disconnected(format!("read timed out after {timeout:?}")))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(FetchError::Disconnected("fetch thread dropped".to_string()))
            }
        }
    }
}

/// Read a file, capping at [`MAX_IMAGE_BYTES`] and classifying failures by I/O error
/// KIND (a typed errno, not a message match — see [`classify_io_error`]).
fn read_bounded(path: &str) -> Result<Vec<u8>, FetchError> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return Err(classify_io_error(&e, path, "open")),
    };
    // Read at most MAX_IMAGE_BYTES + 1 so we can tell "exactly at the cap" from "over".
    let mut buf = Vec::new();
    let mut limited = file.take(MAX_IMAGE_BYTES + 1);
    match limited.read_to_end(&mut buf) {
        Ok(_) => {
            if buf.len() as u64 > MAX_IMAGE_BYTES {
                Err(FetchError::TooLarge)
            } else {
                Ok(buf)
            }
        }
        Err(e) => Err(classify_io_error(&e, path, "read")),
    }
}

/// Classify an OS-mount I/O failure by its errno's `OsErrorClass` (never a message match):
/// `NotFound` for a vanished source, [`FetchError::Disconnected`] ONLY for
/// transport-loss errnos (the mount is gone), and everything else — permission
/// denied, `EIO` on a corrupt region, `EISDIR` — as a per-file
/// [`FetchError::Unreadable`]. The bias is deliberate: misreading a dead mount as
/// per-file skips burns through the list and completes honestly (re-enriched on the
/// next scan), while misreading a per-file fault as "disconnected" pauses the whole
/// pass against a condition that never clears — the M1 TCC-EPERM stall.
fn classify_io_error(e: &std::io::Error, path: &str, op: &str) -> FetchError {
    if e.kind() == std::io::ErrorKind::NotFound {
        return FetchError::NotFound;
    }
    let transport_loss = matches!(
        OsErrorClass::of_io(e),
        Some(
            OsErrorClass::TimedOut
                | OsErrorClass::NotConnected
                | OsErrorClass::NetworkDown
                | OsErrorClass::NetworkUnreachable
                | OsErrorClass::HostDown
                | OsErrorClass::HostUnreachable
                | OsErrorClass::NoDevice
                | OsErrorClass::DeviceNotConfigured
                | OsErrorClass::Stale
        )
    );
    if transport_loss {
        FetchError::Disconnected(format!("{op} '{path}': {e}"))
    } else {
        FetchError::Unreadable(format!("{op} '{path}': {e}"))
    }
}

/// The direct-session fetcher: read through the `Volume` trait (the app's OWN smb2
/// session — no TCC, no foreign-mount surprises), bridging the async read onto the
/// caller's (blocking) thread. Constructed per pass for volumes with
/// `supports_local_fs_access() == false`; the enrichment fetch runs on a
/// `spawn_blocking` / plain worker thread, never a runtime worker, so `block_on`
/// can't reenter the executor (the same bridge as the archive backend's
/// `VolumeByteSource`).
pub struct VolumeByteFetcher {
    volume: std::sync::Arc<dyn cmdr_fs::volume::Volume>,
    /// The tokio runtime the async volume read runs under; captured at
    /// construction (inside the runtime context) because the fetch itself runs on
    /// plain threads with no ambient runtime.
    handle: tokio::runtime::Handle,
}

impl VolumeByteFetcher {
    /// A fetcher reading through `volume` on the runtime behind `handle`.
    pub fn new(volume: std::sync::Arc<dyn cmdr_fs::volume::Volume>, handle: tokio::runtime::Handle) -> Self {
        Self { volume, handle }
    }
}

impl ByteFetcher for VolumeByteFetcher {
    fn fetch(&self, os_path: &str, size_hint: Option<u64>, timeout: Duration) -> Result<Vec<u8>, FetchError> {
        // A known-oversized file is skipped without touching the wire at all.
        if size_hint.is_some_and(|s| s > MAX_IMAGE_BYTES) {
            return Err(FetchError::TooLarge);
        }
        let volume = std::sync::Arc::clone(&self.volume);
        // `Volume` impls accept mount-absolute app paths (SmbVolume strips its mount
        // prefix; LocalPosix resolves absolutes), so the enrich layer's os-joined path
        // passes through as one typed value. Text that is no app path (empty, relative,
        // a `..` name, a NUL) names no file a read could reach: skipped and counted.
        let path = mount_absolute_app_path(os_path)
            .map_err(|why| FetchError::Unreadable(format!("not an app path ({why:?})")))?;
        self.handle.block_on(async move {
            match tokio::time::timeout(timeout, read_via_volume(volume.as_ref(), &path, size_hint)).await {
                Ok(result) => result,
                // The outer timeout is the hung-transport backstop: classify as a
                // disconnect (pause), never a per-file fault. Dropping the timed-out
                // future cancels the in-flight SMB read (safe on smb2; enrichment
                // never runs on MTP, where a dropped round trip wedges the device).
                Err(_) => Err(FetchError::Disconnected(format!(
                    "volume read timed out after {timeout:?}"
                ))),
            }
        })
    }
}

/// Drain one file through `Volume::open_read_stream_for_scan` (the background bulk
/// read: over SMB, small hinted files come from the scan-session connection pool so
/// parallel prefetch reads actually overlap), capping at [`MAX_IMAGE_BYTES`] (stop
/// draining and skip, rather than buffering a pathological file), classifying
/// failures by TYPED `VolumeError` variant.
async fn read_via_volume(
    volume: &dyn cmdr_fs::volume::Volume,
    path: &AppPath,
    size_hint: Option<u64>,
) -> Result<Vec<u8>, FetchError> {
    let mut stream = volume
        .open_read_stream_for_scan(path, size_hint)
        .await
        .map_err(classify_volume_error)?;
    // Pre-size from the hint (capped), so a typical photo lands in one allocation.
    let mut buf = Vec::with_capacity(size_hint.unwrap_or(0).min(MAX_IMAGE_BYTES) as usize);
    while let Some(chunk) = stream.next_chunk().await {
        let chunk = chunk.map_err(classify_volume_error)?;
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > MAX_IMAGE_BYTES {
            // Dropping the stream cancels the producer (SmbReadStream sends its
            // cancel signal on drop).
            return Err(FetchError::TooLarge);
        }
    }
    Ok(buf)
}

/// The mount-absolute path text the enrich layer joined ([`os_join`]: a volume's root
/// plus an index-relative row path) as the app path the volume reads.
///
/// Every escape-range code point in the text is LITERAL (the index's rows are lossy
/// text, and no escape exists before they go live), escaped through the host's namer:
/// the spelling the bridge's `legacy_unchecked` gave the same text, so the volume sees
/// the bytes it did. Read in the host's grammar first, then as a POSIX path, the
/// in-memory and test volumes' `/` root, which is a valid value on every host but the
/// Windows grammar refuses (`WrongFlavor`). Normalized on the way (`//`, `.`, and a
/// trailing `/` fold away), which only respells a path the join never makes.
fn mount_absolute_app_path(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    match AppPathBuf::from_external_text(text, PathFlavor::HOST, Namer::HOST) {
        Err(NotAnAppPath::WrongFlavor) => AppPathBuf::from_external_text(text, PathFlavor::Posix, Namer::HOST),
        read => read,
    }
}

/// Classify a `VolumeError` from the direct read path — TYPED variants only, the
/// whole point of reading through the session Cmdr owns:
///
/// - `NotFound` ⇒ a vanished source (skip; GC collects it after a completed scan).
/// - `DeviceDisconnected` / `ConnectionTimeout` ⇒ the transport is gone (pause the
///   pass; the registration bus resumes it on reconnect).
/// - Everything else (`PermissionDenied`, `IsADirectory`, `IoError`, and MTP's
///   `DeviceSessionReset`, which its docs forbid mapping to a disconnect) ⇒ a
///   per-file [`FetchError::Unreadable`]: skip-and-count, never a pause.
fn classify_volume_error(e: cmdr_fs::volume::VolumeError) -> FetchError {
    use cmdr_fs::volume::VolumeError;
    match e {
        VolumeError::NotFound(_) => FetchError::NotFound,
        VolumeError::DeviceDisconnected(msg) => FetchError::Disconnected(msg.to_string()),
        VolumeError::ConnectionTimeout(msg) => FetchError::Disconnected(format!("connection timeout: {msg}")),
        other => FetchError::Unreadable(other.to_string()),
    }
}

/// Map a volume's index-relative path (`/DCIM/x.jpg`, reconstructed from the index's
/// `ROOT_ID`) to its OS-mount-absolute path by prepending the mount root. For the
/// `root`/local volume the mount root is `/`, so the index-relative path is already
/// absolute and passes through unchanged.
pub fn os_join(mount_root: &str, index_relative: &str) -> String {
    if mount_root == "/" || mount_root.is_empty() {
        return index_relative.to_string();
    }
    let trimmed = mount_root.trim_end_matches('/');
    // `index_relative` starts with '/', so this yields `<mount>/…` with no double slash.
    format!("{trimmed}{index_relative}")
}

/// The inverse of [`os_join`]: map an OS-mount folder path back into a volume's
/// index-path space by stripping the mount root, so the privacy retro-delete can match
/// it against the stored (index-relative) rows. `Some("/Photos")` for
/// `/Volumes/testshare/Photos` under mount `/Volumes/testshare`; `Some("")` for the mount root
/// itself (the whole volume); the OS folder passes through unchanged on a `root`/local
/// volume (mount root `/`). `None` when the folder isn't under this volume's mount at
/// all (a different volume) — the caller skips it.
///
/// The mount root matches with the same case and Unicode folding as the exclusion veto
/// (`config::path_is_within`), one component at a time: folding can change a component's
/// byte length but never adds or removes a `/`, so the rest keeps the folder's own bytes.
pub(crate) fn os_folder_to_index_prefix(folder: &str, mount_root: &str) -> Option<String> {
    if mount_root == "/" || mount_root.is_empty() {
        return Some(folder.to_string());
    }
    let trimmed = mount_root.trim_end_matches('/');
    if folder == trimmed {
        return Some(String::new());
    }
    if let Some(rest) = folder.strip_prefix(trimmed).filter(|rest| rest.starts_with('/')) {
        return Some(rest.to_string());
    }
    let fold = crate::indexing::store::normalize_for_comparison;
    let mut folder_parts = folder.split('/');
    for root_part in trimmed.split('/') {
        let folder_part = folder_parts.next()?;
        if folder_part != root_part && fold(folder_part) != fold(root_part) {
            return None;
        }
    }
    let rest: Vec<&str> = folder_parts.collect();
    Some(if rest.is_empty() {
        String::new()
    } else {
        format!("/{}", rest.join("/"))
    })
}

/// A scripted fetcher for tests: maps an OS path to bytes, or to a disconnect, so the
/// enrich core's pause/resume paths run with no real mount.
#[cfg(test)]
pub(crate) struct FakeByteFetcher {
    bytes: std::collections::HashMap<String, Vec<u8>>,
    disconnected: std::collections::HashSet<String>,
    unreadable: std::collections::HashSet<String>,
}

#[cfg(test)]
impl Default for FakeByteFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FakeByteFetcher {
    pub fn new() -> Self {
        Self {
            bytes: std::collections::HashMap::new(),
            disconnected: std::collections::HashSet::new(),
            unreadable: std::collections::HashSet::new(),
        }
    }

    /// Script bytes for an OS path.
    pub fn with_bytes(mut self, os_path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.bytes.insert(os_path.into(), bytes.into());
        self
    }

    /// Script a disconnect (unmount) for an OS path.
    pub fn disconnect_on(mut self, os_path: impl Into<String>) -> Self {
        self.disconnected.insert(os_path.into());
        self
    }

    /// Script a per-file read failure (permission denied and friends) for an OS path.
    pub fn unreadable_on(mut self, os_path: impl Into<String>) -> Self {
        self.unreadable.insert(os_path.into());
        self
    }
}

#[cfg(test)]
impl ByteFetcher for FakeByteFetcher {
    fn fetch(&self, os_path: &str, _size_hint: Option<u64>, _timeout: Duration) -> Result<Vec<u8>, FetchError> {
        if self.disconnected.contains(os_path) {
            return Err(FetchError::Disconnected("scripted unmount".to_string()));
        }
        if self.unreadable.contains(os_path) {
            return Err(FetchError::Unreadable("scripted per-file read failure".to_string()));
        }
        match self.bytes.get(os_path) {
            Some(b) => Ok(b.clone()),
            None => Err(FetchError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests;
