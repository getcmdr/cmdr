//! Linux: no managed-preferences mechanism yet, so no restriction. A future source (for example
//! `/etc/cmdr/policy.json`) slots in here behind `ManagedPrefsSource`.

use crate::managed_policy::source::ManagedPrefsSource;

pub(crate) fn system_source() -> Box<dyn ManagedPrefsSource> {
    Box::new(NoManagedPrefs)
}

/// Nothing to watch.
pub(crate) fn start_refresh_triggers() {}

struct NoManagedPrefs;

impl ManagedPrefsSource for NoManagedPrefs {
    fn forced_value(&self, _key: &str) -> Option<plist::Value> {
        None
    }
}
