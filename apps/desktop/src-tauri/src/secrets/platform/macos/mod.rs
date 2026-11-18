//! macOS: the login Keychain, always.

mod keychain;

use crate::secrets::SystemStore;
use log::info;

pub(crate) fn system_store() -> SystemStore {
    info!("Secret store: KeychainStore (macOS)");
    SystemStore {
        store: Box::new(keychain::KeychainStore),
        fell_back_to_file: false,
    }
}

pub(crate) fn item_name() -> String {
    keychain::item_name().to_string()
}
