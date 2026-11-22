//! The browser build has no local disk, so no metadata ever describes a real node.

use std::fs::Metadata;
use std::path::Path;

use super::super::{DeviceId, FileId, Follow};

pub(crate) fn file_id(_metadata: &Metadata) -> Option<FileId> {
    None
}

pub(crate) fn file_id_at(_path: &Path, _follow: Follow) -> std::io::Result<Option<FileId>> {
    Ok(None)
}

pub(crate) fn file_id_of_open(_file: &std::fs::File) -> Option<FileId> {
    None
}

pub(crate) fn device_of(_metadata: &Metadata) -> Option<DeviceId> {
    None
}

pub(crate) fn device_at(_path: &Path) -> Option<DeviceId> {
    None
}

pub(crate) fn link_count(_metadata: &Metadata) -> Option<u64> {
    None
}
