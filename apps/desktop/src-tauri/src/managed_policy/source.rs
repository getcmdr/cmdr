//! Where forced values come from: the trait each OS's source answers (`platform/`), the test-build
//! plist file, and the fake. Everything above sees `plist::Value`s keyed by name.

/// A source of FORCED preference values: what a configuration profile put in place, never the
/// user's own `defaults write`.
pub trait ManagedPrefsSource {
    /// The forced value under `key`, or `None` when nothing forces it.
    fn forced_value(&self, key: &str) -> Option<plist::Value>;

    /// Every key the source can name, for the "unknown key" debug line. Sources that can't list
    /// their managed layer (CFPreferences) return nothing, which only loses that log line.
    fn listed_keys(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Test builds only: a plist file standing in for the whole managed layer, named by
/// `CMDR_MANAGED_PREFS_FILE`. Every key in it counts as forced. ❌ Never compiled into a plain
/// release build: it would let anyone replace IT's policy.
#[cfg(any(debug_assertions, feature = "playwright-e2e", test))]
pub struct PlistFileSource {
    values: plist::Dictionary,
}

#[cfg(any(debug_assertions, feature = "playwright-e2e", test))]
impl PlistFileSource {
    /// Reads `path` now. A missing file is an empty policy, quietly: it's how an E2E run starts,
    /// unmanaged until a spec writes one. A malformed one reads as empty too, with a warning.
    pub fn read(path: &std::path::Path) -> Self {
        if !path.exists() {
            log::debug!(target: "managed_policy", "{} doesn't exist; reading no policy", cmdr_fs::log_private::LogPrivate::file_path(path));
            return Self {
                values: plist::Dictionary::new(),
            };
        }
        let values = match plist::Value::from_file(path) {
            Ok(plist::Value::Dictionary(values)) => values,
            Ok(_) => {
                log::warn!(target: "managed_policy", "{} isn't a plist dictionary; reading no policy", cmdr_fs::log_private::LogPrivate::file_path(path));
                plist::Dictionary::new()
            }
            Err(e) => {
                log::warn!(target: "managed_policy", "Couldn't read {}: {e}; reading no policy", cmdr_fs::log_private::LogPrivate::file_path(path));
                plist::Dictionary::new()
            }
        };
        Self { values }
    }
}

#[cfg(any(debug_assertions, feature = "playwright-e2e", test))]
impl ManagedPrefsSource for PlistFileSource {
    fn forced_value(&self, key: &str) -> Option<plist::Value> {
        self.values.get(key).cloned()
    }

    fn listed_keys(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct FakeSource(plist::Dictionary);

#[cfg(test)]
impl FakeSource {
    pub fn with(entries: &[(&str, plist::Value)]) -> Self {
        Self(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        )
    }
}

#[cfg(test)]
impl ManagedPrefsSource for FakeSource {
    fn forced_value(&self, key: &str) -> Option<plist::Value> {
        self.0.get(key).cloned()
    }

    fn listed_keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn a_plist_file_round_trips_into_forced_values() {
        let dir = TestDir::new("managed-prefs-file");
        let path = dir.join("policy.plist");
        let mut dict = plist::Dictionary::new();
        dict.insert("DisableUpdates".into(), plist::Value::Boolean(true));
        dict.insert("MaxUpdateVersion".into(), plist::Value::String("0.52".into()));
        plist::Value::Dictionary(dict)
            .to_file_xml(&path)
            .expect("write the test plist");

        let source = PlistFileSource::read(&path);
        assert_eq!(source.forced_value("DisableUpdates"), Some(plist::Value::Boolean(true)));
        assert_eq!(
            source.forced_value("MaxUpdateVersion"),
            Some(plist::Value::String("0.52".into()))
        );
        assert_eq!(source.forced_value("DisableAI"), None);
    }

    #[test]
    fn a_missing_or_malformed_file_reads_as_no_policy() {
        let dir = TestDir::new("managed-prefs-bad");
        let missing = PlistFileSource::read(&dir.join("nope.plist"));
        assert!(missing.listed_keys().is_empty());

        let garbage = dir.join("garbage.plist");
        std::fs::write(&garbage, "not a plist").expect("write");
        assert!(PlistFileSource::read(&garbage).listed_keys().is_empty());
    }
}
