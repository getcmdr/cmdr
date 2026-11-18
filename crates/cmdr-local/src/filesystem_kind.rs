//! Resolving a path to its OS filesystem-type string.
//!
//! The platform-specific half of filesystem identity. The classification itself
//! (`FilesystemKind`, `MaxFileSize`, `FilesystemInfo`, and the kind → limit map)
//! is platform-free and lives in `cmdr_fs::filesystem_kind`; callers name it
//! there. Also the "is this on a network mount?" question asked of the same
//! probe.

use cmdr_fs::filesystem_kind::{FilesystemInfo, mount_is_local_disk};
#[cfg(doc)]
use cmdr_fs::filesystem_kind::{FilesystemKind, MaxFileSize};

/// One mount probe: the filesystem a path lives on, plus the mount's source
/// (`/dev/disk3s1`, `//me@nas/share`, whatever a FUSE mount chose). `None`
/// source when the probe couldn't resolve the mount.
pub struct MountProbe {
    /// The filesystem the path lives on.
    pub info: FilesystemInfo,
    /// The mount's source, `None` when the probe couldn't resolve the mount.
    pub source: Option<String>,
}

/// Detects the filesystem at `path` by resolving its mount and reading the OS
/// filesystem-type string, then classifying it.
///
/// Returns [`FilesystemKind::Other`] / [`MaxFileSize::Unknown`] when the type
/// can't be resolved, so the write guard never blocks on a guess.
///
/// macOS resolves via `statfs.f_fstypename`; other Unix via `/proc/mounts`.
/// The single `statfs` is fast on local mounts (the only ones that reach the
/// local-FS copy/move path); a hung network mount would already have stalled the
/// preceding free-space query on the same destination.
pub fn detect_filesystem_for_path(path: &std::path::Path) -> FilesystemInfo {
    probe_mount_for_path(path).info
}

/// [`detect_filesystem_for_path`] plus the mount's source, from the same single probe.
#[cfg(target_os = "macos")]
pub fn probe_mount_for_path(path: &std::path::Path) -> MountProbe {
    let mount = super::statfs::get_mount_info(&path.to_string_lossy());
    let (raw, source) = match mount {
        Some(m) => (Some(m.fs_type), Some(m.source)),
        None => (None, None),
    };
    MountProbe {
        info: FilesystemInfo::from_raw_type(raw),
        source,
    }
}

/// [`detect_filesystem_for_path`] plus the mount's source, from the same single probe.
#[cfg(target_os = "linux")]
pub fn probe_mount_for_path(path: &std::path::Path) -> MountProbe {
    let entry = super::linux_mounts::mount_entry_for_path(path);
    let (raw, source) = match entry {
        Some(e) => (Some(e.fstype), Some(e.device)),
        None => (None, None),
    };
    MountProbe {
        info: FilesystemInfo::from_raw_type(raw),
        source,
    }
}

/// [`detect_filesystem_for_path`] on Windows: the volume's filesystem name (`NTFS`, `FAT32`,
/// `exFAT`, `ReFS`) from `GetVolumeInformationW`, for a fixed or removable drive only. A mapped
/// network drive reports the server's `NTFS` too, so it stays unknown (`Other`, and network to
/// [`is_network`]); no source string, since Windows has no mount device to name.
#[cfg(target_os = "windows")]
pub fn probe_mount_for_path(path: &std::path::Path) -> MountProbe {
    MountProbe {
        info: FilesystemInfo::from_raw_type(windows_volume::local_filesystem_name(path)),
        source: None,
    }
}

#[cfg(target_os = "windows")]
mod windows_volume {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW};

    /// `GetDriveTypeW`'s `DRIVE_REMOVABLE` and `DRIVE_FIXED` (`WindowsProgramming`'s
    /// constants, a feature this crate doesn't otherwise need).
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    /// The filesystem name of the local volume holding `path`, `None` for a network or
    /// optical drive, or when Windows can't say.
    pub(super) fn local_filesystem_name(path: &Path) -> Option<String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // The volume root is never longer than the path itself (plus its trailing `\`).
        let mut root = vec![0u16; wide.len().max(261) + 1];
        let root_len = u32::try_from(root.len()).ok()?;
        // SAFETY: `wide` is NUL-terminated and `root` holds `root_len` units.
        if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root_len) } == 0 {
            return None;
        }
        // SAFETY: `root` is the NUL-terminated root `GetVolumePathNameW` wrote.
        let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
        if drive_type != DRIVE_FIXED && drive_type != DRIVE_REMOVABLE {
            return None;
        }
        // `MAX_PATH + 1`, the documented maximum for the filesystem name.
        let mut name = [0u16; 261];
        let name_len = u32::try_from(name.len()).ok()?;
        // SAFETY: every out pointer is null (not wanted) except `name`, which holds its length.
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                name.as_mut_ptr(),
                name_len,
            )
        };
        if ok == 0 {
            return None;
        }
        let len = name.iter().position(|&unit| unit == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..len]))
    }
}

/// Whether `path` sits on anything but a known local disk: a network share (SMB,
/// NFS, AFP, WebDAV, ...), a FUSE or cloud mount (`pcloudfs`, rclone, sshfs), or a
/// mount the probe couldn't resolve.
///
/// One `statfs` (a `/proc/mounts` read on Linux), so ❌ never call it in a loop
/// over entries or on a hot read path: it can block for as long as the mount
/// takes to answer. The index's `mount_facts` is one caller (the walker
/// choice); `watcher::start_watching` is another, deciding a watch's
/// [`WatchCoverage`](cmdr_fs::volume::WatchCoverage) once at arm time rather than
/// per query. Both want the same answer: a remote tree can't be walked at local
/// speed, and its other writers never reach FSEvents.
pub fn path_is_on_network_mount(path: &std::path::Path) -> bool {
    is_network(&probe_mount_for_path(path))
}

/// "Network" here means "not a known local disk" (`cmdr_fs::filesystem_kind::mount_is_local_disk`), so
/// FUSE and cloud mounts count too. Split out so one probe can answer both of
/// the index's `mount_facts` questions.
pub fn is_network(probe: &MountProbe) -> bool {
    !mount_is_local_disk(probe.source.as_deref(), probe.info.raw_type.as_deref())
}
