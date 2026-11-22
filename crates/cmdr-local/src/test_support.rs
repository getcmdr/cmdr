//! What this crate's own cells share: an app path under a volume's root and a volume over a
//! scratch folder, each the same on every OS.

use cmdr_fs::app_path::{AppPathBuf, RelativeAppPath};
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;

use crate::LocalVolume;

/// The app path `rel` (`/`-separated names, `""` for the root itself) names under `volume`'s
/// root, by the app path's own join, so a cell reads the same on every OS.
#[track_caller]
pub(crate) fn at(volume: &(impl Volume + ?Sized), rel: &str) -> AppPathBuf {
    let rel = RelativeAppPath::new(rel).unwrap_or_else(|why| panic!("{rel:?} is no relative app path: {why}"));
    volume
        .root()
        .join_relative(rel)
        .unwrap_or_else(|why| panic!("{rel:?} joins no app path: {why}"))
}

/// This OS's local volume over `dir`, the way the app registers one: rooted at an app path,
/// with a detached host.
pub(crate) fn volume_at(dir: &AppTestDir) -> LocalVolume {
    LocalVolume::local_folder_at("Test", dir.app_path().to_owned(), VolumeHost::detached())
}
