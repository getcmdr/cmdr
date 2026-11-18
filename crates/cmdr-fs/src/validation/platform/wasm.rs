//! The browser build has no drivers of its own; its demo plays a Mac, so it
//! answers as macOS does.

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
