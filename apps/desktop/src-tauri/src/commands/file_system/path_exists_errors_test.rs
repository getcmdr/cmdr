//! What `path_exists` says when a local `stat` fails for a reason other than "not
//! there": "couldn't tell", ❌ never "gone". A FUSE mount that answers EIO or
//! ETIMEDOUT (pCloud's `pcloudfs`, cmdr-reports#4) otherwise reads as deleted, and
//! the pane's walk-up leaves the drive for the boot disk's home folder.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::file_system::volume::manager::get_volume_manager;
use crate::file_system::volume::{ListingProgress, Volume, VolumeError};
use cmdr_fs::entry::FileEntry;

use super::path_exists;

/// The rule itself, by error kind, on every OS: only "not there" and "not a folder" are a
/// miss. The errno twin below pins how POSIX's codes reach those kinds.
#[test]
fn only_the_not_found_kinds_are_a_definite_miss() {
    use super::local_miss_is_definite;
    use std::io::{Error, ErrorKind};

    for kind in [ErrorKind::NotFound, ErrorKind::NotADirectory] {
        assert!(local_miss_is_definite(&Error::from(kind)), "{kind:?}");
    }
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::TimedOut,
        ErrorKind::NotConnected,
        ErrorKind::Other,
    ] {
        assert!(!local_miss_is_definite(&Error::from(kind)), "{kind:?}");
    }
}

#[test]
#[cfg(unix)] // POSIX errnos (`libc`); Windows reads its own error codes
fn only_not_found_and_not_a_folder_are_a_definite_miss() {
    use super::local_miss_is_definite;
    use std::io;

    for errno in [libc::ENOENT, libc::ENOTDIR] {
        assert!(
            local_miss_is_definite(&io::Error::from_raw_os_error(errno)),
            "errno {errno}"
        );
    }
    for errno in [
        libc::EIO,
        libc::ETIMEDOUT,
        libc::EACCES,
        libc::ENXIO,
        libc::EPERM,
        libc::ENOTCONN,
    ] {
        assert!(
            !local_miss_is_definite(&io::Error::from_raw_os_error(errno)),
            "errno {errno}"
        );
    }
}

/// A real non-NotFound error end to end: `stat` under a folder nobody may search
/// answers EACCES, which says nothing about whether the child is there.
#[tokio::test]
#[cfg(unix)] // a folder's search bit (POSIX modes) is what refuses the `stat`
async fn a_stat_the_os_refuses_is_couldnt_tell() {
    use crate::test_support::TestDir;
    use cmdr_local::LocalPosixVolume;
    use std::os::unix::fs::PermissionsExt;

    // Root bypasses the search bit (CAP_DAC_OVERRIDE), so EACCES can't be produced
    // there, and the local Linux test lane runs in Docker as root.
    // SAFETY: geteuid takes no arguments, touches no memory, and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let dir = TestDir::new("path-exists-refused");
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::create_dir(locked.join("child")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let id = format!("path-exists-errors-{}", uuid::Uuid::new_v4());
    let volume = LocalPosixVolume::local_folder("Temp", &*dir, crate::volume_host::host());
    get_volume_manager().register(&id, Arc::new(volume) as Arc<dyn Volume>);
    let child = locked.join("child").to_string_lossy().into_owned();
    let refused = path_exists(Some(id.clone()), child).await;
    let gone = path_exists(Some(id.clone()), dir.join("nope").to_string_lossy().into_owned()).await;
    get_volume_manager().unregister(&id);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(refused.timed_out && !refused.data, "{refused:?}");
    assert!(!gone.timed_out && !gone.data, "{gone:?}");
}

/// A remote volume (no local path) that refuses one folder and holds nothing else.
struct RefusingVolume {
    root: &'static AppPath,
}

impl Volume for RefusingVolume {
    fn name(&self) -> &str {
        "refusing"
    }

    fn root(&self) -> &AppPath {
        self.root
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn list_directory<'a>(
        &'a self,
        _path: &'a AppPath,
        _on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        let shown = path.as_str().to_owned();
        Box::pin(async move {
            if shown.ends_with("/private") {
                Err(VolumeError::PermissionDenied {
                    path: shown,
                    raw_os_error: None,
                })
            } else {
                Err(VolumeError::NotFound(shown))
            }
        })
    }
}

/// #1018: a share folder the account may not read is "couldn't tell", ❌ never "gone".
/// A "gone" walked the pane to the parent as if the folder were deleted.
#[tokio::test]
async fn a_remote_refusal_is_couldnt_tell_and_only_not_found_is_gone() {
    let id = format!("path-exists-refusing-{}", uuid::Uuid::new_v4());
    let volume = RefusingVolume {
        root: app_path!("/Volumes/refusing"),
    };
    get_volume_manager().register(&id, Arc::new(volume) as Arc<dyn Volume>);
    let refused = path_exists(Some(id.clone()), "/Volumes/refusing/private".to_string()).await;
    let gone = path_exists(Some(id.clone()), "/Volumes/refusing/nope".to_string()).await;
    get_volume_manager().unregister(&id);

    assert!(refused.timed_out && !refused.data, "{refused:?}");
    assert!(!gone.timed_out && !gone.data, "{gone:?}");
}
