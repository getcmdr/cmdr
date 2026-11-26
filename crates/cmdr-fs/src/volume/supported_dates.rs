//! Which dates a volume's listings carry beyond the modification date.

use serde::{Deserialize, Serialize};

/// Which dates a volume's listings and `get_metadata` fill beyond
/// [`FileEntry::modified_at`](crate::entry::FileEntry::modified_at), which
/// every backend fills.
///
/// What [`Volume::supported_dates`](super::Volume::supported_dates) answers, and
/// what a sort by a date the volume can't fill falls back from. A claim may be
/// wider than one entry's data (a WebDAV server may leave one file's creation
/// date out), but never narrower: a backend fills
/// [`FileEntry::created_at`](crate::entry::FileEntry::created_at) or
/// [`FileEntry::added_at`](crate::entry::FileEntry::added_at) only when it
/// claims that date, which `conformance::assert_listing_dates_are_claimed`
/// pins. Each backend's answer and why: `crates/cmdr-fs/DETAILS.md`
/// § "Which dates each volume lists".
// DEFAULT-OK: modified only, the honest answer of a backend that fills no other date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SupportedDates {
    /// Entries carry a creation (birth) date.
    pub created: bool,
    /// Entries carry the date they were added to their folder (macOS APFS and
    /// HFS+ only).
    pub added: bool,
    /// The modification date is a LAST USE (the Servers list's "Last used"): a row with
    /// none was never used. Sorting by it, such a row ranks after every dated one in both
    /// orders, and the directory mode doesn't hold the rows in name order, since every
    /// row is a place the person orders by use (`cmdr-listing`'s `EffectiveSort::resolve`).
    #[serde(default)]
    pub modified_is_last_use: bool,
}

impl SupportedDates {
    /// The modification date only: the trait's default, so a new backend
    /// can't claim a date it never fills.
    pub const NONE: Self = Self {
        created: false,
        added: false,
        modified_is_last_use: false,
    };

    /// The creation date on top of the modification date.
    pub const CREATED: Self = Self {
        created: true,
        added: false,
        modified_is_last_use: false,
    };

    /// The modification date only, as a last use (see [`Self::modified_is_last_use`]).
    pub const LAST_USE: Self = Self {
        created: false,
        added: false,
        modified_is_last_use: true,
    };

    /// Every date a listing can carry.
    pub const ALL: Self = Self {
        created: true,
        added: true,
        modified_is_last_use: false,
    };
}
