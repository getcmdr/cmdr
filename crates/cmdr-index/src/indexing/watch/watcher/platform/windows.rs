//! The `notify` backend, shared with the other OS that has no event journal.

pub(crate) use super::notify_backend::{SUPPORTS_EVENT_REPLAY, start, start_branches};

/// A `ReadDirectoryChangesW` start fails on a root that doesn't answer right now (the
/// vendored `notify` fails `watch` when the first read is refused): worth a re-arm.
pub(crate) const A_FAILED_START_IS_A_LOST_WATCH: bool = true;
