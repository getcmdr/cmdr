//! Fresh ZIP routing. The managed seedless creator owns creation; existing ZIP
//! mutation continues through the archive mutator's separate routes.

use cmdr_fs::app_path::AppPath;
use std::path::PathBuf;
use std::sync::Arc;

use super::super::OperationEventSink;
use super::super::look_alike::{LookAlike, look_alike_in, spelled_new_path};
use super::super::transfer::volume::{PathRole, map_volume_error};
use super::super::types::{ConflictResolution, ReadOnlySide, WriteOperationError, WriteOperationStartResult};
use cmdr_fs::os_junk_files::OsJunkFiles;
use cmdr_fs::volume::Volume;

/// Picks the concrete remote spelling that the overwrite warning referred to.
/// One equivalent spelling is the target; several are ambiguous and refused.
/// `target_app` is `target` as the volume is asked about it.
async fn archive_landing(
    parent: &dyn Volume,
    target: PathBuf,
    target_app: &AppPath,
) -> Result<(PathBuf, bool), WriteOperationError> {
    if parent.exists(target_app).await {
        return Ok((target, true));
    }
    let (Some(dir), Some(name)) = (target.parent(), target.file_name().and_then(|name| name.to_str())) else {
        return Ok((target, false));
    };
    match look_alike_in(parent, dir, name).await {
        Ok(LookAlike::None) => Ok((spelled_new_path(parent, &target), false)),
        Ok(LookAlike::One(entry)) => Ok((dir.join(entry.name), true)),
        Ok(LookAlike::Several) => Err(WriteOperationError::DestinationExists {
            path: target.display().to_string(),
        }),
        Err(error) => Err(map_volume_error(
            &target.display().to_string(),
            PathRole::Destination,
            error,
        )),
    }
}

impl crate::Operations {
    /// Starts one managed fresh compression that keeps OS junk files, for the
    /// tests. The app starts through [`Self::compress_start_leaving_out`].
    #[cfg(any(test, feature = "testing"))]
    #[allow(
        clippy::too_many_arguments,
        reason = "command seam carries both endpoints, operation settings, preview ownership, and provenance"
    )]
    pub(crate) async fn compress_start(
        &self,
        events: Arc<dyn OperationEventSink>,
        source_volume: Arc<dyn Volume>,
        source_paths: Vec<PathBuf>,
        dest_zip_full_path: PathBuf,
        parent_volume_id: String,
        conflict: ConflictResolution,
        progress_interval_ms: u64,
        compression_level: Option<i64>,
        preview_id: Option<String>,
        initiator: cmdr_operation_log::types::Initiator,
    ) -> Result<WriteOperationStartResult, WriteOperationError> {
        self.compress_start_leaving_out(
            events,
            source_volume,
            source_paths,
            dest_zip_full_path,
            parent_volume_id,
            conflict,
            progress_interval_ms,
            compression_level,
            preview_id,
            initiator,
            OsJunkFiles::Include,
        )
        .await
    }

    /// Starts one managed fresh compression. Nothing at the destination changes
    /// until the producer has closed, byte counts agree, and the staged ZIP parses.
    ///
    /// `os_junk` says whether to leave out OS junk files below the
    /// selection; on a Mac, with them every `__MACOSX` sidecar the zip would
    /// otherwise carry, which a person on another system sees as the same clutter
    /// (#314, #1305, `OsJunkFiles::leaves_out_apple_double_sidecars`).
    #[allow(
        clippy::too_many_arguments,
        reason = "command seam carries both endpoints, operation settings, preview ownership, and provenance"
    )]
    pub(crate) async fn compress_start_leaving_out(
        &self,
        events: Arc<dyn OperationEventSink>,
        source_volume: Arc<dyn Volume>,
        source_paths: Vec<PathBuf>,
        dest_zip_full_path: PathBuf,
        parent_volume_id: String,
        conflict: ConflictResolution,
        progress_interval_ms: u64,
        compression_level: Option<i64>,
        preview_id: Option<String>,
        initiator: cmdr_operation_log::types::Initiator,
        os_junk: OsJunkFiles,
    ) -> Result<WriteOperationStartResult, WriteOperationError> {
        super::routing::ensure_zip_writable(&dest_zip_full_path, ReadOnlySide::Destination)?;
        // The `.zip` as its parent drive is asked about it.
        let dest_app = AppPath::legacy_unchecked(&dest_zip_full_path).into_owned();
        // Plain `get`: the `.zip` file itself is a plain file on its parent drive.
        let (dest_zip_full_path, existed) = match self.host().volumes().get(&parent_volume_id) {
            Some(parent) if !parent.supports_local_fs_access() => {
                archive_landing(parent.as_ref(), dest_zip_full_path, &dest_app).await?
            }
            Some(parent) => {
                let existed = parent.exists(&dest_app).await;
                (dest_zip_full_path, existed)
            }
            None => {
                let probe_path = dest_zip_full_path.clone();
                let existed = self
                    .host()
                    .runtime()
                    .spawn_blocking(move || std::fs::symlink_metadata(probe_path).is_ok())
                    .await
                    .map_err(|error| WriteOperationError::IoError {
                        path: dest_zip_full_path.display().to_string(),
                        message: error.to_string().into(),
                    })?;
                (dest_zip_full_path, existed)
            }
        };
        super::fresh_compress::start(
            self,
            events,
            source_volume,
            source_paths,
            dest_zip_full_path,
            parent_volume_id,
            conflict,
            progress_interval_ms,
            compression_level,
            preview_id,
            !existed,
            initiator,
            os_junk,
        )
        .await
    }
}
