//! What a copy onto the export keeps beyond the bytes: `SETATTR` of the mtime,
//! the atime, and the mode after the last WRITE (and on a created folder).
//!
//! - ❗ **After the last WRITE and its COMMIT, before the landing rename.** A
//!   WRITE bumps the mtime, so a date set earlier is overwritten by the next
//!   one; and the engine's landing rename only touches the `ctime`, so the
//!   date set here is the one the user's file wears.
//! - The mode is folded the way every routed copy folds it
//!   (`cmdr_fs::metadata::routed_mode`): never wider than the user's umask
//!   allows, never setuid. The atime takes the mtime's value, the way SFTP's
//!   `ATTR_ACMODTIME` does: the source's own atime isn't carried.
//! - ❗ v3 has no creation date, so `CREATED` stays out of the caps, and xattrs
//!   are dropped with the copy's loss note saying so (no AppleDouble `._`
//!   sidecars). `crates/cmdr-nfs/DETAILS.md` § "Mutations".

use cmdr_fs::app_path::AppPath;
use std::time::{SystemTime, UNIX_EPOCH};

use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::{
    CarriedMetadata, ClassOutcome, MetadataCaps, MetadataClasses, MetadataReport, UserFlags, default_folder_mode,
    routed_mode,
};
use cmdr_fs::volume::VolumeError;
use cmdr_fs::volume::http_session::HttpBackend;
use cmdr_fs::volume::patching::patch_modified_at;
use log::warn;

use super::NfsVolume;
use super::reconnect::NfsClient;
use crate::errors::map_nfs_error;
use crate::proto::nfs3::{FileHandle, FileType, NfsTime, Sattr3, SetTime};

/// What this backend keeps: a file's and a folder's mode (both ride the
/// SETATTR the date already costs) and a folder's date. No creation date, no
/// user flags, no xattrs.
const METADATA_CAPS: MetadataCaps = MetadataCaps {
    stores: MetadataClasses::MODE,
    reports: MetadataClasses::MODE,
    folder_stores: MetadataClasses::MODE,
    folder_dates: true,
    staged_flags: UserFlags::NONE,
};

/// `at` as an NFS time, or `None` outside what its `u32` seconds can say
/// (before 1970, after 2106).
pub(super) fn nfs_time_of(at: SystemTime) -> Option<NfsTime> {
    let since = at.duration_since(UNIX_EPOCH).ok()?;
    Some(NfsTime {
        seconds: u32::try_from(since.as_secs()).ok()?,
        nseconds: since.subsec_nanos(),
    })
}

/// What one SETATTR put on, per class it was handed.
pub(super) struct AttrsSet {
    pub(super) mode: ClassOutcome,
    pub(super) modified: ClassOutcome,
}

impl NfsVolume {
    /// What this backend keeps of a copy's metadata.
    pub(super) fn metadata_caps_impl(&self) -> MetadataCaps {
        METADATA_CAPS
    }

    /// One SETATTR of `mode` and `modified` (as both mtime and atime) on
    /// `handle`, the object itself. ❗ Only on a handle this call just got by
    /// name or created, ❌ never a cached one. A refusal is logged and
    /// reported: the bytes are what matter.
    pub(super) async fn set_attrs(
        &self,
        client: &NfsClient,
        handle: &FileHandle,
        remote: &str,
        mode: Option<u32>,
        modified: Option<SystemTime>,
    ) -> AttrsSet {
        let time = modified.and_then(nfs_time_of);
        let unexpressible = modified.is_some() && time.is_none();
        let absent = |present: bool| {
            if present {
                ClassOutcome::Kept
            } else {
                ClassOutcome::Absent
            }
        };
        let mut outcome = AttrsSet {
            mode: absent(mode.is_some()),
            modified: if unexpressible {
                ClassOutcome::Refused
            } else {
                absent(time.is_some())
            },
        };
        if mode.is_none() && time.is_none() {
            return outcome;
        }
        let at = time.map_or(SetTime::DontChange, SetTime::ClientTime);
        let sattr = Sattr3 {
            mode,
            atime: at,
            mtime: at,
            ..Sattr3::default()
        };
        if let Err(err) = client.nfs.setattr(handle, &sattr, None).await {
            warn!(
                target: "nfs",
                "{} keeps the server's date and mode, not the source's: {}",
                LogPrivate::file_path(remote),
                LogError(&map_nfs_error(&err, remote))
            );
            if outcome.mode == ClassOutcome::Kept {
                outcome.mode = ClassOutcome::Refused;
            }
            if outcome.modified == ClassOutcome::Kept {
                outcome.modified = ClassOutcome::Refused;
            }
        }
        outcome
    }

    /// Puts `carried`'s mode and `modified` on `path` (a folder whose subtree
    /// landed, or a file after its landing): the handle by a fresh LOOKUP in
    /// its proven folder.
    pub(super) async fn apply_metadata_impl(
        &self,
        path: &AppPath,
        carried: &CarriedMetadata,
        modified: Option<SystemTime>,
    ) -> MetadataReport {
        let mut report = MetadataReport::none_applicable(carried, modified);
        let found = self.inner.noting(self.handle_for_metadata(path)).await;
        let (client, handle, remote, is_folder) = match found {
            Ok(found) => found,
            Err(err) => {
                warn!(
                    target: "nfs",
                    "couldn't reach {} to put its metadata on: {}",
                    LogPrivate::path(path.as_str()),
                    LogError(&err)
                );
                if carried.mode.is_some() {
                    report.mode = ClassOutcome::Refused;
                }
                if modified.is_some() {
                    report.modified = ClassOutcome::Refused;
                }
                return report;
            }
        };
        let default_mode = if is_folder {
            default_folder_mode()
        } else {
            cmdr_fs::metadata::default_file_mode()
        };
        let mode = carried.mode.and_then(|mode| routed_mode(mode, default_mode));
        let set = self.set_attrs(&client, &handle, &remote, mode, modified).await;
        if carried.mode.is_some() {
            report.mode = match set.mode {
                // A source mode the fold leaves nothing of carries nothing.
                ClassOutcome::Absent => ClassOutcome::NotApplicable,
                other => other,
            };
        }
        if modified.is_some() {
            report.modified = set.modified;
        }
        if set.modified == ClassOutcome::Kept || set.mode == ClassOutcome::Kept {
            patch_modified_at(self, path).await;
        }
        report
    }

    /// The client, a fresh handle for `path`, its remote spelling, and whether
    /// it's a folder.
    async fn handle_for_metadata(
        &self,
        path: &AppPath,
    ) -> Result<(std::sync::Arc<NfsClient>, FileHandle, String, bool), VolumeError> {
        let remote = self.to_remote_path(path)?;
        let client = self.client().await?;
        if remote == "/" {
            let attrs = client
                .nfs
                .getattr(&client.root)
                .await
                .map_err(|err| map_nfs_error(&err, &remote))?;
            let root = client.root.clone();
            return Ok((client, root, remote, attrs.file_type == FileType::Directory));
        }
        let (dir, name) = self.parent_for_mutation(&client, &remote).await?;
        let found = client
            .nfs
            .lookup(&dir.handle, name.as_bytes())
            .await
            .map_err(|err| map_nfs_error(&err, &remote))?;
        let is_folder = found
            .attributes
            .as_ref()
            .is_some_and(|attrs| attrs.file_type == FileType::Directory);
        Ok((client, found.handle, remote, is_folder))
    }
}
