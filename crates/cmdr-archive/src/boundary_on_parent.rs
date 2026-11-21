//! The parent-aware half of the boundary confirm: the same "is this component a
//! real archive?" question as [`confirm_archive_boundary`], asked of whatever
//! volume holds the archive.
//!
//! [`confirm_archive_boundary`] is `std::fs`-only, so it answers for a LOCAL
//! parent alone. A remote parent (direct SMB, MTP, a restic snapshot) holds a
//! `.zip` the local filesystem can't reach, so it confirms through its own
//! `get_metadata` and `read_range`, sniffed with the SAME magic predicate
//! ([`bytes_match_archive_magic`]), so local and remote agree on what "is a
//! `<format>`" means. One detector, never forked.

use cmdr_fs::app_path::{AppPath, AppPathBuf, RelativeAppPathBuf};
use std::path::{Path, PathBuf};

use cmdr_fs::archive_format::archive_boundary_candidate;
use cmdr_fs::volume::Volume;

use super::boundary::{
    ARCHIVE_MAGIC_PREFIX_LEN, archive_boundary_candidate_at, bytes_match_archive_magic, confirm_archive_boundary,
    confirms_on_disk, format_at,
};

/// The boundary confirm for a path on `parent`: the `(archive_path, inner_path)`
/// split when `path` crosses into a CONFIRMED archive (the archive file itself
/// has an empty inner path), else `None`.
///
/// Forks on `parent.supports_local_fs_access()`, ❌ never on whether the path
/// happens to open locally: a local parent takes [`confirm_archive_boundary`]'s
/// `std::fs` stat and sniff, and a remote one confirms through its own I/O
/// ([`confirm_remote_archive_boundary`]), so a direct-SMB parent never reads
/// through its possibly-hung OS mount. No archive-extension component means no
/// I/O at all.
pub async fn confirm_archive_boundary_on_parent(parent: &dyn Volume, path: &Path) -> Option<(PathBuf, PathBuf)> {
    let (archive_path, inner_path) = archive_boundary_candidate(path)?;
    let confirmed = if parent.supports_local_fs_access() {
        confirm_archive_boundary(path).is_some()
    } else {
        confirm_remote_archive_boundary(parent, &archive_path).await
    };
    confirmed.then_some((archive_path, inner_path))
}

/// Whether `archive_path` on a REMOTE `parent` is a real, browsable archive,
/// confirmed through the parent's own I/O: the component must be a FILE (a
/// directory named `foo.zip` loses to normal navigation) whose first
/// [`ARCHIVE_MAGIC_PREFIX_LEN`] bytes (one tar block, covering a plain tar's
/// ustar magic at offset 257) carry the format's magic.
///
/// **Refuse typed on an unavailable primitive.** When the parent can't do a
/// positioned read (`NotSupported`) or the sniff hits a transient fault, the file
/// can't be ruled out, so it routes anyway: the archive layer re-attempts the
/// read while parsing and surfaces a clean typed "unreadable archive" rather than
/// mis-listing the file as a plain one, and it starts browsing for real, with no
/// code change, once the read works. Only a SUCCESSFUL read whose bytes don't
/// match (a genuinely mislabeled file) declines.
pub async fn confirm_remote_archive_boundary(parent: &dyn Volume, archive_path: &Path) -> bool {
    confirm_remote_archive_boundary_at(parent, &AppPath::legacy_unchecked(archive_path)).await
}

/// [`confirm_archive_boundary_on_parent`] for an app path: the split when `path`
/// crosses into a CONFIRMED archive on `parent`, forked the same way.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "a typed twin the host switches to when its routing speaks app paths"
    )
)]
pub(crate) async fn confirm_archive_boundary_on_parent_at(
    parent: &dyn Volume,
    path: &AppPath,
) -> Option<(AppPathBuf, RelativeAppPathBuf)> {
    let (archive_path, inner_path) = archive_boundary_candidate_at(path)?;
    let confirmed = if parent.supports_local_fs_access() {
        confirms_on_disk(&archive_path)
    } else {
        confirm_remote_archive_boundary_at(parent, &archive_path).await
    };
    confirmed.then_some((archive_path, inner_path))
}

/// [`confirm_remote_archive_boundary`] for an app path.
pub(crate) async fn confirm_remote_archive_boundary_at(parent: &dyn Volume, archive_path: &AppPath) -> bool {
    match parent.get_metadata(archive_path).await {
        Ok(meta) if !meta.is_directory => {}
        // A real remote folder named `foo.zip`, a missing file, or a metadata
        // fault: don't route.
        _ => return false,
    }
    let Some(format) = format_at(archive_path) else {
        return false;
    };
    match parent.read_range(archive_path, 0, ARCHIVE_MAGIC_PREFIX_LEN).await {
        Ok(bytes) => bytes_match_archive_magic(format, &bytes),
        // Can't sniff: route, and let the archive layer refuse typed.
        Err(_) => true,
    }
}

#[cfg(test)]
#[path = "boundary_on_parent_test.rs"]
mod boundary_on_parent_test;
