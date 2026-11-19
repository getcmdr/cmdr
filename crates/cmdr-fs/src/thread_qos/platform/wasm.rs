//! The browser build has no threads of its own to lower.

use super::QosClass;

pub(crate) fn apply(_class: QosClass) {}
