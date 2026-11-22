//! What a Windows reparse point is, by its tag: the one rule that tells a link (a symlink or a
//! junction, which the volume reads, copies, and deletes as the link) from a reparse point that
//! is the item itself (a OneDrive placeholder, a deduplicated file, an app execution alias).
//! Pure arithmetic, compiled into every OS's tests; `local_windows/links.rs` reads the tag.
//!
//! The rule is Windows' own: a tag with the name-surrogate bit (`IsReparseTagNameSurrogate`)
//! names ANOTHER entry, so it's a link; one without it is the entry, with a filter driver
//! behind it. `std`'s `FileType::is_symlink` rests on the same bit, so a listing's
//! `is_symlink` and this classification never disagree.

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
pub(crate) const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// `IO_REPARSE_TAG_SYMLINK`: a symbolic link (`mklink`, `CreateSymbolicLinkW`).
pub(crate) const TAG_SYMLINK: u32 = 0xA000_000C;
/// `IO_REPARSE_TAG_MOUNT_POINT`: a junction (`mklink /J`), or a volume mounted in a folder.
pub(crate) const TAG_MOUNT_POINT: u32 = 0xA000_0003;
/// The name-surrogate bit of a reparse tag.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// What sits at a path, as far as links go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReparseKind {
    /// No reparse point: a plain file or folder.
    None,
    /// A symbolic link, whose target `std::fs::read_link` reads.
    Symlink,
    /// A junction (or a volume mount point), whose target `std::fs::read_link` reads.
    Junction,
    /// Another name surrogate (a WSL symlink, for one): a link Win32 can't read the target of.
    OtherLink(u32),
    /// A reparse point that IS the item (a cloud placeholder, an app execution alias): never
    /// a link, so it lists, copies, and deletes as the file or folder it is.
    NotALink(u32),
}

impl ReparseKind {
    /// The kind of an entry wearing `attributes`, with `tag` its reparse tag (read only when
    /// the attributes say it has one).
    pub(crate) fn of(attributes: u32, tag: u32) -> Self {
        if attributes & ATTRIBUTE_REPARSE_POINT == 0 {
            return Self::None;
        }
        match tag {
            TAG_SYMLINK => Self::Symlink,
            TAG_MOUNT_POINT => Self::Junction,
            tag if tag & NAME_SURROGATE != 0 => Self::OtherLink(tag),
            tag => Self::NotALink(tag),
        }
    }
}

/// `ERROR_INVALID_FUNCTION`: what a filesystem with no reparse points (FAT, exFAT) answers.
const ERROR_INVALID_FUNCTION: i32 = 1;
/// `ERROR_NOT_SUPPORTED`.
const ERROR_NOT_SUPPORTED: i32 = 50;
/// `ERROR_PRIVILEGE_NOT_HELD`: no Developer Mode and no elevated token.
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

/// Whether a refused `CreateSymbolicLinkW` (its Win32 code) means "this destination can't
/// hold the link", the trait's `NotSupported`, rather than a refusal of this one name: no
/// privilege to make links at all, or a filesystem without them. ❗ An access refusal
/// (`ERROR_ACCESS_DENIED`) stays a permission refusal: it's the folder's DACL, and telling
/// that person their drive can't hold links would be wrong.
pub(crate) fn holds_no_links(code: Option<i32>) -> bool {
    matches!(
        code,
        Some(ERROR_PRIVILEGE_NOT_HELD | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION)
    )
}

#[cfg(test)]
#[path = "reparse_test.rs"]
mod tests;
