//! Picks this OS's reader for [`FileId`], [`DeviceId`], and the link count.

use std::fs::Metadata;
use std::path::Path;

use super::{DeviceId, FileId, Follow};

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
mod imp;

const _: fn(&Metadata) -> Option<FileId> = file_id;
const _: fn(&Path, Follow) -> std::io::Result<Option<FileId>> = file_id_at;
const _: fn(&Metadata) -> Option<DeviceId> = device_of;
const _: fn(&Path) -> Option<DeviceId> = device_at;
const _: fn(&Metadata) -> Option<u64> = link_count;

/// The node `metadata` describes, or `None` where this OS's metadata carries no id.
pub(super) use imp::file_id;

/// The node at a path, read by path: `Err` when the path can't be opened or read (its kind
/// tells a missing path apart), `Ok(None)` when it can but carries no usable id.
pub(super) use imp::file_id_at;

/// The device `metadata`'s node lives on, or `None` where this OS's metadata doesn't say.
pub(super) use imp::device_of;

/// The device the node at a path lives on, links followed, or `None` when the path can't be
/// read or this OS gives no device.
pub(super) use imp::device_at;

/// The node's name count, or `None` where this OS's metadata doesn't say.
pub(super) use imp::link_count;
