//! macOS Keychain backend via `security-framework`.

use super::super::service_name::service_name;
use crate::secrets::{SecretStore, SecretStoreError};
use cmdr_fs::log_private::LogPrivate;
use log::debug;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

/// The name Keychain Access shows this instance's items under.
pub(super) fn item_name() -> &'static str {
    service_name()
}

/// Stores secrets in the macOS Keychain.
pub struct KeychainStore;

impl SecretStore for KeychainStore {
    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        debug!("Keychain: setting secret for key: {}", LogPrivate::host(key));
        set_generic_password(service_name(), key, value)
            .map_err(|e| SecretStoreError::Other(format!("Failed to save to Keychain: {}", e)))
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, SecretStoreError> {
        debug!("Keychain: getting secret for key: {}", LogPrivate::host(key));
        get_generic_password(service_name(), key).map_err(|e| classify_security_error(key, e))
    }

    fn delete(&self, key: &str) -> Result<(), SecretStoreError> {
        debug!("Keychain: deleting secret for key: {}", LogPrivate::host(key));
        delete_generic_password(service_name(), key).map_err(|e| classify_security_error(key, e))
    }

    /// Every generic password under this instance's service. Reads attributes only, never the
    /// secret data, so it raises no Keychain consent dialog.
    fn keys(&self) -> Result<Vec<String>, SecretStoreError> {
        debug!("Keychain: listing keys");
        let found = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service_name())
            .load_attributes(true)
            .limit(Limit::All)
            .search();
        match found {
            Ok(results) => Ok(results
                .iter()
                .filter_map(SearchResult::simplify_dict)
                .filter_map(|attributes| attributes.get(ACCOUNT_ATTRIBUTE).cloned())
                .collect()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(Vec::new()),
            Err(e) => Err(classify_security_error(service_name(), e)),
        }
    }
}

/// `kSecAttrAccount`'s key in a simplified attribute dictionary: where a generic password keeps
/// the key we stored it under.
const ACCOUNT_ATTRIBUTE: &str = "acct";

/// Stable OSStatus codes from Apple's `SecBase.h`. Match on these instead of
/// the `security-framework` `Display` impl, which localizes on non-English
/// systems and could be reformatted in any minor crate bump.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
const ERR_SEC_AUTH_FAILED: i32 = -25293;
const ERR_SEC_USER_CANCELED: i32 = -128;
const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;

fn classify_security_error(key: &str, error: security_framework::base::Error) -> SecretStoreError {
    match error.code() {
        ERR_SEC_ITEM_NOT_FOUND => SecretStoreError::NotFound(format!("No secret found for key: {}", key)),
        ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED | ERR_SEC_INTERACTION_NOT_ALLOWED => {
            SecretStoreError::AccessDenied(format!("{}", error))
        }
        _ => SecretStoreError::Other(format!("{}", error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_security_error_uses_osstatus_not_message() {
        // Regression for the medium-severity audit finding: the classifier
        // used to substring-match the localized `Display` of the error, so
        // `errSecItemNotFound` on a non-English macOS came back as `Other`
        // and broke the "missing key → empty default" fallback in the AI
        // API-key + SMB credential flows.
        let not_found = security_framework::base::Error::from(ERR_SEC_ITEM_NOT_FOUND);
        assert!(matches!(
            classify_security_error("ai.apiKey.openai", not_found),
            SecretStoreError::NotFound(_)
        ));

        let auth_failed = security_framework::base::Error::from(ERR_SEC_AUTH_FAILED);
        assert!(matches!(
            classify_security_error("ai.apiKey.openai", auth_failed),
            SecretStoreError::AccessDenied(_)
        ));

        let user_canceled = security_framework::base::Error::from(ERR_SEC_USER_CANCELED);
        assert!(matches!(
            classify_security_error("ai.apiKey.openai", user_canceled),
            SecretStoreError::AccessDenied(_)
        ));

        // Unknown OSStatus must NOT collapse into `NotFound`/`AccessDenied`.
        let unknown = security_framework::base::Error::from(-99999);
        assert!(matches!(
            classify_security_error("ai.apiKey.openai", unknown),
            SecretStoreError::Other(_)
        ));
    }
}
