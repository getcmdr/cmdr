//! Windows Credential Manager backend through `keyring-core` and `windows-native-keyring-store`,
//! the same API the Linux Secret Service arm uses.
//!
//! Each secret is one generic credential whose target name is `<service>:<key>`
//! (`Cmdr:license.key`, `Cmdr:smb://nas/share`), so Credential Manager lists them readably and one
//! prefix finds them all. Persisted `CRED_PERSIST_LOCAL_MACHINE`: the secret stays on this PC and
//! never roams with a domain profile, like the macOS login Keychain item.

use crate::ignore_poison::IgnorePoison;
use crate::secrets::{SecretStore, SecretStoreError};
use cmdr_fs::log_private::LogPrivate;
use keyring_core::api::CredentialStoreApi;
use keyring_core::{Entry, Error};
use log::debug;
use std::collections::HashMap;
use std::sync::Mutex;
use windows_native_keyring_store::Store;

/// The store's persistence modifier for `CRED_PERSIST_LOCAL_MACHINE`.
const PERSISTENCE: &str = "Local";

/// One operation at a time, process-wide. The crate warns that Credential Manager doesn't
/// reliably order operations on one target from different threads (a set then a get on another
/// thread can read `NoEntry`), and Cmdr's callers run on whatever thread they're on.
static SERIAL: Mutex<()> = Mutex::new(());

/// The Credential Manager target for `key` under `service`.
fn target_name(service: &str, key: &str) -> String {
    format!("{service}:{key}")
}

/// Stores secrets as generic credentials in Windows Credential Manager.
pub(crate) struct CredentialManagerStore {
    /// `Cmdr` in prod, instance-suffixed elsewhere (`service_name`); tests use their own.
    service: String,
}

impl CredentialManagerStore {
    pub(crate) fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<Entry, SecretStoreError> {
        let store = Store::new().map_err(|e| classify(key, e))?;
        let target = target_name(&self.service, key);
        let modifiers = HashMap::from([("target", target.as_str()), ("persistence", PERSISTENCE)]);
        store
            .build(&self.service, key, Some(&modifiers))
            .map_err(|e| classify(key, e))
    }
}

impl SecretStore for CredentialManagerStore {
    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        debug!("Credential Manager: setting secret for key: {}", LogPrivate::host(key));
        let _serial = SERIAL.lock_ignore_poison();
        self.entry(key)?.set_secret(value).map_err(|e| classify(key, e))
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, SecretStoreError> {
        debug!("Credential Manager: getting secret for key: {}", LogPrivate::host(key));
        let _serial = SERIAL.lock_ignore_poison();
        self.entry(key)?.get_secret().map_err(|e| classify(key, e))
    }

    fn delete(&self, key: &str) -> Result<(), SecretStoreError> {
        debug!("Credential Manager: deleting secret for key: {}", LogPrivate::host(key));
        let _serial = SERIAL.lock_ignore_poison();
        self.entry(key)?.delete_credential().map_err(|e| classify(key, e))
    }

    /// Every credential whose target starts with `<service>:`. Reads target names only, never
    /// a secret.
    fn keys(&self) -> Result<Vec<String>, SecretStoreError> {
        debug!("Credential Manager: listing keys");
        let _serial = SERIAL.lock_ignore_poison();
        let store = Store::new().map_err(|e| classify(&self.service, e))?;
        let prefix = target_name(&self.service, "");
        let pattern = format!("^{}", regex::escape(&prefix));
        let entries = store
            .search(&HashMap::from([("pattern", pattern.as_str())]))
            .map_err(|e| classify(&self.service, e))?;
        let mut keys = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry.get_attributes() {
                Ok(attributes) => {
                    if let Some(key) = attributes.get("target_name").and_then(|t| t.strip_prefix(&prefix)) {
                        keys.push(key.to_string());
                    }
                }
                // Deleted between the listing and this read: nothing left to report.
                Err(Error::NoEntry) => {}
                Err(e) => return Err(classify(&self.service, e)),
            }
        }
        Ok(keys)
    }
}

/// Maps the store's typed errors onto ours. `NoEntry` is Win32 `ERROR_NOT_FOUND`;
/// `NoStorageAccess` is `ERROR_NO_SUCH_LOGON_SESSION` (no profile loaded, so no vault), the closest
/// thing Credential Manager has to a locked keyring.
fn classify(key: &str, error: Error) -> SecretStoreError {
    match error {
        Error::NoEntry => SecretStoreError::NotFound(format!("No secret found for key: {key}")),
        Error::NoStorageAccess(e) => SecretStoreError::AccessDenied(format!("Credential Manager is out of reach: {e}")),
        other => SecretStoreError::Other(format!("Credential Manager: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store under a test-only service, with every credential it made removed on drop, so a
    /// failed assertion never leaves a test credential in the builder's Credential Manager.
    struct ScratchStore {
        store: CredentialManagerStore,
    }

    impl ScratchStore {
        fn new(tag: &str) -> Self {
            let service = format!("CmdrTest-{tag}-{}", std::process::id());
            Self {
                store: CredentialManagerStore::new(service),
            }
        }
    }

    impl Drop for ScratchStore {
        fn drop(&mut self) {
            for key in self.store.keys().unwrap_or_default() {
                let _ = self.store.delete(&key);
            }
        }
    }

    #[test]
    fn the_target_is_the_service_then_the_key() {
        assert_eq!(target_name("Cmdr", "license.key"), "Cmdr:license.key");
        assert_eq!(target_name("Cmdr-dev", "smb://nas/share"), "Cmdr-dev:smb://nas/share");
    }

    #[test]
    fn a_secret_round_trips_through_the_real_credential_manager() {
        let scratch = ScratchStore::new("roundtrip");
        let store = &scratch.store;
        let value = b"test_user_1\0not-a-real-password\xff\x00binary";

        store.set("smb://testnas.local/share", value).expect("set");
        assert_eq!(store.get("smb://testnas.local/share").expect("get"), value);

        store.set("smb://testnas.local/share", b"replaced").expect("overwrite");
        assert_eq!(store.get("smb://testnas.local/share").expect("get"), b"replaced");

        store.set("ai.apiKey.openai", b"sk-test-12345678").expect("set second");
        let mut keys = store.keys().expect("keys");
        keys.sort();
        assert_eq!(keys, ["ai.apiKey.openai", "smb://testnas.local/share"]);

        store.delete("smb://testnas.local/share").expect("delete");
        store.delete("ai.apiKey.openai").expect("delete second");
        assert!(store.keys().expect("keys after delete").is_empty());
    }

    #[test]
    fn a_missing_key_reads_and_deletes_as_not_found() {
        let scratch = ScratchStore::new("missing");
        assert!(matches!(
            scratch.store.get("nothing-here"),
            Err(SecretStoreError::NotFound(_))
        ));
        assert!(matches!(
            scratch.store.delete("nothing-here"),
            Err(SecretStoreError::NotFound(_))
        ));
        assert!(scratch.store.keys().expect("keys").is_empty());
    }

    /// The listing is by prefix: another service whose name starts the same (`Cmdr` and
    /// `Cmdr-dev`) must not leak into this one's keys, which "Delete all Cmdr data" deletes.
    #[test]
    fn keys_stay_inside_their_own_service() {
        let outer = ScratchStore::new("prefix");
        let inner = ScratchStore {
            store: CredentialManagerStore::new(format!("{}-inner", outer.store.service)),
        };
        outer.store.set("a", b"1").expect("set outer");
        inner.store.set("b", b"2").expect("set inner");
        assert_eq!(outer.store.keys().expect("outer keys"), ["a"]);
        assert_eq!(inner.store.keys().expect("inner keys"), ["b"]);
    }

    /// Credential Manager caps a blob at 2,560 bytes; past it the write refuses, typed as
    /// `Other`, and nothing is stored.
    #[test]
    fn a_secret_past_the_blob_cap_is_refused_whole() {
        let scratch = ScratchStore::new("cap");
        scratch.store.set("fits", &[7u8; 2560]).expect("2,560 bytes fit");
        assert_eq!(scratch.store.get("fits").expect("get").len(), 2560);
        assert!(matches!(
            scratch.store.set("too-big", &[7u8; 2561]),
            Err(SecretStoreError::Other(_))
        ));
        assert!(matches!(
            scratch.store.get("too-big"),
            Err(SecretStoreError::NotFound(_))
        ));
    }
}
