//! Filename and path validation utilities.
//!
//! Reusable validation for rename, mkdir, Multi-Rename, and file transfer
//! operations: which names a destination filesystem takes ([`NameRules`],
//! keyed on its kind), which it takes but won't keep portable
//! ([`PortabilityWarning`]), and its name and path length limits, each in the
//! filesystem's own unit.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

mod platform;

/// The name length every destination takes, in bytes: ext4, ZFS, and Samba
/// over them count bytes, so it's the floor. A name of exactly this many is
/// fine (verified on macOS 27.0 with a 255-byte ASCII name on APFS, exFAT,
/// FAT32, and a QNAP SMB share, 2026-11-18). A destination's own limit is
/// [`NameRules::name_limit`]; this floor is for a name with no destination yet
/// ([`validate_plain_leaf`]).
pub const MAX_NAME_BYTES: usize = 255;

/// What a length limit counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LengthUnit {
    /// UTF-8 bytes: ext4, Btrfs, XFS, ZFS, and Samba over them.
    Bytes,
    /// UTF-16 code units: APFS, exFAT, FAT32, NTFS, and Windows' own paths.
    /// An emoji outside the Basic Multilingual Plane is two.
    Utf16Units,
    /// UTF-16 code units of the name DECOMPOSED (NFD): HFS+, which stores names
    /// that way, so a precomposed `é` (one unit) takes two there.
    Utf16UnitsDecomposed,
}

/// A length limit, inclusive, in its filesystem's own unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LengthLimit {
    /// The longest a name or path may be, inclusive.
    pub max: usize,
    /// What [`max`](Self::max) counts.
    pub unit: LengthUnit,
}

impl LengthLimit {
    /// 255 UTF-8 bytes: the byte-counting filesystems' name limit.
    pub const BYTES_255: Self = Self {
        max: MAX_NAME_BYTES,
        unit: LengthUnit::Bytes,
    };
    /// 255 UTF-16 units: APFS's and the Windows formats' name limit
    /// (verified on macOS 27.0: `é` × 200 lands on APFS, exFAT, and FAT32, and
    /// `😀` × 128 doesn't, 2026-11-18).
    pub const UTF16_255: Self = Self {
        max: 255,
        unit: LengthUnit::Utf16Units,
    };
    /// HFS+'s name limit: 255 UTF-16 units of the name as HFS+ stores it,
    /// decomposed (Apple's TN1150, "HFS Plus Names").
    pub const HFS_PLUS_255: Self = Self {
        max: 255,
        unit: LengthUnit::Utf16UnitsDecomposed,
    };
    /// An SMB2 path's ceiling: CREATE's `NameLength` is a 16-bit byte count
    /// of UTF-16 (MS-SMB2 § 2.2.13, read 2026-11-18), so 32,767 units.
    pub const SMB_PATH: Self = Self {
        max: 32_767,
        unit: LengthUnit::Utf16Units,
    };

    /// How long `text` is in this limit's unit.
    pub fn length_of(&self, text: &str) -> usize {
        match self.unit {
            LengthUnit::Bytes => text.len(),
            LengthUnit::Utf16Units => text.encode_utf16().count(),
            LengthUnit::Utf16UnitsDecomposed => text.nfd().map(char::len_utf16).sum(),
        }
    }

    /// `text`'s length when it's past this limit, or `None` when it fits.
    fn exceeded_by(&self, text: &str) -> Option<usize> {
        // A UTF-8 string is never shorter in bytes than in UTF-16 units, so a
        // short one skips the count. Not decomposed: `Ǖ` is two bytes and three
        // decomposed units.
        if self.unit != LengthUnit::Utf16UnitsDecomposed && text.len() <= self.max {
            return None;
        }
        let length = self.length_of(text);
        (length > self.max).then_some(length)
    }
}

/// Validation error types for filename and path checks: what a destination
/// REFUSES. A name it takes but won't keep portable is a
/// [`PortabilityWarning`] instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ValidationError {
    /// Name is empty, or renders as nothing: only whitespace and invisible
    /// characters ([`renders_blank`]).
    Empty,
    /// Name is `.` or `..`, which name a folder, not an entry in one.
    DotName,
    /// Name contains a disallowed character.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    DisallowedCharacter { character: String },
    /// Name is longer than the destination's [`NameRules::name_limit`], counted
    /// in its unit.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    NameTooLong {
        length: usize,
        max: usize,
        unit: LengthUnit,
    },
    /// Full path is longer than the destination's [`NameRules::path_limit`].
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    PathTooLong {
        length: usize,
        max: usize,
        unit: LengthUnit,
    },
    /// Name is a device name Windows reserves (`CON`, `NUL`, `COM1`, …), on a
    /// destination that refuses one (Windows' own disks).
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    ReservedName { name: String },
    /// Name ends in a space or a period, on a Windows-format destination whose
    /// driver refuses or strips one.
    EndsWithSpaceOrPeriod,
}

impl std::fmt::Display for ValidationError {
    /// ❗ For logs and debugging only: the frontend words each variant from its own catalog.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Name can't be empty"),
            Self::DotName => write!(f, "Name is . or .."),
            Self::DisallowedCharacter { character } => {
                write!(f, "Name contains a disallowed character: {}", character)
            }
            Self::NameTooLong { length, max, unit } => {
                write!(f, "Name is {length} {unit:?}, past the {max} limit")
            }
            Self::PathTooLong { length, max, unit } => {
                write!(f, "Path is {length} {unit:?}, past the {max} limit")
            }
            Self::ReservedName { name } => write!(f, "Name is the reserved device name {name}"),
            Self::EndsWithSpaceOrPeriod => write!(f, "Name ends in a space or a period"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// A name the destination TAKES but won't keep portable: Windows (and, for a
/// stand-in on a stick, Linux) shows or opens it as something else. The
/// operation goes ahead; the person is told. ❗ Never a stand-in for a
/// [`ValidationError`]: a refusal protects data, a warning only informs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum PortabilityWarning {
    /// A character Windows can't use in a name (`" * : < > ? \ |` or a control
    /// character), which the destination stores as a private-use stand-in.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    WindowsCharacter { character: String },
    /// A device name Windows reserves (`CON`, `nul.txt`): Windows can't open
    /// it, and a Samba share lists it under a mangled 8.3 name.
    #[allow(
        missing_docs,
        reason = "a field doc would change `bindings.ts`; the variant doc says what it holds"
    )]
    ReservedName { name: String },
    /// A trailing space or period, which the destination stores as a stand-in.
    EndsWithSpaceOrPeriod,
}

impl PortabilityWarning {
    /// Whether Linux reads the name as something else too: a stand-in on a
    /// Windows-format disk shows as a private-use character there (verified on
    /// kernel 7.2.12, `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`).
    /// A reserved name is only Windows' trouble, and a share's stand-ins map back
    /// for its clients. ❗ The frontend's `warningReachesLinux` mirrors this.
    pub fn reaches_linux(&self, rules: NameRules) -> bool {
        rules.format == NameFormat::Windows && !matches!(self, Self::ReservedName { .. })
    }
}

/// What a destination is, for which names Windows can't use there, and for the
/// words a warning uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NameFormat {
    /// APFS, HFS+, ext4, Btrfs, XFS, ZFS, MTP, and any filesystem Cmdr can't
    /// see through: only `/` and NUL are refused.
    Posix,
    /// An SMB share, which Windows computers read too.
    SmbShare,
    /// FAT32, exFAT, and NTFS.
    Windows,
}

/// What a destination does with one class of name Windows can't use.
/// Ordered from the weakest to the strictest, so a floor can raise a tier ([`NameRules::at_least`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NameTier {
    /// Nothing to say: a POSIX destination.
    Allow,
    /// The destination takes it, under a stand-in or a mangled listing:
    /// [`PortabilityWarning`].
    Warn,
    /// The destination refuses it, or would lose data taking it (a stripped
    /// trailing period opening a sibling, `CON` opening the console):
    /// [`ValidationError`].
    Refuse,
}

/// Which names a destination filesystem takes, and which it takes but won't
/// keep portable, keyed on its
/// [`FilesystemKind`](crate::filesystem_kind::FilesystemKind)
/// ([`FilesystemKind::name_rules`](crate::filesystem_kind::FilesystemKind::name_rules))
/// plus how THIS host's driver writes a Windows-format disk.
///
/// What each format does with a Windows-unfriendly name, read on a Mac and on
/// Linux: `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NameRules {
    /// What the destination is.
    pub format: NameFormat,
    /// The characters Windows can't use (`" * : < > ? \ |`, control
    /// characters) and a trailing space or period.
    pub windows_characters: NameTier,
    /// The device names Windows reserves (`CON`, `NUL`, `COM1`, `con.txt`, …).
    pub reserved_names: NameTier,
    /// The longest name, in the filesystem's unit.
    pub name_limit: LengthLimit,
    /// The longest full path, or `None` when the backend answers for itself.
    pub path_limit: Option<LengthLimit>,
}

impl NameRules {
    /// Only `/` and NUL, 255 bytes, no path limit: a destination Cmdr can't see
    /// behind (MTP, SFTP, S3, an unknown mount), which answers for itself, and
    /// the answer when a probe can't tell.
    pub const POSIX: Self = Self {
        format: NameFormat::Posix,
        windows_characters: NameTier::Allow,
        reserved_names: NameTier::Allow,
        name_limit: LengthLimit::BYTES_255,
        path_limit: None,
    };

    /// A POSIX filesystem whose names are `name_limit` long.
    pub const fn posix(name_limit: LengthLimit) -> Self {
        Self {
            name_limit,
            ..Self::POSIX
        }
    }

    /// An SMB share. Both clients Cmdr writes through (smb2, macOS smbfs) carry
    /// the characters Windows can't use as private-use stand-ins, both ways, so
    /// the share takes them; and Samba stores a device name and lists it back
    /// mangled (`CON` → `CR23RR~9`). The SMB specs forbid neither: MS-FSCC
    /// § 2.1.5.2 names no device names (v20251121, read 2026-11-18), so both
    /// warn. 255 bytes, since a NAS's disk under Samba counts bytes.
    pub const fn smb_share() -> Self {
        Self {
            format: NameFormat::SmbShare,
            windows_characters: NameTier::Warn,
            reserved_names: NameTier::Warn,
            name_limit: LengthLimit::BYTES_255,
            path_limit: None,
        }
    }

    /// FAT32, exFAT, or NTFS, written through this host's own driver
    /// (`platform::WINDOWS_FORMAT`): 255 UTF-16 units.
    pub const fn windows_format() -> Self {
        Self {
            format: NameFormat::Windows,
            windows_characters: platform::WINDOWS_FORMAT.characters,
            reserved_names: platform::WINDOWS_FORMAT.reserved_names,
            name_limit: LengthLimit::UTF16_255,
            path_limit: None,
        }
    }

    /// These rules with `path_limit` as the path ceiling.
    pub const fn with_path_limit(self, path_limit: Option<LengthLimit>) -> Self {
        Self { path_limit, ..self }
    }

    /// These rules for a disk this host writes through its own path API, never weaker than
    /// what that API itself refuses (`platform::LOCAL_FLOOR`), whatever the filesystem kind
    /// lookup said. On Windows that's every Win32 refusal (a backslash, a colon, a control
    /// character, a device name, a trailing space or period), since a `\\?\` path would
    /// otherwise write an NTFS stream, a name Explorer can't open, or into a subfolder. On
    /// macOS and Linux the kind's tiers stand.
    pub fn for_host_local_disk(self) -> Self {
        self.at_least(platform::LOCAL_FLOOR)
    }

    /// These rules with each Windows tier raised to at least `floor`'s. The limits stay.
    pub(crate) fn at_least(self, floor: WindowsFormatTiers) -> Self {
        Self {
            windows_characters: self.windows_characters.max(floor.characters),
            reserved_names: self.reserved_names.max(floor.reserved_names),
            ..self
        }
    }
}

/// How one host's driver writes a Windows-format disk, per class of name.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowsFormatTiers {
    characters: NameTier,
    reserved_names: NameTier,
}

/// The longest path the host's own path API takes, for a destination it
/// reaches through that API (the local disk, a mounted share). macOS: 1,023
/// bytes (`PATH_MAX` counts the NUL), Linux 4,095 bytes, Windows 32,767 UTF-16
/// units through `\\?\` paths.
pub const HOST_PATH_LIMIT: LengthLimit = platform::HOST_PATH_LIMIT;

/// The name limit of the host's own system disk (APFS, ext4, NTFS), for a path
/// typed before any destination answers.
pub const HOST_NAME_LIMIT: LengthLimit = platform::HOST_NAME_LIMIT;

/// The device names Windows reserves in every folder, matched ignoring case
/// against a name's part before its first period, trailing spaces trimmed
/// (`nul.tar.gz` and `CON .txt` are reserved too). Microsoft's list, superscript
/// digits included: "Naming Files, Paths, and Namespaces", learn.microsoft.com,
/// read 2026-11-18. Windows 11 opens `con.txt` as a file (22000 and later) but
/// Windows 10 still opens the console, so the list keeps the extension forms.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹",
    "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²",
    "LPT³",
];

/// The printable characters a Windows name can't hold (MS-FSCC § 2.1.5.2).
const WINDOWS_FORBIDDEN_CHARACTERS: &[char] = &['"', '*', ':', '<', '>', '?', '\\', '|'];

/// Checks `name` for a destination that follows `rules`: `Err` when it refuses
/// the name, `Ok(Some(warning))` when it takes the name but won't keep it
/// portable, `Ok(None)` when all's well.
///
/// The input is checked as-is (callers trim if they want to). In order:
/// - Empty, or rendering as nothing ([`renders_blank`]): refused everywhere
/// - `/` or NUL: refused everywhere
/// - `.` or `..`: refused everywhere
/// - Longer than [`NameRules::name_limit`], in its unit
/// - A character Windows can't use, a reserved device name, a trailing space
///   or period: refused, warned about, or fine, per [`NameRules`]' tiers. A
///   refusal anywhere beats a warning; among warnings, the first is named.
pub fn validate_filename(name: &str, rules: NameRules) -> Result<Option<PortabilityWarning>, ValidationError> {
    if name.chars().all(renders_blank) {
        return Err(ValidationError::Empty);
    }
    if let Some(character) = name.chars().find_map(posix_disallowed) {
        return Err(ValidationError::DisallowedCharacter { character });
    }
    if name == "." || name == ".." {
        return Err(ValidationError::DotName);
    }
    if let Some(length) = rules.name_limit.exceeded_by(name) {
        return Err(ValidationError::NameTooLong {
            length,
            max: rules.name_limit.max,
            unit: rules.name_limit.unit,
        });
    }

    let mut warning = None;
    let mut note = |tier: NameTier, refusal: ValidationError, warn: PortabilityWarning| match tier {
        NameTier::Allow => Ok(()),
        NameTier::Warn => {
            warning.get_or_insert(warn);
            Ok(())
        }
        NameTier::Refuse => Err(refusal),
    };
    if let Some(character) = name.chars().find_map(windows_character) {
        note(
            rules.windows_characters,
            ValidationError::DisallowedCharacter {
                character: character.clone(),
            },
            PortabilityWarning::WindowsCharacter { character },
        )?;
    }
    if let Some(reserved) = reserved_device_name(name) {
        note(
            rules.reserved_names,
            ValidationError::ReservedName {
                name: reserved.to_string(),
            },
            PortabilityWarning::ReservedName {
                name: reserved.to_string(),
            },
        )?;
    }
    if name.ends_with([' ', '.']) {
        note(
            rules.windows_characters,
            ValidationError::EndsWithSpaceOrPeriod,
            PortabilityWarning::EndsWithSpaceOrPeriod,
        )?;
    }
    Ok(warning)
}

/// Every class of Windows trouble `name` has, whatever the destination, in
/// [`validate_filename`]'s order (a character, a device name, a trailing space
/// or period): the scan's tally reads it, then the destination's [`NameRules`]
/// say whether each is refused, warned about, or fine there. A name in several
/// classes (`CON.a:b`, `a?.`) answers each, so a destination that warns about
/// only one of them still hears of it. All `None` for a name Windows takes as
/// is. Allocates only for a hit's character.
pub fn windows_portability(name: &str) -> [Option<PortabilityWarning>; 3] {
    if name.chars().all(renders_blank) {
        return [None, None, None];
    }
    [
        name.chars()
            .find_map(windows_character)
            .map(|character| PortabilityWarning::WindowsCharacter { character }),
        reserved_device_name(name).map(|reserved| PortabilityWarning::ReservedName {
            name: reserved.to_string(),
        }),
        name.ends_with([' ', '.'])
            .then_some(PortabilityWarning::EndsWithSpaceOrPeriod),
    ]
}

/// How a character no filesystem takes reads in a refusal: `/` itself, NUL by name.
fn posix_disallowed(ch: char) -> Option<String> {
    match ch {
        '/' => Some("/".to_string()),
        '\0' => Some("NUL".to_string()),
        _ => None,
    }
}

/// How a character Windows can't use reads: itself when it's visible, its
/// `U+` code when it's a control character.
fn windows_character(ch: char) -> Option<String> {
    match ch {
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

/// Whether `ch` shows nothing on its own: Unicode `White_Space`, an invisible
/// format character ([`is_invisible_format`]), or one of the other characters
/// that render blank: the combining grapheme joiner, the Hangul fillers, the
/// Khmer inherent vowels, the Mongolian variation selectors and vowel
/// separator, the invisible math operators, the deprecated format characters,
/// the variation selectors, the shorthand and musical format controls, the
/// tag characters, and the braille blank. A name made only of these is empty.
/// ❗ The frontend's `rendersBlank` mirrors this list (`name_rules_cases.json`
/// pins both).
pub fn renders_blank(ch: char) -> bool {
    ch.is_whitespace()
        || is_invisible_format(ch)
        || matches!(
            ch,
            '\u{034F}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{3164}'
                | '\u{FFA0}'
                | '\u{17B4}'
                | '\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{2061}'..='\u{2064}'
                | '\u{206A}'..='\u{206F}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
                | '\u{2800}'
        )
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

/// Whether `name` is longer than ANY filesystem Cmdr writes to can store: past
/// 255 bytes AND past 255 UTF-16 units. A transfer refuses only such a name up
/// front; anything shorter is the destination's own answer.
pub fn over_every_name_limit(name: &str) -> bool {
    LengthLimit::BYTES_255.exceeded_by(name).is_some() && LengthLimit::UTF16_255.exceeded_by(name).is_some()
}

/// Validates that a full path fits the destination's [`NameRules::path_limit`],
/// inclusive, in its unit. No limit means the backend answers for itself.
pub fn validate_path_length(path: &std::path::Path, rules: NameRules) -> Result<(), ValidationError> {
    let Some(limit) = rules.path_limit else {
        return Ok(());
    };
    match limit.exceeded_by(&path.to_string_lossy()) {
        None => Ok(()),
        Some(length) => Err(ValidationError::PathTooLong {
            length,
            max: limit.max,
            unit: limit.unit,
        }),
    }
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

    #[test]
    fn the_host_path_limit_holds_inclusive() {
        let rules = NameRules::POSIX.with_path_limit(Some(HOST_PATH_LIMIT));
        let at_limit = "/".to_string() + &"a".repeat(HOST_PATH_LIMIT.max - 1);
        assert!(validate_path_length(Path::new(&at_limit), rules).is_ok());
        let past = at_limit + "a";
        assert!(matches!(
            validate_path_length(Path::new(&past), rules),
            Err(ValidationError::PathTooLong { .. })
        ));
    }

    #[test]
    fn only_a_name_past_both_units_is_too_long_everywhere() {
        assert!(!over_every_name_limit(&"é".repeat(200)));
        assert!(over_every_name_limit(&"a".repeat(256)));
        assert!(over_every_name_limit(&"😀".repeat(128)));
    }

    #[test]
    fn the_tally_reads_every_class_whatever_the_destination() {
        let character = |c: &str| Some(PortabilityWarning::WindowsCharacter { character: c.into() });
        let reserved = |n: &str| Some(PortabilityWarning::ReservedName { name: n.into() });
        let trailing = Some(PortabilityWarning::EndsWithSpaceOrPeriod);
        assert_eq!(windows_portability("a:b"), [character(":"), None, None]);
        assert_eq!(windows_portability("con.txt"), [None, reserved("con"), None]);
        assert_eq!(windows_portability("x."), [None, None, trailing.clone()]);
        assert_eq!(
            windows_portability("CON.a:b?."),
            [character(":"), reserved("CON"), trailing],
            "every class a name falls in"
        );
        assert_eq!(windows_portability("fine.txt"), [None, None, None]);
        assert_eq!(
            windows_portability(&"a".repeat(300)),
            [None, None, None],
            "length isn't a portability class"
        );
    }
}
