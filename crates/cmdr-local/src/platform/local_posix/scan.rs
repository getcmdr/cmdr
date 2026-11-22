//! The POSIX half of the copy-scan family: a source's walk handed to the shared
//! `crate::copy_scan` (`subtree`), the source listing that leaves out a FAT folder's
//! sidecars (`list_directory_as_source_impl`), and the conflict probe
//! (`scan_for_conflicts_impl`), which the trait methods in `local_posix.rs` delegate to.
//! A trait impl can't span files, so the trait side stays in the parent and the work
//! lives here.

use super::LocalPosixVolume;
use cmdr_fs::app_path::{AppName, AppPath, shown_name};
use cmdr_fs::apple_double::SidecarRule;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::file_identity::device_of;
use cmdr_fs::log_error::LogError;
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::{ScanConflict, SourceItemInfo, VolumeError};
use std::ffi::OsStr;
use std::future::Future;
use std::pin::Pin;
use tokio::task::spawn_blocking;

impl LocalPosixVolume {
    /// One source's walk for the shared copy scan (`crate::copy_scan`): its OS path, the
    /// caller's spelling, and this volume's sidecar probe.
    pub(super) fn subtree(&self, path: &AppPath) -> crate::copy_scan::Subtree {
        crate::copy_scan::Subtree {
            os: self.os_path(path),
            as_called: path.as_std_legacy().into_owned(),
            spell_below: |as_called, below| as_called.join(below),
            sidecars: SidecarRule::with_probe(self.sidecar_probe),
        }
    }

    /// [`Volume::list_directory_as_source`]'s body: the listing, minus the
    /// AppleDouble sidecars a FAT or exFAT volume keeps beside each item
    /// (`cmdr_fs::apple_double`). A level with no `._` name never leaves the
    /// async task, so an ordinary folder pays nothing.
    pub(super) fn list_directory_as_source_impl<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        let abs_dir = self.os_path(path);
        let sidecars = SidecarRule::with_probe(self.sidecar_probe);
        Box::pin(async move {
            let entries = self.list_directory(path, None).await?;
            if !entries.iter().any(|entry| entry.name.starts_with("._")) {
                return Ok(entries);
            }
            // The probe and the sibling checks are syscalls, and on a mounted
            // share one can stall, so they run where the listing itself did.
            Ok(spawn_blocking(move || {
                // An unreadable folder can't vouch for its device: keep everything.
                let Ok(dir_meta) = std::fs::symlink_metadata(&abs_dir) else {
                    return entries;
                };
                entries
                    .into_iter()
                    .filter(|entry| !sidecars.is_sidecar(&abs_dir, device_of(&dir_meta), OsStr::new(&entry.name)))
                    .collect()
            })
            .await
            .expect("spawn_blocking sidecar filter doesn't panic and the task is uncancelable"))
        })
    }

    /// Stats each source's name under `dest_path` and reports the ones already
    /// taken, with both sides' size, mtime, and kind for the conflict dialog.
    pub(super) fn scan_for_conflicts_impl<'a>(
        &'a self,
        source_items: &'a [SourceItemInfo],
        dest_path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ScanConflict>, VolumeError>> + Send + 'a>> {
        let dest = self.listing_dir(dest_path);
        let source_items: Vec<SourceItemInfo> = source_items.to_vec();
        Box::pin(async move {
            spawn_blocking(move || {
                let mut conflicts = Vec::new();

                for item in &source_items {
                    // ❗ One plain name, the item refused typed otherwise: `Path::join` of `..`
                    // asked about the destination's PARENT, and of `/x` about `/x` itself.
                    // The refusal is the item's alone (it names no destination, so nothing
                    // collides with it): a conflict carries no per-item error, so it's
                    // logged, and the batch's other items are still checked. Read by its shown
                    // form, so only the name's shape decides: escape code points (a real
                    // file's literal plane-16 name) are one plain name, checked as before.
                    if let Err(refused) = AppName::from_app_spelling(&shown_name(&item.name)) {
                        let refused = VolumeError::from(refused);
                        log::warn!("scan_for_conflicts: skipping a source name: {}", LogError(&refused));
                        continue;
                    }
                    let Some((dest_file_path, row)) = dest.named_child(&item.name) else {
                        continue;
                    };
                    if dest_file_path.exists()
                        && let Ok(meta) = std::fs::metadata(&dest_file_path)
                    {
                        let dest_modified = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64));

                        conflicts.push(ScanConflict {
                            source_path: item.name.clone(),
                            dest_path: row.path,
                            source_size: item.size,
                            dest_size: meta.len(),
                            source_modified: item.modified,
                            dest_modified,
                            source_is_directory: item.is_directory,
                            dest_is_directory: meta.is_dir(),
                        });
                    }
                }

                Ok(conflicts)
            })
            .await
            .expect("spawn_blocking scan_for_conflicts closure doesn't panic and the task is uncancelable")
        })
    }
}
