//! Linux: `PATH_MAX` is 4,096 bytes with its NUL, ext4 names are 255 bytes. The
//! kernel's vfat and exfat drivers refuse the characters Windows can't use and a
//! control character (`EINVAL`), and they STRIP a trailing period: `touch dup.`
//! beside `dup` opens `dup`, so taking one could write over a sibling. A device
//! name lands as itself, which only Windows can't open. Verified on kernel 7.2.12
//! (OrbStack), loop-mounting the `hdiutil` images in an Alpine container,
//! 2026-11-18 (`docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`).
//! NTFS (`ntfs3`) is held to the same rules: untested, and refusing is the safe side.

use super::super::{LengthLimit, LengthUnit, NameTier, WindowsFormatTiers};

pub(crate) const HOST_PATH_LIMIT: LengthLimit = LengthLimit {
    max: 4095,
    unit: LengthUnit::Bytes,
};

pub(crate) const HOST_NAME_LIMIT: LengthLimit = LengthLimit::BYTES_255;

pub(crate) const WINDOWS_FORMAT: WindowsFormatTiers = WindowsFormatTiers {
    characters: NameTier::Refuse,
    reserved_names: NameTier::Warn,
};
