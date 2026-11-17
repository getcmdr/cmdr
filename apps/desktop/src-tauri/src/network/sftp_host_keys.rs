//! The app's answer to a backend's "have we met this server before?" question.
//!
//! SSH trust is trust-on-first-use: the security of every later connection rests
//! on recognizing the same host key next time. A backend can't own that store —
//! it outlives any one volume, it's the user's to inspect and clear, and writing
//! it durably is the app's business — so it arrives through
//! `cmdr_fs::volume::host::host_keys::HostKeys` and this is what the app
//! installs.
//!
//! ❗ **A new store rather than a widened `KnownNetworkShare`.** That type's
//! `share_name` and its guest-versus-credentials `AuthOptions` can't express key
//! or agent auth, and a host key isn't a share at all.
//!
//! ❌ **Nothing here writes `~/.ssh/known_hosts`.** The backend reads that file
//! as a fallback so a server the user's terminal already reaches doesn't ask
//! again, but it belongs to `ssh`, and a file manager appending to it is a
//! surprise nobody asked for.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use cmdr_fs::volume::host::host_keys::{HostKeyVerdict, HostKeys};
use serde::{Deserialize, Serialize};

use super::server_list_file;
use crate::ignore_poison::IgnorePoison;
use crate::state_recovery::StateFile;

/// One approved host key.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrustedHostKey {
    /// The server, as the user addressed it.
    pub host: String,
    /// Its port. Part of the identity: a jump box and a container on one machine
    /// are different servers.
    pub port: u16,
    /// The SSH key-type name (`ssh-ed25519`, `rsa-sha2-512`). ❗ Part of the key
    /// too: a server may hold several types and present any of them, so a store
    /// keyed by host alone reports a changed key on a healthy server.
    pub algorithm: String,
    /// The OpenSSH `SHA256:…` fingerprint, which is what a human compares
    /// against `ssh-keygen -lf`.
    pub fingerprint: String,
    /// When it was approved, ISO 8601, so the settings screen can show it.
    pub approved_at: String,
}

/// The whole store, as it sits on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedHostKeysStore {
    /// Every key the user has approved.
    #[serde(default, deserialize_with = "crate::state_recovery::lenient_entries")]
    pub trusted_host_keys: Vec<TrustedHostKey>,
}

/// In-memory mirror of the file, so a lookup during key exchange is not a disk
/// read.
static TRUSTED: OnceLock<Mutex<TrustedHostKeysStore>> = OnceLock::new();

/// Where the file lives once the app has told us its data dir.
static STORE_PATH: OnceLock<PathBuf> = OnceLock::new();

fn trusted() -> &'static Mutex<TrustedHostKeysStore> {
    TRUSTED.get_or_init(|| Mutex::new(TrustedHostKeysStore::default()))
}

/// Loads the store from the app's data dir into memory.
///
/// Call once at startup. A missing file is an empty store: every server then
/// reads as first contact, which costs one approval and never trusts anything it
/// shouldn't. One that doesn't read is set aside first, so the next approval's
/// save can't land over every other approval (`server_list_file::read`).
pub fn load_trusted_host_keys<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some((store, path)) = server_list_file::load(app, "known-sftp-hosts.json", StateFile::SftpHostKeys) else {
        return;
    };
    *trusted().lock_ignore_poison() = store;
    let _ = STORE_PATH.set(path);
}

/// Writes the in-memory store back, durably.
///
/// Temp file, fsync, rename, parent fsync (`server_list_file::save`): a
/// half-written trust store would either lose approvals or, worse, be
/// unparseable and read as "nothing is trusted", turning every mounted server
/// into a fresh security prompt.
fn save() {
    let Some(path) = STORE_PATH.get() else {
        // Before `load_trusted_host_keys` there's nowhere to write. A test binary
        // and a bench both land here, and both want the in-memory half to keep
        // working.
        return;
    };
    let store = trusted().lock_ignore_poison().clone();
    server_list_file::save(path, "the trusted SSH host keys", &store);
}

/// Every key the user has approved, newest write last.
///
/// For a settings screen listing what this machine trusts, which is the only way
/// [`forget_trusted_host_key`] is reachable for a server the user isn't looking
/// at right now.
pub fn list_trusted_host_keys() -> Vec<TrustedHostKey> {
    trusted().lock_ignore_poison().trusted_host_keys.clone()
}

/// Drops the approval for `(host, port, algorithm)`, so the next connection to
/// that server is first contact again.
///
/// Answers whether anything was there. ❗ App-side only, and deliberately not on
/// the `HostKeys` seam: a backend never forgets a key, it only asks and records.
///
/// ❌ Nothing here touches `~/.ssh/known_hosts`, so a server trusted through that
/// file keeps being trusted; `ssh-keygen -R` is what forgets one of those.
pub fn forget_trusted_host_key(host: &str, port: u16, algorithm: &str) -> bool {
    let removed = {
        let mut store = trusted().lock_ignore_poison();
        let before = store.trusted_host_keys.len();
        store
            .trusted_host_keys
            .retain(|entry| !(entry.host == host && entry.port == port && entry.algorithm == algorithm));
        store.trusted_host_keys.len() != before
    };
    if removed {
        save();
    }
    removed
}

/// Answers a backend's host-key questions from the durable store.
pub struct AppHostKeys;

impl HostKeys for AppHostKeys {
    fn verdict(&self, host: &str, port: u16, algorithm: &str, fingerprint: &str) -> HostKeyVerdict {
        let store = trusted().lock_ignore_poison();
        match store
            .trusted_host_keys
            .iter()
            .find(|entry| entry.host == host && entry.port == port && entry.algorithm == algorithm)
        {
            Some(entry) if entry.fingerprint == fingerprint => HostKeyVerdict::Matches,
            Some(_) => HostKeyVerdict::Changed,
            None => HostKeyVerdict::Unknown,
        }
    }

    fn trusted_algorithms(&self, host: &str, port: u16) -> Vec<String> {
        let store = trusted().lock_ignore_poison();
        let mut algorithms: Vec<String> = store
            .trusted_host_keys
            .iter()
            .filter(|entry| entry.host == host && entry.port == port)
            .map(|entry| entry.algorithm.clone())
            .collect();
        algorithms.sort();
        algorithms.dedup();
        algorithms
    }

    fn record(&self, host: &str, port: u16, algorithm: &str, fingerprint: &str) {
        {
            let mut store = trusted().lock_ignore_poison();
            let entry = TrustedHostKey {
                host: host.to_string(),
                port,
                algorithm: algorithm.to_string(),
                fingerprint: fingerprint.to_string(),
                approved_at: crate::timestamps::now_rfc3339(),
            };
            match store
                .trusted_host_keys
                .iter_mut()
                .find(|e| e.host == host && e.port == port && e.algorithm == algorithm)
            {
                // Replacing rather than appending: two entries for one triple
                // would make the verdict depend on iteration order, and a stale
                // one could answer `Matches` for a key the user replaced.
                Some(existing) => *existing = entry,
                None => store.trusted_host_keys.push(entry),
            }
        }
        save();
    }
}

#[cfg(test)]
#[path = "sftp_host_keys_test.rs"]
mod sftp_host_keys_test;
