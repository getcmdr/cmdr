//! macOS: the FORCED layer of the app's preferences domain (what a configuration profile put in
//! place), re-read on activation and on a watch of the folder profiles write to.

mod watch;

use crate::managed_policy::source::ManagedPrefsSource;

pub(crate) fn system_source() -> Box<dyn ManagedPrefsSource> {
    Box::new(CfPrefsSource::synchronized(crate::config::BUNDLE_ID))
}

pub(crate) fn start_refresh_triggers() {
    watch::start();
}

/// A value that CFPreferences reports as forced but that won't convert to a `plist::Value`. It's
/// the wrong type for every key, so each key reads it as its most restrictive value (rule 4: a
/// value we can't read never turns a restriction off).
fn unreadable_forced_value() -> plist::Value {
    plist::Value::Data(Vec::new())
}

/// The real thing on macOS: the app's domain as `cfprefsd` merges it, filtered to forced keys.
pub struct CfPrefsSource {
    domain: objc2_core_foundation::CFRetained<objc2_core_foundation::CFString>,
}

impl CfPrefsSource {
    /// Opens `domain` for one read pass. ❗ Synchronizes first, or a profile installed while Cmdr
    /// runs stays invisible to this process (the same gotcha as
    /// `system_look/platform/macos/glass_tint.rs`).
    pub fn synchronized(domain: &str) -> Self {
        use objc2_core_foundation::{CFPreferencesAppSynchronize, CFString};

        let domain = CFString::from_str(domain);
        if !CFPreferencesAppSynchronize(&domain) {
            log::debug!(target: "managed_policy", "CFPreferencesAppSynchronize returned false; reading the cached domain");
        }
        Self { domain }
    }
}

impl ManagedPrefsSource for CfPrefsSource {
    fn forced_value(&self, key: &str) -> Option<plist::Value> {
        use objc2_core_foundation::{CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue, CFString};

        let key = CFString::from_str(key);
        // ❗ `IsForced` FIRST: `CopyAppValue` merges the user's own layer too, and a user's
        // `defaults write` must never read as policy.
        if !CFPreferencesAppValueIsForced(&key, &self.domain) {
            return None;
        }
        let value = CFPreferencesCopyAppValue(&key, &self.domain)?;
        Some(crate::cf_plist::to_plist(&value).unwrap_or_else(unreadable_forced_value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cf_plist::{ScratchDomain, from_plist};
    use crate::managed_policy::keys::{self, DISABLE_USAGE_STATS};

    #[test]
    fn a_value_in_the_users_own_layer_is_not_policy() {
        let scratch = ScratchDomain::new("com.getcmdr.policytest.userlayer");
        let value = from_plist(&plist::Value::Boolean(true)).expect("encode");
        scratch.set(DISABLE_USAGE_STATS, Some(&value));

        // The user layer really holds it: `CopyAppValue` (which merges every layer) sees it.
        let key = objc2_core_foundation::CFString::from_str(DISABLE_USAGE_STATS);
        let domain = objc2_core_foundation::CFString::from_str(scratch.domain());
        assert!(
            objc2_core_foundation::CFPreferencesCopyAppValue(&key, &domain).is_some(),
            "the scratch write must be visible, or this test proves nothing"
        );

        let source = CfPrefsSource::synchronized(scratch.domain());
        assert_eq!(source.forced_value(DISABLE_USAGE_STATS), None);
        assert!(!keys::parse(&source).policy.is_managed());
    }

    #[test]
    fn a_key_nobody_wrote_reads_as_unforced() {
        let scratch = ScratchDomain::new("com.getcmdr.policytest.empty");
        let source = CfPrefsSource::synchronized(scratch.domain());
        assert_eq!(source.forced_value(DISABLE_USAGE_STATS), None);
    }
}
