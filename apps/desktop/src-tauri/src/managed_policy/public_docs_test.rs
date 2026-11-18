//! The drift guard: the policy definition (`policies.json`), the sample profile, the bare
//! preference file, and the `/trust` key list are the public mirror of `keys.rs`, so a key added
//! here can't be missing there (or the other way round), and every example value an admin starts
//! from parses without a warning.

use std::collections::BTreeSet;

use super::definition::{self, Kind};
use super::keys::{self, ALL_KEYS};
use super::source::FakeSource;

const PREFERENCE_FILE: &str = include_str!("../../../../website/public/mdm/com.veszelovszki.cmdr.plist");
const PROFILE: &str = include_str!("../../../../website/public/mdm/cmdr-managed-preferences.mobileconfig");
const TRUST_TS: &str = include_str!("../../../../website/src/lib/trust-mdm.ts");

fn expected_keys() -> BTreeSet<String> {
    ALL_KEYS.iter().map(ToString::to_string).collect()
}

pub(super) fn read_dictionary(text: &str, what: &str) -> plist::Dictionary {
    plist::Value::from_reader_xml(text.as_bytes())
        .unwrap_or_else(|e| panic!("{what} isn't a valid plist: {e}"))
        .into_dictionary()
        .unwrap_or_else(|| panic!("{what} isn't a dictionary at the top"))
}

/// The keys of the profile's one Cmdr payload, minus the `Payload*` bookkeeping every payload has.
fn profile_payload() -> plist::Dictionary {
    let profile = read_dictionary(PROFILE, "the sample profile");
    let payloads = profile
        .get("PayloadContent")
        .and_then(plist::Value::as_array)
        .expect("the profile has a PayloadContent array");
    let mut cmdr: Vec<&plist::Dictionary> = payloads
        .iter()
        .filter_map(plist::Value::as_dictionary)
        .filter(|p| p.get("PayloadType").and_then(plist::Value::as_string) == Some(crate::config::BUNDLE_ID))
        .collect();
    assert_eq!(
        cmdr.len(),
        1,
        "the profile needs exactly one payload of type {}",
        crate::config::BUNDLE_ID
    );
    cmdr.remove(0)
        .iter()
        .filter(|(key, _)| !key.starts_with("Payload"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The names `policyEffects` in `trust-mdm.ts` gives a `/trust` effect: its `  Name:` lines.
fn trust_page_effect_keys() -> BTreeSet<String> {
    let start = TRUST_TS
        .find("const policyEffects")
        .expect("trust-mdm.ts declares policyEffects");
    let rest = &TRUST_TS[start..];
    let block = &rest[..rest.find("\n}").expect("policyEffects ends with a `}` line")];
    block
        .lines()
        .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
        .filter_map(|line| line.trim().split_once(':'))
        .map(|(name, _)| name.trim_matches('\'').to_string())
        .collect()
}

fn assert_parses_cleanly(dictionary: &plist::Dictionary, what: &str) {
    let entries: Vec<(&str, plist::Value)> = dictionary
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect();
    let parsed = keys::parse(&FakeSource::with(&entries));
    assert!(
        parsed.warnings.is_empty(),
        "{what} has an example value Cmdr can't read: {:?}",
        parsed.warnings
    );
}

#[test]
fn the_preference_file_carries_every_key_and_nothing_else() {
    let dictionary = read_dictionary(PREFERENCE_FILE, "the preference file");
    assert_eq!(dictionary.keys().cloned().collect::<BTreeSet<_>>(), expected_keys());
    assert_parses_cleanly(&dictionary, "the preference file");
}

#[test]
fn the_sample_profile_carries_every_key_and_nothing_else() {
    let payload = profile_payload();
    assert_eq!(payload.keys().cloned().collect::<BTreeSet<_>>(), expected_keys());
    assert_parses_cleanly(&payload, "the sample profile");
}

#[test]
fn the_profile_and_the_preference_file_agree_on_every_value() {
    assert_eq!(
        profile_payload(),
        read_dictionary(PREFERENCE_FILE, "the preference file")
    );
}

/// `/trust` builds its key list from the definition, and its effect copy covers exactly those keys.
#[test]
fn the_trust_page_has_an_effect_for_every_key_and_nothing_else() {
    assert_eq!(trust_page_effect_keys(), expected_keys());
}

/// The definition `/trust` and the ADMX generator read names what the app reads: every key in
/// catalog order, the macOS preference domain, and the Windows registry key, to the character.
#[test]
fn the_policy_definition_matches_what_the_app_reads() {
    let definition = definition::load();
    let names: Vec<&str> = definition.policies.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ALL_KEYS);
    assert_eq!(definition.preference_domain, crate::config::BUNDLE_ID);
    assert_eq!(
        definition.windows_policy_key,
        super::platform::registry_values::POLICY_KEY
    );
}

/// Each key's example value in the sample profile has the plist type its kind says, so the type
/// `/trust` shows for it is the one the profile carries.
#[test]
fn the_sample_profile_writes_each_key_as_its_kind() {
    let payload = profile_payload();
    for policy in definition::load().policies {
        let value = payload.get(&policy.name).expect("the profile carries every key");
        let matches = match policy.kind {
            Kind::Bool => value.as_boolean().is_some(),
            Kind::String => value.as_string().is_some(),
            Kind::StringList => value
                .as_array()
                .is_some_and(|entries| entries.iter().all(|e| e.as_string().is_some())),
        };
        assert!(matches, "{} is {value:?}, not a {:?}", policy.name, policy.kind);
    }
}
