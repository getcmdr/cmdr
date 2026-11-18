//! Windows policy, as pure data: registry values decoded into the `plist::Value`s `keys.rs` parses,
//! and the machine and user hives merged with the machine winning. The Windows arm reads the bytes;
//! everything that decides lives here, so it's tested on every OS.
//!
//! The mapping (the policy key catalog in `DETAILS.md` § Key catalog is the same on every OS):
//!
//! - `REG_DWORD` / `REG_QWORD` (and big-endian DWORD) → an integer, which a bool key reads as
//!   `0` = false, anything else true, and `MaxUpdateVersion` as a major version.
//! - `REG_SZ` / `REG_EXPAND_SZ` → a string (not expanded: no policy value names a path).
//! - `REG_MULTI_SZ`, or a subkey of string values (what an ADMX `<list>` element writes, named
//!   `1`, `2`, …) → an array of strings, for `AllowedCloudAIHosts`.
//! - Anything else (`REG_BINARY`, `REG_NONE`, a malformed length or UTF-16) → data, the wrong type
//!   for every key, so the key takes its most restrictive reading (rule 4).

use std::collections::HashMap;

use crate::managed_policy::source::ManagedPrefsSource;

/// The policy key under `HKLM` and `HKCU`. `Rymd` is the vendor level (Rymdskottkärra AB), so a
/// second product sits beside `Cmdr`. ❗ A public contract: admins' GPOs and the ADMX template
/// name it, so it never moves (`DETAILS.md` § The Windows registry keys).
pub(crate) const POLICY_KEY: &str = "SOFTWARE\\Policies\\Rymd\\Cmdr";

pub(crate) const REG_SZ: u32 = 1;
pub(crate) const REG_EXPAND_SZ: u32 = 2;
pub(crate) const REG_BINARY: u32 = 3;
pub(crate) const REG_DWORD: u32 = 4;
pub(crate) const REG_DWORD_BIG_ENDIAN: u32 = 5;
pub(crate) const REG_MULTI_SZ: u32 = 7;
pub(crate) const REG_QWORD: u32 = 11;

/// One value as the registry hands it over: its name, its type, and its bytes.
pub(crate) type RawEntry = (String, u32, Vec<u8>);

/// One hive's policy key: its own values, plus each subkey's values (list policies).
#[derive(Debug, Default)]
pub(crate) struct Hive {
    pub(crate) values: Vec<RawEntry>,
    pub(crate) lists: Vec<(String, Vec<RawEntry>)>,
}

/// One registry value as a policy value.
pub(crate) fn decode(kind: u32, data: &[u8]) -> plist::Value {
    let unreadable = || plist::Value::Data(data.to_vec());
    match kind {
        REG_DWORD => match <[u8; 4]>::try_from(data) {
            Ok(bytes) => plist::Value::Integer(u32::from_le_bytes(bytes).into()),
            Err(_) => unreadable(),
        },
        REG_DWORD_BIG_ENDIAN => match <[u8; 4]>::try_from(data) {
            Ok(bytes) => plist::Value::Integer(u32::from_be_bytes(bytes).into()),
            Err(_) => unreadable(),
        },
        REG_QWORD => match <[u8; 8]>::try_from(data) {
            Ok(bytes) => plist::Value::Integer(u64::from_le_bytes(bytes).into()),
            Err(_) => unreadable(),
        },
        REG_SZ | REG_EXPAND_SZ => match utf16(data) {
            Some(text) => plist::Value::String(text.trim_end_matches('\0').to_string()),
            None => unreadable(),
        },
        REG_MULTI_SZ => match utf16(data) {
            Some(text) => plist::Value::Array(
                text.split('\0')
                    .filter(|entry| !entry.is_empty())
                    .map(|entry| plist::Value::String(entry.to_string()))
                    .collect(),
            ),
            None => unreadable(),
        },
        _ => unreadable(),
    }
}

/// A list subkey's values as one array, ordered by their names: numerically where they're
/// numbers (`2` before `10`), then the rest by name.
pub(crate) fn decode_list(mut entries: Vec<RawEntry>) -> plist::Value {
    entries.sort_by(|(a, ..), (b, ..)| match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    });
    plist::Value::Array(entries.iter().map(|(_, kind, data)| decode(*kind, data)).collect())
}

fn utf16(data: &[u8]) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// The policy in force: every name either hive sets, the machine's value wherever both do (the
/// Group Policy rule: computer policy outranks user policy). Names compare case-insensitively, as
/// the registry's do.
#[derive(Debug, Default)]
pub(crate) struct RegistryPolicy {
    /// Lowercased name → the name as written, and its value.
    values: HashMap<String, (String, plist::Value)>,
}

impl RegistryPolicy {
    pub(crate) fn merge(machine: Hive, user: Hive) -> Self {
        let mut policy = Self::default();
        // The user hive first, so the machine's inserts replace it.
        for hive in [user, machine] {
            for (name, value) in flatten(hive) {
                policy.values.insert(name.to_lowercase(), (name, value));
            }
        }
        policy
    }
}

/// One hive's names and values. Within a hive a plain value outranks a list subkey of the same
/// name, so a `REG_MULTI_SZ` and an ADMX list can't both count.
fn flatten(hive: Hive) -> Vec<(String, plist::Value)> {
    let mut out: Vec<(String, plist::Value)> = hive
        .values
        .into_iter()
        .map(|(name, kind, data)| {
            let value = decode(kind, &data);
            (name, value)
        })
        .collect();
    for (name, entries) in hive.lists {
        if !out.iter().any(|(taken, _)| taken.eq_ignore_ascii_case(&name)) {
            out.push((name, decode_list(entries)));
        }
    }
    out
}

impl ManagedPrefsSource for RegistryPolicy {
    fn forced_value(&self, key: &str) -> Option<plist::Value> {
        self.values.get(&key.to_lowercase()).map(|(_, value)| value.clone())
    }

    fn listed_keys(&self) -> Vec<String> {
        self.values.values().map(|(name, _)| name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_policy::keys::{
        self, ALLOWED_CLOUD_AI_HOSTS, DISABLE_AI, DISABLE_UPDATES, DISABLE_USAGE_STATS, MAX_UPDATE_VERSION,
    };
    use plist::Value;

    fn sz(text: &str) -> Vec<u8> {
        text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
    }

    fn multi_sz(entries: &[&str]) -> Vec<u8> {
        let mut joined: Vec<u16> = Vec::new();
        for entry in entries {
            joined.extend(entry.encode_utf16());
            joined.push(0);
        }
        joined.push(0);
        joined.into_iter().flat_map(u16::to_le_bytes).collect()
    }

    fn entry(name: &str, kind: u32, data: Vec<u8>) -> RawEntry {
        (name.to_string(), kind, data)
    }

    #[test]
    fn each_registry_type_decodes_to_its_policy_value() {
        assert_eq!(decode(REG_DWORD, &1u32.to_le_bytes()), Value::Integer(1.into()));
        assert_eq!(decode(REG_DWORD_BIG_ENDIAN, &[0, 0, 0, 2]), Value::Integer(2.into()));
        assert_eq!(decode(REG_QWORD, &3u64.to_le_bytes()), Value::Integer(3.into()));
        assert_eq!(decode(REG_SZ, &sz("0.52")), Value::String("0.52".into()));
        assert_eq!(decode(REG_EXPAND_SZ, &sz("%X%")), Value::String("%X%".into()));
        assert_eq!(
            decode(REG_MULTI_SZ, &multi_sz(&["api.openai.com", "*.openai.azure.com"])),
            Value::Array(vec![
                Value::String("api.openai.com".into()),
                Value::String("*.openai.azure.com".into())
            ])
        );
    }

    /// Rule 4: what we can't read is the wrong type for every key, so each key restricts.
    #[test]
    fn malformed_or_binary_values_read_as_data() {
        assert!(matches!(decode(REG_BINARY, &[1]), Value::Data(_)));
        assert!(matches!(decode(0, &[]), Value::Data(_)));
        assert!(matches!(decode(REG_DWORD, &[1, 0]), Value::Data(_)));
        assert!(matches!(decode(REG_SZ, &[0x41]), Value::Data(_)));
        // An unpaired surrogate isn't text.
        assert!(matches!(decode(REG_SZ, &0xD800u16.to_le_bytes()), Value::Data(_)));
    }

    #[test]
    fn an_admx_list_subkey_reads_in_numeric_order() {
        let list = decode_list(vec![
            entry("10", REG_SZ, sz("ten.example")),
            entry("2", REG_SZ, sz("two.example")),
            entry("1", REG_SZ, sz("one.example")),
        ]);
        assert_eq!(
            list,
            Value::Array(vec![
                Value::String("one.example".into()),
                Value::String("two.example".into()),
                Value::String("ten.example".into()),
            ])
        );
    }

    #[test]
    fn the_machine_hive_wins_and_names_are_case_insensitive() {
        let machine = Hive {
            values: vec![entry("DisableUpdates", REG_DWORD, 1u32.to_le_bytes().to_vec())],
            lists: Vec::new(),
        };
        let user = Hive {
            values: vec![
                entry("disableupdates", REG_DWORD, 0u32.to_le_bytes().to_vec()),
                entry("DisableUsageStats", REG_SZ, sz("true")),
            ],
            lists: Vec::new(),
        };
        let policy = RegistryPolicy::merge(machine, user);
        assert_eq!(policy.forced_value(DISABLE_UPDATES), Some(Value::Integer(1.into())));
        assert_eq!(policy.forced_value("DISABLEUPDATES"), Some(Value::Integer(1.into())));
        assert_eq!(
            policy.forced_value(DISABLE_USAGE_STATS),
            Some(Value::String("true".into()))
        );
        assert_eq!(policy.forced_value(DISABLE_AI), None);

        // Machine wins: the user's 0 can't turn the machine's DisableUpdates off.
        let expected = keys::parse(&crate::managed_policy::source::FakeSource::with(&[
            (DISABLE_UPDATES, Value::Boolean(true)),
            (DISABLE_USAGE_STATS, Value::Boolean(true)),
        ]));
        assert_eq!(keys::parse(&policy).policy, expected.policy);
    }

    #[test]
    fn a_plain_value_outranks_a_list_subkey_of_the_same_name() {
        let hive = Hive {
            values: vec![entry(ALLOWED_CLOUD_AI_HOSTS, REG_MULTI_SZ, multi_sz(&["a.example"]))],
            lists: vec![(
                ALLOWED_CLOUD_AI_HOSTS.to_string(),
                vec![entry("1", REG_SZ, sz("b.example"))],
            )],
        };
        let policy = RegistryPolicy::merge(hive, Hive::default());
        assert_eq!(
            policy.forced_value(ALLOWED_CLOUD_AI_HOSTS),
            Some(Value::Array(vec![Value::String("a.example".into())]))
        );
    }

    /// Every key reads off registry types the way the same plist value would, so a GPO and a
    /// macOS profile with the same settings make the same policy.
    #[test]
    fn registry_policy_parses_like_the_same_plist_policy() {
        let machine = Hive {
            values: vec![
                entry(DISABLE_AI, REG_DWORD, 1u32.to_le_bytes().to_vec()),
                entry(MAX_UPDATE_VERSION, REG_SZ, sz("0.52")),
            ],
            lists: vec![(
                ALLOWED_CLOUD_AI_HOSTS.to_string(),
                vec![entry("1", REG_SZ, sz("api.openai.com"))],
            )],
        };
        let from_registry = keys::parse(&RegistryPolicy::merge(machine, Hive::default()));
        let from_plist = keys::parse(&crate::managed_policy::source::FakeSource::with(&[
            (DISABLE_AI, Value::Boolean(true)),
            (MAX_UPDATE_VERSION, Value::String("0.52".into())),
            (
                ALLOWED_CLOUD_AI_HOSTS,
                Value::Array(vec![Value::String("api.openai.com".into())]),
            ),
        ]));
        assert_eq!(from_registry.policy, from_plist.policy);
        assert!(from_registry.warnings.is_empty(), "{:?}", from_registry.warnings);
    }
}
