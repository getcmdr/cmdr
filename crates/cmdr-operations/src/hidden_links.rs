//! A listing's word on a link, on a backend whose listings can't be trusted to
//! mark one ([`BackendKind::listings_mark_links`]).
//!
//! A Samba share lists a Unix symlink as the folder it points to, with no
//! `is_symlink`, so a walk that merges or descends on the listing alone acts
//! through the link: on its target's files, a folder the user never picked
//! (#613). There, a listed folder is a folder only once `Volume::entry_kind`
//! says so, one round trip per folder. Everywhere else the listing is the
//! answer and these cost nothing.
//!
//! [`BackendKind::listings_mark_links`]: cmdr_fs::volume::BackendKind::listings_mark_links

use cmdr_fs::app_path::AppPath;
use std::path::Path;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{EntryKind, Volume, VolumeError};

/// Whether `volume`'s listing may call a link the plain folder `entry`, so
/// nothing that reads the listing alone (a pane's cached copy included) may
/// take it as a real folder.
pub(crate) fn listed_folder_may_be_a_link(volume: &dyn Volume, entry: &FileEntry) -> bool {
    entry.is_directory && !entry.is_symlink && !volume.backend_kind().listings_mark_links()
}

/// `volume.list_directory(dir)`, with `is_symlink` set on every entry that
/// `entry_kind` names a link where the listing may have called one a folder.
/// `is_directory` stays, as every backend's listing keeps it for a link to a
/// folder, so the entries read exactly as a link-marking backend lists them.
/// One `entry_kind` per listed folder there, none anywhere else.
pub(crate) async fn list_marking_hidden_links(volume: &dyn Volume, dir: &Path) -> Result<Vec<FileEntry>, VolumeError> {
    let mut entries = volume.list_directory(&on_volume(dir), None).await?;
    for entry in entries.iter_mut() {
        if listed_folder_may_be_a_link(volume, entry)
            && volume.entry_kind(&on_volume(Path::new(&entry.path))).await? == EntryKind::Symlink
        {
            entry.is_symlink = true;
        }
    }
    Ok(entries)
}

/// A folder or a listed row as the app path the volume takes: the merges hand their folders
/// over as `std::path` values, and a row's path is read back the same way, losslessly. The
/// one crossing here.
fn on_volume(path: &Path) -> std::borrow::Cow<'_, AppPath> {
    AppPath::legacy_unchecked(path)
}
