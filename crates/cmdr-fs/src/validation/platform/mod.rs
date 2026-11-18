//! Picks how this OS's own drivers take names: the path limit of its path API,
//! its system disk's name limit, and what its FAT32, exFAT, and NTFS drivers do
//! with a name Windows can't use.

use super::{LengthLimit, WindowsFormatTiers};

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
// The browser build has no drivers of its own; its demo plays a Mac, so it takes the macOS arm.
#[cfg_attr(target_family = "wasm", path = "macos.rs")]
mod imp;

const _: LengthLimit = HOST_PATH_LIMIT;
const _: LengthLimit = HOST_NAME_LIMIT;
const _: WindowsFormatTiers = WINDOWS_FORMAT;
const _: WindowsFormatTiers = LOCAL_FLOOR;

/// The longest path this OS's path API takes, inclusive.
pub(super) use imp::HOST_PATH_LIMIT;

/// The name limit of this OS's system disk.
pub(super) use imp::HOST_NAME_LIMIT;

/// What this OS's drivers do with a Windows-unfriendly name on a FAT32, exFAT,
/// or NTFS disk: store a stand-in (warn), or refuse or mangle it (refuse).
pub(super) use imp::WINDOWS_FORMAT;

/// The weakest rules a disk this OS writes through its own path API may get
/// (`NameRules::for_host_local_disk`): Win32's refusals on Windows, none elsewhere.
pub(super) use imp::LOCAL_FLOOR;
