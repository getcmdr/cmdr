//! Both Unixes answer from the `stat` the metadata already holds: no extra syscall.

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use super::super::{DeviceId, FileId, Follow};

pub(crate) fn file_id_at(path: &Path, follow: Follow) -> std::io::Result<Option<FileId>> {
    let metadata = match follow {
        Follow::Links => std::fs::metadata(path)?,
        Follow::NotLinks => std::fs::symlink_metadata(path)?,
    };
    Ok(file_id(&metadata))
}

pub(crate) fn file_id_of_open(file: &std::fs::File) -> Option<FileId> {
    file_id(&file.metadata().ok()?)
}

pub(crate) fn file_id(metadata: &Metadata) -> Option<FileId> {
    Some(FileId::from_parts(
        DeviceId::from_raw(metadata.dev()),
        u128::from(metadata.ino()),
    ))
}

pub(crate) fn device_of(metadata: &Metadata) -> Option<DeviceId> {
    Some(DeviceId::from_raw(metadata.dev()))
}

pub(crate) fn device_at(path: &Path) -> Option<DeviceId> {
    device_of(&std::fs::metadata(path).ok()?)
}

pub(crate) fn link_count(metadata: &Metadata) -> Option<u64> {
    Some(metadata.nlink())
}
