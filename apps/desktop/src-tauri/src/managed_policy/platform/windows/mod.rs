//! Windows: Group Policy's registry keys, `HKLM\SOFTWARE\Policies\Rymd\Cmdr` (computer) and
//! `HKCU\SOFTWARE\Policies\Rymd\Cmdr` (user), the machine winning per name. Only administrators
//! (and Group Policy) can write under `Policies`, so everything there counts as forced, the way a
//! macOS profile's values do. What each registry type reads as: `../registry_values.rs`.

mod watch;

use super::registry_values::{Hive, POLICY_KEY, RawEntry, RegistryPolicy};
use crate::managed_policy::source::ManagedPrefsSource;
use crate::platform::registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, Key};
use windows_sys::Win32::System::Registry::HKEY;

pub(crate) fn system_source() -> Box<dyn ManagedPrefsSource> {
    Box::new(read_policy(
        HKEY_LOCAL_MACHINE,
        POLICY_KEY,
        HKEY_CURRENT_USER,
        POLICY_KEY,
    ))
}

pub(crate) fn start_refresh_triggers() {
    watch::start();
}

/// The merged policy from the two keys. A hive whose key is missing adds nothing; one we can't
/// read at all restricts everything it could have set, which we can't know, so it's logged and
/// read as empty (a broken user hive can't be allowed to hide the machine's policy either).
fn read_policy(machine_root: HKEY, machine_path: &str, user_root: HKEY, user_path: &str) -> RegistryPolicy {
    RegistryPolicy::merge(read_hive(machine_root, machine_path), read_hive(user_root, user_path))
}

fn read_hive(root: HKEY, path: &str) -> Hive {
    match try_read_hive(root, path) {
        Ok(hive) => hive,
        Err(code) => {
            // allowed-log-private: `path` is our own fixed registry key, not a user value
            log::warn!(target: "managed_policy", "Couldn't read the policy key {path}: Win32 error {code}; reading it as empty");
            Hive::default()
        }
    }
}

fn try_read_hive(root: HKEY, path: &str) -> Result<Hive, u32> {
    let Some(key) = Key::open(root, path)? else {
        return Ok(Hive::default());
    };
    let values = raw_entries(&key)?;
    let mut lists = Vec::new();
    for name in key.subkey_names()? {
        if let Some(child) = Key::open(root, &format!("{path}\\{name}"))? {
            lists.push((name, raw_entries(&child)?));
        }
    }
    Ok(Hive { values, lists })
}

fn raw_entries(key: &Key) -> Result<Vec<RawEntry>, u32> {
    Ok(key
        .values()?
        .into_iter()
        .map(|(name, value)| (name, value.kind, value.data))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_policy::keys::{self, ALLOWED_CLOUD_AI_HOSTS, DISABLE_UPDATES, DISABLE_USAGE_STATS};
    use crate::platform::registry::ScratchKey;

    /// Two scratch keys stand in for the machine and user hives (both under `HKCU\Software\CmdrTest`,
    /// never `Policies`), read through the real registry calls.
    #[test]
    fn policy_reads_from_the_registry_with_the_machine_winning() {
        let machine = ScratchKey::new("policy-machine");
        let user = ScratchKey::new("policy-user");
        machine.set_dword("", "DisableUpdates", 1);
        user.set_dword("", "disableupdates", 0);
        user.set_string("", "DisableUsageStats", "yes");
        machine.set_string(ALLOWED_CLOUD_AI_HOSTS, "1", "api.openai.com");
        machine.set_string(ALLOWED_CLOUD_AI_HOSTS, "2", "*.openai.azure.com");

        let policy = read_policy(HKEY_CURRENT_USER, machine.path(), HKEY_CURRENT_USER, user.path());
        assert_eq!(
            policy.forced_value(DISABLE_UPDATES),
            Some(plist::Value::Integer(1.into()))
        );
        assert_eq!(
            policy.forced_value(DISABLE_USAGE_STATS),
            Some(plist::Value::String("yes".into()))
        );
        assert_eq!(
            policy.forced_value(ALLOWED_CLOUD_AI_HOSTS),
            Some(plist::Value::Array(vec![
                plist::Value::String("api.openai.com".into()),
                plist::Value::String("*.openai.azure.com".into()),
            ]))
        );
        let parsed = keys::parse(&policy);
        assert!(parsed.policy.is_managed());
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn missing_keys_read_as_no_policy() {
        let policy = read_policy(
            HKEY_CURRENT_USER,
            "Software\\CmdrTest\\no-such-machine-key",
            HKEY_CURRENT_USER,
            "Software\\CmdrTest\\no-such-user-key",
        );
        assert!(!keys::parse(&policy).policy.is_managed());
    }

    /// The real keys, read-only: on the builder nothing manages Cmdr, so this proves the read
    /// path opens `SOFTWARE\Policies` without error and finds no policy there.
    #[test]
    fn the_real_policy_keys_open_and_read_cleanly() {
        assert!(try_read_hive(HKEY_LOCAL_MACHINE, POLICY_KEY).is_ok());
        assert!(try_read_hive(HKEY_CURRENT_USER, POLICY_KEY).is_ok());
    }
}
