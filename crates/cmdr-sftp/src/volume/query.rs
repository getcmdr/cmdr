//! Reading what's on the server without changing it.
use cmdr_fs::app_path::{AppPath, AppPathBuf};
use cmdr_fs::volume::Presence;
use cmdr_fs::volume::dot_dot::{LinkFuture, LinkReader, parent_across_links};
use std::path::PathBuf;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{EntryKind, ListingProgress, SpaceInfo, VolumeError};
use openssh_sftp_client::fs::DirEntry;
use tokio_util::sync::CancellationToken;

use super::SftpVolume;
use super::paths::{join_remote, remote_file_name};
use cmdr_fs::volume::remote_paths::RemoteRoot;

use super::mapping::{metadata_to_file_entry, statvfs_to_space_info};
use crate::errors::map_sftp_error;

/// The server's links, for a `..` ([`SftpVolume::parent_for_dot_dot_impl`]). A link
/// read off this volume's root answers `NotFound` (no link this volume can see), and an
/// absolute target off it can't be spelled, so it's refused.
impl LinkReader for SftpVolume {
    fn read_link<'a>(&'a self, path: &'a AppPath) -> LinkFuture<'a> {
        Box::pin(self.read_link_target_impl(path))
    }

    fn absolute_target(&self, target: &str) -> Option<AppPathBuf> {
        self.root.to_app(target).ok()
    }
}

// ⚠️ **A filename that isn't UTF-8 costs the SESSION, not just the listing.**
// SFTP v3 filenames are BYTES with no declared encoding, and a Linux server may
// hand back any sequence. `openssh-sftp-client` deserializes names through a
// strict `ssh_format`, and it does so INSIDE its own read task, which then
// exits — so every later request on that session answers
// `BackgroundTaskFailure`, which `map_sftp_error` reports as a lost connection.
//
// Still the right failure to have: the alternative crate substitutes U+FFFD, and
// a name that addresses nothing gets written at a copy's destination. The fix,
// when it's worth it, is the byte-backed `NameEntry::filename` the vendoring
// escape hatch buys (`openssh-sftp-client` 0.15.7, measured against
// `sftp-fixture-oddnames`, 2026-08-22).

impl SftpVolume {
    /// One `readdir` walk, in the batches the server sends them.
    ///
    /// ❗ Cancellation is checked between BATCHES rather than per entry: a
    /// directory over a 50 ms link is many round trips, and the token is what
    /// lets a pane the user navigated away from stop paying for them.
    pub(super) async fn list_directory_impl(
        &self,
        path: &AppPath,
        on_progress: Option<&(dyn Fn(ListingProgress) + Sync)>,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<FileEntry>, VolumeError> {
        use futures_util::StreamExt;

        let remote = self.to_remote_path(path)?;
        let session = self.clone_session().await?;
        let dir = session
            .sftp()
            .fs()
            .open_dir(&remote)
            .await
            .map_err(|e| map_sftp_error(&e, &remote))?;

        let mut entries = Vec::new();
        let mut tally = ListingProgress::default();
        // `ReadDir` holds a cancellation future, so it isn't `Unpin`.
        let mut stream = std::pin::pin!(dir.read_dir());
        while let Some(next) = stream.next().await {
            if cancel.is_some_and(CancellationToken::is_cancelled) {
                return Err(VolumeError::Cancelled(remote.into()));
            }
            // ⚠️ A non-UTF-8 filename fails the WHOLE readdir here, by design in
            // `openssh-sftp-client`: it deserializes names through `ssh_format`,
            // which is strict. Loud and lossless beats the alternative, where a
            // folder copy writes files under names that address nothing.
            let entry = next.map_err(|e| map_sftp_error(&e, &remote))?;
            let Some(built) = file_entry(&self.root, &entry, &remote) else {
                continue;
            };
            if built.is_directory {
                tally.dirs += 1;
            } else {
                tally.files += 1;
                tally.bytes += built.size.unwrap_or(0);
            }
            entries.push(built);
        }

        // One report for the whole listing, ❌ never one per entry: the seam is a
        // trait object and a quarter-million-entry directory would pay for every
        // one of them.
        if let Some(on_progress) = on_progress {
            on_progress(tally);
        }
        Ok(entries)
    }

    /// One `stat` round trip, as a `FileEntry`.
    pub(super) async fn get_metadata_impl(&self, path: &AppPath) -> Result<FileEntry, VolumeError> {
        let remote = self.to_remote_path(path)?;
        let session = self.clone_session().await?;
        let meta = session
            .sftp()
            .fs()
            .metadata(&remote)
            .await
            .map_err(|e| map_sftp_error(&e, &remote))?;
        let name = remote_file_name(&remote).map_or_else(|| self.name.clone(), str::to_owned);
        let app_path = self
            .app_text_of(&remote)
            .ok_or_else(|| VolumeError::NotFound(path.to_string()))?;
        Ok(metadata_to_file_entry(&name, &app_path, &meta))
    }

    /// What is AT `path`, with a link reported as the link: one `lstat`.
    ///
    /// `get_metadata_impl` asks `stat`, which follows a link, so the trait
    /// default would report a link to a folder as the folder.
    pub(super) async fn entry_kind_impl(&self, path: &AppPath) -> Result<EntryKind, VolumeError> {
        let remote = self.to_remote_path(path)?;
        let session = self.clone_session().await?;
        let meta = session
            .sftp()
            .fs()
            .symlink_metadata(&remote)
            .await
            .map_err(|e| map_sftp_error(&e, &remote))?;
        Ok(match meta.file_type() {
            Some(t) if t.is_symlink() => EntryKind::Symlink,
            Some(t) if t.is_dir() => EntryKind::Directory,
            _ => EntryKind::File,
        })
    }

    /// The target of the link at `path`, as the server stores it. `lstat` first:
    /// `SSH_FXP_READLINK` on anything else answers a generic failure, which
    /// reads the same as a real one.
    pub(super) async fn read_link_target_impl(&self, path: &AppPath) -> Result<Option<PathBuf>, VolumeError> {
        if self.entry_kind_impl(path).await? != EntryKind::Symlink {
            return Ok(None);
        }
        let remote = self.to_remote_path(path)?;
        let session = self.clone_session().await?;
        let target = session
            .sftp()
            .fs()
            .read_link(&remote)
            .await
            .map_err(|e| map_sftp_error(&e, &remote))?;
        Ok(Some(target))
    }

    /// `<path>/..` as the server reads it: the link target's parent when the last
    /// name is a symlink (`cmdr_fs::volume::dot_dot`), one `LSTAT` per name asked
    /// and one `READLINK` per link followed. A target off this volume's root is
    /// refused, ❌ never anchored.
    pub(super) async fn parent_for_dot_dot_impl(&self, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
        parent_across_links(self, path).await
    }

    /// The free and total space of the filesystem holding `path`: one
    /// `statvfs@openssh.com` round trip.
    ///
    /// A server without the extension answers `NotSupported` without touching
    /// the wire, which callers read as "can't tell", ❌ never "no room".
    /// Asked per path because one server can hold several filesystems, and a
    /// copy is judged against the one it lands on.
    pub(super) async fn space_info_impl(&self, path: &AppPath) -> Result<SpaceInfo, VolumeError> {
        let remote = self.to_remote_path(path)?;
        let session = self.clone_session().await?;
        if !session.extensions().statvfs {
            return Err(VolumeError::NotSupported);
        }
        let stat = session
            .sftp()
            .fs()
            .statvfs(&remote)
            .await
            .map_err(|e| map_sftp_error(&e, &remote))?;
        Ok(statvfs_to_space_info(&stat))
    }

    /// Whether `path` is there. A path off this volume is `Absent` (nothing is
    /// there ON THIS VOLUME, the question asked); a refusal is `Unknown`.
    pub(super) async fn presence_impl(&self, path: &AppPath) -> Presence {
        Presence::of(&self.get_metadata_impl(path).await)
    }
}

/// One directory entry as a `FileEntry`, or `None` for the two the protocol
/// includes and a pane never shows, and for a name that would land off the root.
fn file_entry(root: &RemoteRoot, entry: &DirEntry, parent: &str) -> Option<FileEntry> {
    let name = entry.filename().to_string_lossy().into_owned();
    if name == "." || name == ".." {
        return None;
    }
    let remote_path = join_remote(parent, &name);
    let app_path = root.to_app_path(&remote_path)?;
    Some(metadata_to_file_entry(
        &name,
        &app_path.to_string_lossy(),
        &entry.metadata(),
    ))
}
