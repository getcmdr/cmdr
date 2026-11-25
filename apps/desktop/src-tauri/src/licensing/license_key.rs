//! The user's license key: verifying one they enter (short codes exchanged first), storing it,
//! and reading the installed one back, verified and cached in memory.
//!
//! The signature check itself is `verification.rs`; this module only feeds it keys.

use crate::ignore_poison::IgnorePoison;
use crate::licensing::key_storage::{self, LICENSE_FILE, StoredLicense};
use crate::licensing::validation_client::{activate_short_code, is_short_code};
use crate::licensing::verification::{LicenseActivationError, validate_license_key};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// In-memory cache for verified license info. Avoids re-parsing and re-verifying
/// the Ed25519 signature on every call to `get_license_info`.
static LICENSE_CACHE: Mutex<Option<LicenseInfo>> = Mutex::new(None);

/// Set when the secret store errored on reading the key (a denied Keychain prompt, a locked
/// keyring). Status checks run often, so retrying each time could raise a prompt per check; this
/// holds the license as unreadable until the next launch, a new activation, or a reset.
static KEY_UNREADABLE: AtomicBool = AtomicBool::new(false);

/// Information about the current license.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInfo {
    pub email: String,
    pub transaction_id: String,
    pub issued_at: String,
    pub organization_name: Option<String>,
    pub license_type: Option<String>,
    /// The short code used to activate (if available)
    pub short_code: Option<String>,
    /// The end date signed into the key (RFC 3339), only on a dated license.
    pub expires_at: Option<String>,
}

/// Result of verifying a license key without persisting it.
/// Kept separate from `LicenseInfo` so the full key doesn't leak to the frontend via
/// `get_license_info`.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub info: LicenseInfo,
    /// The full cryptographic key (same as input for full keys, exchanged for short codes).
    pub full_key: String,
    /// The original short code, if the input was a short code.
    pub short_code: Option<String>,
}

/// Verify a license key or short code without writing anything to disk or cache.
/// If the input is a short code, exchanges it for the full key via the server first.
/// This is the "try it" step: call `commit_license` afterwards to persist a verified key.
pub async fn verify_license_async(input: &str) -> Result<VerifyResult, LicenseActivationError> {
    let (full_key, short_code) = if is_short_code(input) {
        let key = activate_short_code(input).await?;
        (key, Some(input.to_string()))
    } else {
        (input.to_string(), None)
    };

    let data = validate_license_key(&full_key)?;
    // Prefer the input short code (user typed it), fall back to the one embedded in the payload
    let resolved_short_code = short_code.or(data.short_code);
    let info = LicenseInfo {
        email: data.email,
        transaction_id: data.transaction_id,
        issued_at: data.issued_at,
        organization_name: data.organization_name,
        license_type: data.license_type,
        short_code: resolved_short_code.clone(),
        expires_at: data.expires_at,
    };
    Ok(VerifyResult {
        info,
        full_key,
        short_code: resolved_short_code,
    })
}

/// Verify a license key, store it in the secret store, and cache its info in memory.
/// `app_status::commit_license` calls this, then seeds the cached status. On `Err`, nothing was
/// stored.
pub(super) fn store_license(
    app: &tauri::AppHandle,
    license_key: &str,
    short_code: Option<&str>,
) -> Result<LicenseInfo, LicenseActivationError> {
    let data = validate_license_key(license_key)?;

    // Prefer the explicit short code (user typed it), fall back to the one embedded in the payload
    let resolved_short_code = short_code.map(|s| s.to_string()).or(data.short_code.clone());

    let store = key_storage::license_store(app).map_err(|e| LicenseActivationError::StorageError {
        detail: format!("Couldn't open {LICENSE_FILE}: {e}"),
    })?;

    // The key goes to the secret store (the Keychain on macOS); license.json only gets the marker.
    let stored = StoredLicense {
        license_key: license_key.to_string(),
        short_code: resolved_short_code.clone(),
    };
    key_storage::save(&*store, crate::secrets::store(), &stored).map_err(|e| {
        log::warn!("Couldn't store the license key in the secret store: {e}");
        LicenseActivationError::StorageError { detail: e.to_string() }
    })?;
    // The marker that says the key is in the secret store has to be on disk, or the next launch
    // reads the app as unlicensed.
    store.save().map_err(|e| LicenseActivationError::StorageError {
        detail: format!("Couldn't save {LICENSE_FILE}: {e}"),
    })?;
    KEY_UNREADABLE.store(false, Ordering::Relaxed);

    let info = LicenseInfo {
        email: data.email,
        transaction_id: data.transaction_id,
        issued_at: data.issued_at,
        organization_name: data.organization_name,
        license_type: data.license_type,
        short_code: resolved_short_code,
        expires_at: data.expires_at,
    };

    // Update the in-memory cache with the newly committed license
    *LICENSE_CACHE.lock_ignore_poison() = Some(info.clone());

    Ok(info)
}

/// Get stored license info, if any. Returns a cached result after the first successful
/// verification.
pub fn get_license_info(app: &tauri::AppHandle) -> Option<LicenseInfo> {
    // Fast path: return cached info if available
    if let Some(info) = LICENSE_CACHE.lock_ignore_poison().clone() {
        return Some(info);
    }

    if KEY_UNREADABLE.load(Ordering::Relaxed) {
        return None;
    }

    // Slow path: read the key (secret store, or a not-yet-migrated license.json) and verify its
    // Ed25519 signature
    let store = key_storage::license_store(app).ok()?;
    let StoredLicense {
        license_key,
        short_code,
    } = match key_storage::load(&*store, crate::secrets::store()) {
        Ok(stored) => stored?,
        Err(e) => {
            log::warn!(
                "Couldn't read the license key from the secret store, treating it as absent until relaunch: {e}"
            );
            KEY_UNREADABLE.store(true, Ordering::Relaxed);
            return None;
        }
    };

    let info = validate_license_key(&license_key).ok().map(|data| {
        // Prefer the stored short code, fall back to the one embedded in the payload
        let resolved_short_code = short_code.or(data.short_code);
        LicenseInfo {
            email: data.email,
            transaction_id: data.transaction_id,
            issued_at: data.issued_at,
            organization_name: data.organization_name,
            license_type: data.license_type,
            short_code: resolved_short_code,
            expires_at: data.expires_at,
        }
    })?;

    // Populate cache for subsequent calls
    *LICENSE_CACHE.lock_ignore_poison() = Some(info.clone());

    Some(info)
}

/// Clear the in-memory license cache. Called when the license is reset.
pub fn clear_license_cache() {
    *LICENSE_CACHE.lock_ignore_poison() = None;
    KEY_UNREADABLE.store(false, Ordering::Relaxed);
}
