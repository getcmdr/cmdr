//! Whether a drive's Recycle Bin can take an item, asked before the Shell is.
//!
//! Whether the drive keeps a bin at all is `cmdr_local::recycle_bin`'s answer, the same one
//! the volume's `has_trash` and the volume list read, so the refusal can't disagree with what
//! the UI offered (#1332). What's left here is per item: the bin's size limit.
//!
//! ❗ The Shell announces a FILE too big for the bin as a recycle (`PreDeleteItem` with
//! `TSF_DELETE_RECYCLE_IF_POSSIBLE`), then deletes it for good and reports success with no
//! bin item, so the sink's veto never sees it. So a file is weighed here first, against
//! the bin's size limit as Explorer's own setting stores it. (verified on Windows 10 22H2,
//! NTFS, `windows_tests.rs` on the builder, 2026-11-21: a 64 GiB sparse file on `C:`, whose
//! bin holds 5,831 MB, came back `PostDeleteItem` `0x00270008` with no new item and was
//! gone from disk and from the bin; a folder holding it fell back to its children, which
//! the sink vetoed.)

use std::path::Path;

use cmdr_local::recycle_bin::drive_bin;

/// What the drive's Recycle Bin would do with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bin {
    /// It takes it.
    Takes,
    /// There's no bin that would keep it: the drive keeps none (`cmdr_local::recycle_bin`), or
    /// the item is bigger than its limit.
    WouldDeleteForGood,
}

/// What the bin of the drive holding `plain` does with an item of `size` bytes (`None`
/// for a folder, which the Shell weighs itself: it falls back to the folder's children,
/// and the sink vetoes that).
pub(super) fn verdict(plain: &Path, size: Option<u64>) -> Bin {
    let Some(bin) = drive_bin(plain) else {
        return Bin::WouldDeleteForGood;
    };
    let Some(size) = size else {
        return Bin::Takes;
    };
    // At the limit still recycles, past it doesn't (verified on Windows 10 22H2:
    // a 5,831 MiB file into a 5,831 MB bin landed, one 1 MiB bigger was deleted for good).
    match bin.limit_bytes() {
        Some(limit) if size <= limit => Bin::Takes,
        _ => Bin::WouldDeleteForGood,
    }
}

/// The bin's limit for the drive holding `plain`, for a test to step past.
#[cfg(test)]
pub(super) fn limit_of(plain: &Path) -> Option<u64> {
    drive_bin(plain)?.limit_bytes()
}
