//! Linux: nothing. The scheduler's fair share is the answer today; a per-thread
//! `setpriority(2)` would go here if it's ever wanted.

use super::QosClass;

pub(crate) fn apply(_class: QosClass) {}
