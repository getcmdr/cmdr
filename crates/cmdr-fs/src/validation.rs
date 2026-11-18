//! Filename and path validation utilities.
//!
//! Reusable validation for rename, mkdir, Multi-Rename, and file transfer
//! operations: which names a destination filesystem takes ([`NameRules`],
//! keyed on its kind, never the host OS), name length, and path length limits.

use serde::{Deserialize, Serialize};

/// Maximum file name length in bytes. A name of exactly this many is fine
/// (verified on macOS 27.0 with a 255-byte ASCII name on APFS, exFAT, FAT32,
/// and a QNAP SMB share, 2026-11-18); ext4, ZFS, and Samba over them count bytes,
/// so it's the floor every destination meets. APFS, HFS+, exFAT, FAT32, and NTFS
/// count UTF-16 units instead and take some longer non-ASCII names.
pub const MAX_NAME_BYTES: usize = 255;

/// Maximum path length in bytes.
/// macOS PATH_MAX is 1024; Linux PATH_MAX is 4096.
#[cfg(target_os = "linux")]
pub const MAX_PATH_BYTES: usize = 4096;
/// Maximum path length in bytes (macOS `PATH_MAX`, which the browser build borrows).
#[cfg(any(target_os = "macos", target_family = "wasm"))]
pub const MAX_PATH_BYTES: usize = 1024;

/// Validation error types for filename and path checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ValidationError {
    /// Name is empty or whitespace-only after trimming.
    Empty,
    /// Name contains a disallowed character.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    DisallowedCharacter { character: String },
    /// Name exceeds the maximum byte length for a single filename component.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    NameTooLong { bytes: usize, max: usize },
    /// Full path exceeds the maximum byte length.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    PathTooLong { bytes: usize, max: usize },
    /// Name is a device name Windows reserves (`CON`, `NUL`, `COM1`, …), on a
    /// destination that refuses one ([`NameRules::SmbShare`], [`NameRules::Windows`]).
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    ReservedName { name: String },
    /// Name ends in a space or a period, on a FAT32, exFAT, or NTFS destination
    /// ([`NameRules::Windows`]).
    EndsWithSpaceOrPeriod,
}

impl std::fmt::Display for ValidationError {
    /// ❗ For logs and debugging only: the frontend words each variant from its own catalog.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Name can't be empty"),
            Self::DisallowedCharacter { character } => {
                write!(f, "Name contains a disallowed character: {}", character)
            }
            Self::NameTooLong { bytes, max } => {
                write!(f, "Name is {} bytes, which exceeds the {} byte limit", bytes, max)
            }
            Self::PathTooLong { bytes, max } => {
                write!(f, "Path is {} bytes, which exceeds the {} byte limit", bytes, max)
            }
            Self::ReservedName { name } => write!(f, "Name is the reserved device name {name}"),
            Self::EndsWithSpaceOrPeriod => write!(f, "Name ends in a space or a period"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Which names a destination filesystem takes, keyed on its
/// [`FilesystemKind`](crate::filesystem_kind::FilesystemKind)
/// ([`FilesystemKind::name_rules`](crate::filesystem_kind::FilesystemKind::name_rules)),
/// ❌ never on the host OS: a Mac writing to an SMB share meets the share's
/// rules, and Windows writing over MTP meets none of its own.
///
/// What each format does with a name its rules refuse, read on a Mac:
/// `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NameRules {
    /// Only `/` and NUL are refused: APFS, HFS+, ext4, Btrfs, XFS, ZFS, MTP, and
    /// any filesystem Cmdr can't see through, which answers for itself.
    Posix,
    /// An SMB share: [`Posix`](Self::Posix), plus the device names Windows
    /// reserves (`CON`, `NUL`, `COM1`, `con.txt`, …), which Samba stores but
    /// lists back to every client under a mangled 8.3 name (`CON` reads
    /// `CR23RR~9`). The characters Windows forbids are fine here: smb2 and
    /// macOS smbfs both carry them in the private-use area, both ways.
    SmbShare,
    /// FAT32, exFAT, and NTFS: [`SmbShare`](Self::SmbShare), plus the characters
    /// the format can't store (`"`, `*`, `:`, `<`, `>`, `?`, `\`, `|`, and the
    /// control characters) and a name that ends in a space or a period. Windows
    /// refuses each one; macOS's drivers write them as private-use stand-ins
    /// that any other reader shows as something else, so the name that would
    /// land isn't the name the person typed.
    Windows,
}

/// The device names Windows reserves in every folder, matched ignoring case
/// against a name's part before its first period, trailing spaces trimmed
/// (`nul.tar.gz` and `CON .txt` are reserved too). Microsoft's list, superscript
/// digits included: "Naming Files, Paths, and Namespaces", learn.microsoft.com,
/// read 2026-11-18.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹",
    "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²",
    "LPT³",
];

/// The printable characters a FAT32, exFAT, or NTFS name can't hold.
const WINDOWS_FORBIDDEN_CHARACTERS: &[char] = &['"', '*', ':', '<', '>', '?', '\\', '|'];

/// Validates `name` for a destination that follows `rules`.
///
/// Checks performed (the input is checked as-is; callers trim if they want to):
/// - Not empty / whitespace-only
/// - No `/` or NUL, on every filesystem
/// - No more than [`MAX_NAME_BYTES`] bytes
/// - [`NameRules::SmbShare`] and [`NameRules::Windows`]: not a reserved device name
/// - [`NameRules::Windows`]: none of the characters the format can't store, and
///   no trailing space or period
pub fn validate_filename(name: &str, rules: NameRules) -> Result<(), ValidationError> {
    if name.trim().is_empty() {
        return Err(ValidationError::Empty);
    }

    for ch in name.chars() {
        if let Some(character) = disallowed_character(ch, rules) {
            return Err(ValidationError::DisallowedCharacter { character });
        }
    }

    let byte_len = name.len();
    if byte_len > MAX_NAME_BYTES {
        return Err(ValidationError::NameTooLong {
            bytes: byte_len,
            max: MAX_NAME_BYTES,
        });
    }

    if matches!(rules, NameRules::SmbShare | NameRules::Windows)
        && let Some(reserved) = reserved_device_name(name)
    {
        return Err(ValidationError::ReservedName {
            name: reserved.to_string(),
        });
    }

    if rules == NameRules::Windows && name.ends_with([' ', '.']) {
        return Err(ValidationError::EndsWithSpaceOrPeriod);
    }

    Ok(())
}

/// How `ch` reads in a [`ValidationError::DisallowedCharacter`] when `rules`
/// refuse it: itself when it's visible, `NUL` or its `U+` code when it isn't.
fn disallowed_character(ch: char, rules: NameRules) -> Option<String> {
    match ch {
        '/' => Some("/".to_string()),
        '\0' => Some("NUL".to_string()),
        _ if rules != NameRules::Windows => None,
        '\u{1}'..='\u{1f}' => Some(format!("U+{:04X}", u32::from(ch))),
        _ if WINDOWS_FORBIDDEN_CHARACTERS.contains(&ch) => Some(ch.to_string()),
        _ => None,
    }
}

/// The part of `name` that makes it a Windows device name, as typed, or `None`.
fn reserved_device_name(name: &str) -> Option<&str> {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        .then_some(stem)
}

/// Why a name isn't one plain, visible leaf, for [`validate_plain_leaf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlainLeafRefusal {
    /// Empty, or only whitespace.
    Empty,
    /// Starts or ends with whitespace, which reads exactly like the name without it.
    EdgeWhitespace,
    /// Holds a `/`, so it's a path, not one name.
    Separator,
    /// `.` or `..`, which name a folder, not an entry in one.
    DotName,
    /// Longer than [`MAX_NAME_BYTES`].
    TooLong {
        /// The name's length in bytes.
        bytes: usize,
    },
    /// Holds a control character (`char::is_control`, NUL included).
    Control(char),
    /// Holds an invisible formatting character: a bidi embedding, override,
    /// isolate, or mark, a zero-width character, a byte-order mark, or a soft
    /// hyphen.
    Invisible(char),
}

/// Checks that `name` is one plain, visible leaf: a name a reviewer reads as
/// exactly what lands.
///
/// Stricter than [`validate_filename`] on purpose: it's for a name somebody
/// else chose for the user (a per-item transfer name, a proposal), where a name
/// that renders as something else is a spoof the review can't show:
/// `invoice`, a right-to-left override, then `fdp.exe` reads as
/// `invoiceexe.pdf`. Refuses an empty name, edge whitespace, `/`, `.` and
/// `..`, more than [`MAX_NAME_BYTES`] bytes, any control character, and the
/// invisible format characters [`is_invisible_format`] lists.
pub fn validate_plain_leaf(name: &str) -> Result<(), PlainLeafRefusal> {
    if name.trim().is_empty() {
        return Err(PlainLeafRefusal::Empty);
    }
    if name.starts_with(char::is_whitespace) || name.ends_with(char::is_whitespace) {
        return Err(PlainLeafRefusal::EdgeWhitespace);
    }
    if name.contains('/') {
        return Err(PlainLeafRefusal::Separator);
    }
    if name == "." || name == ".." {
        return Err(PlainLeafRefusal::DotName);
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(PlainLeafRefusal::TooLong { bytes: name.len() });
    }
    for ch in name.chars() {
        if ch.is_control() {
            return Err(PlainLeafRefusal::Control(ch));
        }
        if is_invisible_format(ch) {
            return Err(PlainLeafRefusal::Invisible(ch));
        }
    }
    Ok(())
}

/// The invisible format characters a plain leaf refuses: they change how a
/// name renders, or hide inside it, without showing themselves. The bidi
/// embeddings and overrides (U+202A–U+202E), the isolates (U+2066–U+2069), the
/// marks (U+200E, U+200F, U+061C), the zero-width characters (U+200B–U+200D,
/// U+2060), the byte-order mark (U+FEFF), and the soft hyphen (U+00AD). An
/// explicit list rather than all of category `Cf`, so a legitimate name loses
/// nothing it could show.
pub fn is_invisible_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200D}'
            | '\u{2060}'
            | '\u{FEFF}'
            | '\u{00AD}'
    )
}

/// Validates that a full path doesn't exceed the filesystem path length limit.
pub fn validate_path_length(path: &std::path::Path) -> Result<(), ValidationError> {
    let byte_len = path.as_os_str().len();
    if byte_len >= MAX_PATH_BYTES {
        return Err(ValidationError::PathTooLong {
            bytes: byte_len,
            max: MAX_PATH_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "validation_plain_leaf_test.rs"]
mod plain_leaf_tests;

#[cfg(test)]
#[path = "validation_name_rules_test.rs"]
mod name_rules_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ========================================================================
    // validate_path_length
    // ========================================================================

    #[test]
    fn valid_short_path() {
        assert!(validate_path_length(Path::new("/Users/test/file.txt")).is_ok());
    }

    #[test]
    fn rejects_path_at_max_bytes() {
        let long_path = "/".to_string() + &"a".repeat(MAX_PATH_BYTES - 1);
        let result = validate_path_length(Path::new(&long_path));
        assert!(matches!(result, Err(ValidationError::PathTooLong { .. })));
    }

    #[test]
    fn accepts_path_just_under_max() {
        let path = "/".to_string() + &"a".repeat(MAX_PATH_BYTES - 2);
        assert!(validate_path_length(Path::new(&path)).is_ok());
    }
}
