//! Linux: the Secret Service when one answers a write-read-delete probe, else the encrypted file
//! keyed by this machine's id.

mod keyring;

use crate::secrets::{SystemStore, encrypted_file, secret_store_dir};
use log::info;

pub(crate) fn system_store() -> SystemStore {
    if keyring::KeyringStore::is_available() {
        info!("Secret store: KeyringStore (Linux Secret Service)");
        return SystemStore {
            store: Box::new(keyring::KeyringStore),
            fell_back_to_file: false,
        };
    }
    let dir = secret_store_dir();
    info!("Secret store: EncryptedFileStore (Linux fallback, no secret service)");
    SystemStore {
        store: Box::new(encrypted_file::EncryptedFileStore::new(dir)),
        fell_back_to_file: true,
    }
}

/// Secret Service items carry the plain product name.
pub(crate) fn item_name() -> String {
    "Cmdr".to_string()
}
