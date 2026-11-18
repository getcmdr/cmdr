//! `policies.json`, the one policy definition the tests here, the `/trust` page, and the Group
//! Policy template read. Test-only: the app reads `keys.rs`,
//! and the tests hold the two together (`DETAILS.md` § The policy definition).

use serde::Deserialize;

const POLICIES_JSON: &str = include_str!("policies.json");

/// How a policy's value is written: what the macOS profile's plist type and the ADMX element are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Kind {
    /// A plist `<true/>` / `<false/>`; a `REG_DWORD`, 1 or 0.
    Bool,
    /// A plist `<string>`; a `REG_SZ`.
    String,
    /// A plist `<array>` of strings; a subkey of `REG_SZ` values named 1, 2, … (an ADMX `<list>`).
    StringList,
}

#[derive(Debug, Deserialize)]
pub(super) struct Policy {
    pub(super) name: String,
    pub(super) kind: Kind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Definition {
    pub(super) preference_domain: String,
    pub(super) windows_policy_key: String,
    pub(super) policies: Vec<Policy>,
}

pub(super) fn load() -> Definition {
    serde_json::from_str(POLICIES_JSON).expect("policies.json matches `Definition`")
}
