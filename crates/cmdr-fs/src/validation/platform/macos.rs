//! macOS: `PATH_MAX` is 1,024 bytes with its NUL, APFS names are 255 UTF-16 units,
//! and the FSKit msdos and exfat drivers take every name, writing the characters
//! Windows can't use, a trailing space or period, as private-use stand-ins, and a
//! device name as itself (verified on macOS 27.0 with `hdiutil` images,
//! `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`, 2026-11-18). So
//! nothing is refused there; everything is warned about.

use super::super::{LengthLimit, LengthUnit, NameTier, WindowsFormatTiers};

pub(crate) const HOST_PATH_LIMIT: LengthLimit = LengthLimit {
    max: 1023,
    unit: LengthUnit::Bytes,
};

pub(crate) const HOST_NAME_LIMIT: LengthLimit = LengthLimit::UTF16_255;

pub(crate) const WINDOWS_FORMAT: WindowsFormatTiers = WindowsFormatTiers {
    characters: NameTier::Warn,
    reserved_names: NameTier::Warn,
};
