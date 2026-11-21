//! The volume's root, in the two spellings a manager hands on: the TEXT the index stores
//! and compares, and the NATIVE path the local pipeline opens. Both cross from the one
//! app path the start named, through `paths::app_text`.

use std::path::Path;

use super::IndexManager;
use crate::indexing::paths::app_text;

impl IndexManager {
    /// The root as the index stores and compares it: claims, announced walks, and the
    /// `volume_path` meta (`app_text::stored_text`).
    pub(super) fn root_text(&self) -> String {
        app_text::stored_text(&self.volume_root).into_owned()
    }

    /// The root's native path, for a run of the local pipeline (the guarded walker, the
    /// watcher, the reconcile walk). A root no OS opens never gets here: only a
    /// local-scanner kind runs that pipeline, and its root is a mount point.
    pub(super) fn local_root(&self) -> Result<&Path, String> {
        self.native_root
            .as_deref()
            .ok_or_else(|| format!("'{}' has a root no local walk can open", self.volume_id))
    }
}
