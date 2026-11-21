//! The host's own disk, as the engine's shared code needs it: a file's device
//! and mode, the no-replace rename, mount and file-size questions, a
//! working-copy file, and a volume over a local folder.
//!
//! The one place (with the local-engine trees, `transfer/copy/` and friends)
//! that names `cmdr_local`, `libc`, or `std::os::unix`. What differs per OS sits
//! behind `platform/`, one arm per OS plus the browser build's; what only tells
//! the desktop from the browser stays here. The browser build has no local
//! disk, so there each answers what a host without one would; nothing reaches
//! them there, because that host's `OperationVolumes::local_disk` is `None` and
//! every local-disk branch asks it first (`host/DETAILS.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::host::listings::ListingHost;
use cmdr_fs::volume::{MutationEvent, Volume, VolumeError};

/// The per-OS arms. Reached by path only for `symlink`, which macOS's local copy
/// never asks (it copies a link with `copyfile`); everything else is re-exported
/// below.
pub(crate) mod platform;

pub(crate) use platform::locked_against_removal;
#[cfg(target_os = "macos")]
pub(crate) use platform::{BsdFlags, bsd_flags, read_xattr};
pub(crate) use platform::{
    WriteProbe, create_dir_with_mode, free_bytes, important_free_bytes, is_locked_item, mode_of, set_mode, with_mode,
    write_probe,
};

/// `rename(2)` that fails with `AlreadyExists` instead of clobbering `destination`
/// (`cmdr_local::rename_local_exclusive`).
pub(crate) fn rename_exclusive(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(not(target_family = "wasm"))]
    {
        cmdr_local::rename_local_exclusive(source, destination)
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = (source, destination);
        Err(no_local_disk())
    }
}

/// A failed local rename as the volume vocabulary words it.
pub(crate) fn rename_error(err: &std::io::Error, from: &Path, to: &Path) -> VolumeError {
    #[cfg(not(target_family = "wasm"))]
    {
        cmdr_local::rename_volume_error(err, from, to)
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = (err, from, to);
        VolumeError::NotSupported
    }
}

/// Every mount point in the OS mount table, or `None` when it can't be read
/// (and with no local disk, which has no mount table).
pub(crate) fn mount_points() -> Option<Vec<PathBuf>> {
    #[cfg(not(target_family = "wasm"))]
    {
        cmdr_local::mounts::list()
    }
    #[cfg(target_family = "wasm")]
    {
        None
    }
}

/// Whether `root` is where a filesystem is mounted, by the OS mount table; `None`
/// when the table can't say (or there's no local disk).
pub(crate) fn is_mount_point(root: &Path) -> Option<bool> {
    #[cfg(not(target_family = "wasm"))]
    {
        cmdr_local::mounts::is_mount_point(root)
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = root;
        None
    }
}

/// Whether `path` sits on a network mount.
pub(crate) fn is_on_network_mount(path: &Path) -> bool {
    #[cfg(not(target_family = "wasm"))]
    {
        cmdr_local::filesystem_kind::path_is_on_network_mount(path)
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = path;
        false
    }
}

/// What filesystem holds `path` (its kind and limits); `None` with no local disk.
pub(crate) fn filesystem_at(path: &Path) -> Option<cmdr_fs::filesystem_kind::FilesystemInfo> {
    #[cfg(not(target_family = "wasm"))]
    {
        Some(cmdr_local::filesystem_kind::detect_filesystem_for_path(path))
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = path;
        None
    }
}

/// Whether the filesystem holding `path` keeps a node's inode and birth time for
/// as long as the node lives, so the pair still names it when the operation log
/// is read back months later (`cmdr_fs::volume::NodeIdentity`).
///
/// An allowlist of the disk formats that store both. Not FAT32 or exFAT, which
/// derive the inode from the file's first cluster
/// (`FilesystemKind::has_stable_inodes`); not a network or FUSE mount, whose
/// numbers are the server's or the driver's to change; not NTFS, which reaches a
/// Mac or a Linux box through drivers that differ on both. A wrong "no" costs a
/// row its node check and nothing else; a wrong "yes" would make an undo refuse
/// every item it landed there.
pub(crate) fn keeps_node_identity(path: &Path) -> bool {
    use cmdr_fs::filesystem_kind::FilesystemKind;
    filesystem_at(path).is_some_and(|filesystem| {
        matches!(
            filesystem.kind,
            FilesystemKind::Apfs
                | FilesystemKind::HfsPlus
                | FilesystemKind::Ext4
                | FilesystemKind::Btrfs
                | FilesystemKind::Xfs
                | FilesystemKind::Zfs
        )
    })
}

/// Tells the panes about a create on the local disk.
pub(crate) fn patch_listings_after(listings: &dyn ListingHost, volume_id: &str, parent: &Path, event: MutationEvent) {
    #[cfg(not(target_family = "wasm"))]
    {
        cmdr_local::mutation::patch_listing_after_local_mutation(listings, volume_id, parent, event);
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = (listings, volume_id, parent, event);
    }
}

/// A volume over the local folder `root`, for a working copy or a spool.
pub(crate) fn folder_volume(name: &str, root: impl Into<PathBuf>, host: VolumeHost) -> Arc<dyn Volume> {
    platform::folder_volume(name, root.into(), host)
}

/// The person's home folder (`None` in the browser build, which has none).
pub(crate) fn home_dir() -> Option<PathBuf> {
    #[cfg(not(target_family = "wasm"))]
    {
        dirs::home_dir()
    }
    #[cfg(target_family = "wasm")]
    {
        None
    }
}

/// Every refusal a browser-build stand-in gives.
#[cfg(target_family = "wasm")]
fn no_local_disk() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, "no local disk")
}
