//! How a failed transfer starts again: the queue window's Retry on a "Couldn't
//! finish" row.
//!
//! A background job has no dialog holding its birth context the way the
//! foreground error dialog does (`dialog-state.svelte.ts::retryPropsFrom`), and
//! the queue window is a separate webview that never saw how the job began. So
//! the BACKEND keeps the recipe: the start's own arguments, captured at the one
//! routing entry every cross-volume copy and move goes through
//! (`super::start_volume_copy` / `super::start_volume_move`), held with the
//! operation by the manager (as a `manager::StartAgain`, so the manager never
//! reaches back into the routing it would re-enter), and carried onto its
//! retained failure.
//!
//! **A retry is a NEW operation**, with the same sources, destination, and
//! conflict policy, and four things reset, the same four the foreground's
//! `retryPropsFrom` resets (❗ keep the two in step):
//!
//! - **No preview**: the backend refuses a second claim on one preview, and the
//!   retry runs its own scan.
//! - **No pre-known conflicts**: the failed run may have landed files, so the
//!   old pre-flight's list is stale; the backend finds conflicts itself.
//! - **The initiator is the user**: the click is theirs, whoever started the
//!   original (an agent, a drag).
//! - **No source binding**: the binding is a promise to whoever started the
//!   original (`source_binding.rs`), and the person clicking Retry is looking at
//!   the row, not at that promise.
//!
//! Everything else a start does, the retry does too, because it IS a start: it
//! queues behind a busy lane like any operation, and one that waited checks its
//! sources when admitted and asks "move the rest / cancel" about any that left
//! meanwhile (`../queued_sources.rs`). One admitted at once is a fresh start the
//! person just asked for, so a source gone by then fails it with its own typed
//! error, exactly as a new copy of that file would.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::super::Operations;
use super::super::event_sinks::OperationEventSink;
use super::super::manager::StartAgain;
use super::super::types::{VolumeCopyConfig, WriteOperationError, WriteOperationStartResult};
use cmdr_fs::app_path::AppPathBuf;
use cmdr_operation_log::types::Initiator;

/// Which start a retry runs again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetryStart {
    Copy,
    Move,
}

/// Everything a cross-volume copy or move needs to start again. See the module
/// docs for what carries over and what resets.
#[derive(Clone)]
pub(super) struct RetryRecipe {
    pub start: RetryStart,
    pub events: Arc<dyn OperationEventSink>,
    pub source_volume_id: String,
    pub source_paths: Vec<AppPathBuf>,
    pub dest_volume_id: String,
    pub dest_path: String,
    pub config: VolumeCopyConfig,
}

impl std::fmt::Debug for RetryRecipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetryRecipe")
            .field("start", &self.start)
            .field("source_volume_id", &self.source_volume_id)
            .field("sources", &self.source_paths.len())
            .field("dest_volume_id", &self.dest_volume_id)
            .finish_non_exhaustive()
    }
}

impl RetryRecipe {
    /// The config the retry starts with: the original's, minus what a NEW
    /// operation can't inherit (module docs).
    fn retry_config(&self) -> VolumeCopyConfig {
        VolumeCopyConfig {
            preview_id: None,
            pre_known_conflicts: Vec::new(),
            ..self.config.clone()
        }
    }

    /// Starts the transfer again as a new operation, through the same routing
    /// the original took.
    async fn start_routed(&self, engine: &Operations) -> Result<WriteOperationStartResult, WriteOperationError> {
        let events = Arc::clone(&self.events);
        let config = self.retry_config();
        match self.start {
            RetryStart::Copy => {
                engine
                    .start_volume_copy(
                        events,
                        self.source_volume_id.clone(),
                        self.source_paths.clone(),
                        self.dest_volume_id.clone(),
                        self.dest_path.clone(),
                        config,
                        Initiator::User,
                        None,
                    )
                    .await
            }
            RetryStart::Move => {
                engine
                    .start_volume_move(
                        events,
                        self.source_volume_id.clone(),
                        self.source_paths.clone(),
                        self.dest_volume_id.clone(),
                        self.dest_path.clone(),
                        config,
                        Initiator::User,
                        None,
                    )
                    .await
            }
        }
    }
}

/// Hands `recipe` to `engine`'s manager as `operation_id`'s way to start again.
pub(super) fn attach(engine: &Operations, operation_id: &str, recipe: RetryRecipe) {
    engine.manager().attach_retry(operation_id, Arc::new(recipe));
}

impl StartAgain for RetryRecipe {
    fn start_again<'a>(
        &'a self,
        engine: &'a Operations,
    ) -> Pin<Box<dyn Future<Output = Result<WriteOperationStartResult, WriteOperationError>> + Send + 'a>> {
        Box::pin(self.start_routed(engine))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cmdr_fs::app_path;

    use super::*;
    use crate::event_sinks::CollectorEventSink;

    /// The reset rule, pinned: a retry never claims the old preview (the
    /// backend refuses a second claim) or trusts the old conflict list (the
    /// failed run may have landed files), and keeps everything else.
    #[test]
    fn a_retry_starts_fresh_but_keeps_what_the_person_chose() {
        let original = VolumeCopyConfig {
            preview_id: Some("preview-1".into()),
            pre_known_conflicts: vec!["a.txt".into()],
            leaves: crate::types::TransferLeaves::single("renamed.txt"),
            conflict_resolution: crate::types::ConflictResolution::Skip,
            ..VolumeCopyConfig::default()
        };
        let recipe = RetryRecipe {
            start: RetryStart::Copy,
            events: Arc::new(CollectorEventSink::new()),
            source_volume_id: "root".into(),
            source_paths: vec![app_path!("/a.txt").to_owned()],
            dest_volume_id: "smb-nas".into(),
            dest_path: "/share".into(),
            config: original,
        };

        let retry = recipe.retry_config();

        assert_eq!(retry.preview_id, None);
        assert!(retry.pre_known_conflicts.is_empty());
        assert_eq!(retry.leaves, crate::types::TransferLeaves::single("renamed.txt"));
        assert_eq!(retry.conflict_resolution, crate::types::ConflictResolution::Skip);
    }

    /// A retry names each item exactly as the first run did.
    #[test]
    fn a_retry_keeps_the_name_of_each_item() {
        let leaves = crate::types::TransferLeaves::per_source([
            (PathBuf::from("/a.pdf"), "alpha.pdf".to_string()),
            (PathBuf::from("/b.pdf"), "beta.pdf".to_string()),
        ]);
        let recipe = RetryRecipe {
            start: RetryStart::Move,
            events: Arc::new(CollectorEventSink::new()),
            source_volume_id: "root".into(),
            source_paths: vec![
                app_path!("/a.pdf").to_owned(),
                app_path!("/b.pdf").to_owned(),
                app_path!("/c.pdf").to_owned(),
            ],
            dest_volume_id: "root".into(),
            dest_path: "/filed".into(),
            config: VolumeCopyConfig {
                leaves: leaves.clone(),
                ..VolumeCopyConfig::default()
            },
        };

        assert_eq!(recipe.retry_config().leaves, leaves);
    }
}
