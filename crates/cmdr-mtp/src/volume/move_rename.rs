//! A rename that also changes folder, when the destination folder already holds
//! the item's OLD name: renamed where it is first, then moved, so the move never
//! meets a name it would replace. `DETAILS.md` § "A move that also renames".

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, ListedFolder, PathArithmetic};
use std::time::{SystemTime, UNIX_EPOCH};

use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{Volume, VolumeError};

use super::MtpVolume;
use super::mapping::map_mtp_error;
use crate::connection::storage_path::folder_or_root;

impl MtpVolume {
    /// Moves `from` (at `from_storage` on the device) into the folder `to_parent` (a
    /// storage path) as `to_name`,
    /// renaming before the move. Where the item starts may hold `to_name` too, so
    /// it then takes an interim name neither folder holds, and the last step
    /// gives it `to_name`.
    ///
    /// A move that fails after the first rename puts the old name back, so a
    /// refused move leaves the item exactly as it was.
    pub(super) async fn rename_then_move(
        &self,
        from: &AppPath,
        from_storage: &AppPath,
        from_name: &str,
        to_parent: &str,
        to_name: &str,
    ) -> Result<(), VolumeError> {
        let from_mtp = from_storage.as_str();
        let from_parent = folder_or_root(from_storage).as_str();
        let source_folder = cmdr_fs::twin_path::unpicked_app(from).parent().map(AppPath::to_owned);
        let new_name_taken_here = match source_folder.as_deref().and_then(|folder| sibling(folder, to_name)) {
            Some(taken) => self.exists(&taken).await,
            None => true,
        };
        let interim = if new_name_taken_here {
            interim_name(from_name)
        } else {
            to_name.to_string()
        };

        self.manager
            .rename_object(&self.device_id, self.storage_id, from_mtp, &interim)
            .await
            .map_err(map_mtp_error)?;
        let renamed = child_of(from_parent, &interim);
        if let Err(moving) = self
            .manager
            .move_object(&self.device_id, self.storage_id, &renamed, to_parent)
            .await
        {
            if let Err(back) = self
                .manager
                .rename_object(&self.device_id, self.storage_id, &renamed, from_name)
                .await
            {
                log::warn!(
                    "MTP rename_then_move: the move failed and the old name couldn't be put back, so {} stays as {}: {back:?}",
                    LogPrivate::path(from.as_str()),
                    LogPrivate::name(&interim),
                );
            }
            return Err(map_mtp_error(moving));
        }
        if interim != to_name {
            self.manager
                .rename_object(
                    &self.device_id,
                    self.storage_id,
                    &child_of(to_parent, &interim),
                    to_name,
                )
                .await
                .map_err(map_mtp_error)?;
        }
        Ok(())
    }
}

/// `name` (a name the storage path already split off, so one plain name, as the device
/// spells it) beside the other items in the app folder `folder`, joined as that folder
/// joins (`ListedFolder`) through the listed-name door, so a name holding an escape-range
/// code point names what the app holds for it (#1310). `None` only for a name a drive or
/// UNC folder can't take, which no MTP path is.
pub(super) fn sibling(folder: &AppPath, name: &str) -> Option<AppPathBuf> {
    ListedFolder::new(folder)
        .join(&AppName::from_listed_text(name).ok()?)
        .ok()
}

/// `name` in the device folder `folder`, as the session layer spells a path.
pub(super) fn child_of(folder: &str, name: &str) -> String {
    let separator = if folder.is_empty() || folder.ends_with('/') {
        ""
    } else {
        "/"
    };
    format!("{folder}{separator}{name}")
}

/// A name for the item while it crosses between two folders that each hold one
/// of its names. Not one of Cmdr's scratch names (`cmdr_fs::staging`): a sweep
/// deletes those, and this is the user's file. It keeps the old name legible, so
/// an item a crash leaves under it still says what it was.
fn interim_name(from_name: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let suffix = format!(".cmdr-moving-{stamp:x}");
    // Room for the suffix within PTP's 254 UTF-16 code units, trimmed on a
    // character boundary.
    let mut kept = String::new();
    for c in from_name.chars() {
        if kept.encode_utf16().count() + c.len_utf16() + suffix.len() > 200 {
            break;
        }
        kept.push(c);
    }
    format!("{kept}{suffix}")
}
