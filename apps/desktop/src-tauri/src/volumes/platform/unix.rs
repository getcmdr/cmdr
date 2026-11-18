//! What macOS and Linux share: the host's local volume is `LocalPosixVolume`, rooted at `/`.

use std::path::Path;
use std::sync::Arc;

use cmdr_fs::volume::{MountClass, Volume};
use cmdr_local::LocalVolume;

/// The whole filesystem at `/`, under the name this OS gives its startup disk.
pub(crate) fn root_volume_named(name: &str) -> Arc<dyn Volume> {
    Arc::new(LocalVolume::local_folder(name, "/", crate::volume_host::host()))
}

pub(crate) fn local_volume_on_mount(name: &str, root: &Path, mount: MountClass) -> Arc<dyn Volume> {
    Arc::new(LocalVolume::on_mount(name, root, mount, crate::volume_host::host()))
}

pub(crate) fn is_local_volume(volume: &dyn Volume) -> bool {
    volume.as_any().is::<LocalVolume>()
}
