//! Windows: Credential Manager, always. There's no file fallback: every Windows user session has
//! a credential vault, and one that's out of reach (`ERROR_NO_SUCH_LOGON_SESSION`) answers
//! `AccessDenied` per operation rather than moving secrets into a file.

mod credential_manager;

use super::service_name::service_name;
use crate::secrets::SystemStore;
use log::info;

pub(crate) fn system_store() -> SystemStore {
    info!("Secret store: CredentialManagerStore (Windows)");
    SystemStore {
        store: Box::new(credential_manager::CredentialManagerStore::new(service_name())),
        fell_back_to_file: false,
    }
}

pub(crate) fn item_name() -> String {
    service_name().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Windows arm has no file fallback to take: before it, an unsupported-platform branch
    /// wrote every secret into a plaintext `secrets.json`.
    #[test]
    fn the_system_store_never_falls_back_to_a_file() {
        assert!(!system_store().fell_back_to_file);
    }
}
