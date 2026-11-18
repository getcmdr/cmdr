//! The user flags a copied FILE puts on only after its landing rename: Locked
//! (and append-only) refuse that rename on local disk, and SMB's Read-only
//! makes a staged temp unremovable, so they can't ride the write the way the
//! rest of a file's metadata does (`Volume::write_from_stream` asks the stream
//! and puts it on the temp). The contract: `DETAILS.md` § "Copies keep the
//! source's metadata".
//!
//! The engine snapshots the set off the source stream before the write
//! consumes it ([`after_landing`]), a few bytes read off what the open already
//! holds, and spends one `Volume::apply_metadata` on the landed name only when
//! the set isn't empty ([`put_on`]): a plain file costs nothing, and so does a
//! Hidden-only file onto a destination whose write keeps Hidden itself (SMB's
//! `staged_flags`).
//!
//! One landing: `stream_pipe_file` lands every staged file, a safe-replace's
//! swap over the original included, and puts the set on right after, at the
//! file's final name.

use cmdr_fs::app_path::AppPath;
use std::path::Path;
use std::sync::Arc;

use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::{CarriedMetadata, ClassOutcome, MetadataClasses};

use super::super::super::state::WriteOperationState;
use cmdr_fs::volume::{Volume, VolumeReadStream};

/// What the file `stream` reads waits to wear until it has its final name on
/// `dest`: empty unless `dest` keeps flags, `source` reports them, and the file
/// has one the write can't put on (`CarriedMetadata::after_landing`).
pub(super) fn after_landing(source: &dyn Volume, dest: &dyn Volume, stream: &dyn VolumeReadStream) -> CarriedMetadata {
    let caps = dest.metadata_caps();
    if !caps.stores.contains(MetadataClasses::FLAGS) || !source.metadata_caps().reports.contains(MetadataClasses::FLAGS)
    {
        return CarriedMetadata::default();
    }
    stream.metadata(MetadataClasses::FLAGS).after_landing(caps.staged_flags)
}

/// Puts `set` on the file that just landed at `landed`. Best effort: a refusal
/// is a `warn!` and a counted loss, never a failed copy.
pub(super) async fn put_on(dest: &Arc<dyn Volume>, landed: &Path, set: &CarriedMetadata, state: &WriteOperationState) {
    if set.is_empty() {
        return;
    }
    let report = dest.apply_metadata(&AppPath::legacy_unchecked(landed), set, None).await;
    if report.flags == ClassOutcome::Refused {
        log::warn!(
            target: "transfer",
            "landed flags: {} landed without the source's flags {:?}: {report:?}",
            LogPrivate::path(landed),
            set.flags
        );
        state.metadata_losses.note_file(MetadataClasses::FLAGS);
    }
}
