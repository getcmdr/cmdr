//! The browser build: the answers of a host with no local disk. Nothing reaches
//! them there (`OperationVolumes::local_disk` is `None`, `host/DETAILS.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

use super::super::{WriteProbe, no_local_disk};

pub(crate) fn mode_of(permissions: &std::fs::Permissions) -> u32 {
    if permissions.readonly() { 0o444 } else { 0o644 }
}

pub(crate) fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Err(no_local_disk())
}

pub(crate) fn create_dir_with_mode(dir: &Path, _mode: u32) -> std::io::Result<()> {
    std::fs::create_dir(dir)
}

pub(crate) fn with_mode(options: &mut std::fs::OpenOptions, _mode: u32) -> &mut std::fs::OpenOptions {
    options
}

pub(crate) fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(no_local_disk())
}

pub(crate) fn write_probe(_path: &Path) -> WriteProbe {
    WriteProbe::Unknown
}

pub(crate) fn free_bytes(_path: &Path) -> Option<u64> {
    None
}

pub(crate) fn important_free_bytes(_path: &Path) -> Option<u64> {
    None
}

pub(crate) fn is_locked_item(_path: &Path) -> bool {
    false
}

pub(crate) fn locked_against_removal(_path: &Path) -> bool {
    false
}

pub(crate) const ASKS_ABOUT_LOCKS: bool = false;

/// Unreachable there (archive edits refuse on a host with no local disk); an
/// empty in-memory folder is the honest stand-in for "no disk here".
pub(crate) fn folder_volume(name: &str, root: PathBuf, _host: VolumeHost) -> Arc<dyn Volume> {
    Arc::new(cmdr_fs::volume::InMemoryVolume::new(name).with_root(root))
}
