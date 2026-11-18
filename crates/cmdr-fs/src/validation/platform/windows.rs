//! Windows: a `\\?\` path reaches 32,767 UTF-16 units (`cmdr_fs::volume::path_grammar`
//! builds them), NTFS names are 255 units. Win32 refuses the characters it can't
//! use, strips a trailing space or period (so `dup.` opens `dup`), and opens a
//! device for a reserved name, `con.txt` included on Windows 10 (Windows 11 build
//! 22000 and later opens it as a file). Every class is refused: each one would
//! write somewhere other than the name typed.

use super::super::{LengthLimit, LengthUnit, NameTier, WindowsFormatTiers};

pub(crate) const HOST_PATH_LIMIT: LengthLimit = LengthLimit {
    max: 32_767,
    unit: LengthUnit::Utf16Units,
};

pub(crate) const HOST_NAME_LIMIT: LengthLimit = LengthLimit::UTF16_255;

pub(crate) const WINDOWS_FORMAT: WindowsFormatTiers = WindowsFormatTiers {
    characters: NameTier::Refuse,
    reserved_names: NameTier::Refuse,
};

/// Win32 refuses these on every local volume, whatever its filesystem: a `\\?\` path
/// skips Win32's name rules, so taking one would write an NTFS stream (`a:b`), a name
/// Explorer can't open (`con`, `dup.`), or into a subfolder (`x\y`).
pub(crate) const LOCAL_FLOOR: WindowsFormatTiers = WINDOWS_FORMAT;
