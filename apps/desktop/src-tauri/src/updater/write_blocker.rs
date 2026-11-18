//! `BundleWriteBlocker`, the typed answer to "can this install write its own update?". Its own
//! file so the IPC type exists on every OS; only macOS classifies (`bundle_location.rs`), so
//! elsewhere `update_write_blocker` answers `None`.

/// Why this install can't apply an update, when the reason is where the bundle SITS or who owns it,
/// rather than anything about the update itself.
///
/// The first two ask the user to move Cmdr into Applications; the last two to replace it (or have it
/// replaced). They're kept apart so the log and the `update_check` event can say which arrangement
/// produced the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[allow(
    dead_code,
    reason = "only macOS classifies a bundle (`platform/macos/bundle_location.rs`)"
)]
pub enum BundleWriteBlocker {
    /// macOS App Translocation: Cmdr was opened from where it was downloaded, so it's running
    /// from a randomized read-only mount under `/private/var/folders/…/AppTranslocation/`.
    Translocated,
    /// The bundle lives on a read-only volume, which in practice means a `.dmg` still mounted.
    ReadOnlyVolume,
    /// This user can't write into the bundle (it belongs to another account, typically `root`), and
    /// they're an administrator: Finder can replace the app after their password.
    NotWritableAdminUser,
    /// This user can't write into the bundle and isn't an administrator (or we couldn't tell): only
    /// whoever manages this Mac can update it.
    NotWritableStandardUser,
}

impl std::fmt::Display for BundleWriteBlocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Translocated => f.write_str("running translocated (opened from its download location)"),
            Self::ReadOnlyVolume => f.write_str("running from a read-only volume"),
            Self::NotWritableAdminUser => f.write_str("not writable by this user, who is an administrator"),
            Self::NotWritableStandardUser => f.write_str("not writable by this user, who isn't an administrator"),
        }
    }
}
