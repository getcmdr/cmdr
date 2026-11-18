//! The `platform_case` collation the `name` column sorts and compares under, and
//! the same folding as a plain function.
//!
//! Custom collations aren't persisted in the DB file, so every connection has to
//! register this one before any table creation or query (`connection.rs` does).

use rusqlite::Connection;

use super::IndexStoreError;

/// Register the `platform_case` collation on a connection.
///
/// Must be called on every connection before any table creation or query,
/// because custom collations are not persisted in the DB file.
pub fn register_platform_case_collation(conn: &Connection) -> Result<(), IndexStoreError> {
    conn.create_collation("platform_case", platform_case_compare)?;
    Ok(())
}

/// Compare two strings using the platform's filesystem case/normalization rules:
/// the order of [`normalize_for_comparison`]'s keys, so the collation and the
/// stored `name_folded` key can never disagree. macOS folds form and case
/// (APFS), Windows case (NTFS), Linux nothing (ext4/btrfs).
pub(super) fn platform_case_compare(a: &str, b: &str) -> std::cmp::Ordering {
    cmdr_fs::name_fold::compare_for_comparison(a, b)
}

/// The same folding as a plain function, owned by `cmdr-fs` so a crate that
/// matches paths against the index (the write engine) shares one key without
/// depending on the index.
pub use cmdr_fs::name_fold::normalize_for_comparison;
