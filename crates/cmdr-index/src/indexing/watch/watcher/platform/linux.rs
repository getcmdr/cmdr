//! The `notify` backend, shared with the other OS that has no event journal.

pub(crate) use super::notify_backend::{SUPPORTS_EVENT_REPLAY, start, start_branches};

/// An inotify start fails on the watch budget, which a retry won't refill.
pub(crate) const A_FAILED_START_IS_A_LOST_WATCH: bool = false;
