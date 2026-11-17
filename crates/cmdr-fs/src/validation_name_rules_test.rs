//! `validate_filename` against each destination's [`NameRules`].
//!
//! The cases live in `name_rules_cases.json`, which the frontend's
//! `filename-validation.test.ts` runs against its keystroke mirror too: both
//! suites reading ONE file is what keeps the two languages agreeing.

use super::{MAX_NAME_BYTES, NameRules, ValidationError, validate_filename};

#[derive(serde::Deserialize)]
struct Spec {
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    repeat: Option<usize>,
    all: Option<String>,
    posix: Option<String>,
    smb_share: Option<String>,
    windows: Option<String>,
    character: Option<String>,
    reserved: Option<String>,
}

impl Case {
    fn name(&self) -> String {
        self.name.repeat(self.repeat.unwrap_or(1))
    }

    fn expected(&self, rules: NameRules) -> &str {
        let own = match rules {
            NameRules::Posix => &self.posix,
            NameRules::SmbShare => &self.smb_share,
            NameRules::Windows => &self.windows,
        };
        own.as_deref()
            .or(self.all.as_deref())
            .unwrap_or_else(|| panic!("{:?} names no answer for {rules:?}", self.name))
    }
}

/// The wire spelling of a refusal's kind, the one the case file uses.
fn kind_of(error: &ValidationError) -> &'static str {
    match error {
        ValidationError::Empty => "empty",
        ValidationError::DisallowedCharacter { .. } => "disallowedCharacter",
        ValidationError::NameTooLong { .. } => "nameTooLong",
        ValidationError::PathTooLong { .. } => "pathTooLong",
        ValidationError::ReservedName { .. } => "reservedName",
        ValidationError::EndsWithSpaceOrPeriod => "endsWithSpaceOrPeriod",
    }
}

fn spec() -> Spec {
    serde_json::from_str(include_str!("name_rules_cases.json")).expect("name_rules_cases.json parses")
}

#[test]
fn every_case_answers_as_the_shared_file_says() {
    for case in spec().cases {
        let name = case.name();
        for rules in [NameRules::Posix, NameRules::SmbShare, NameRules::Windows] {
            let expected = case.expected(rules);
            let answer = validate_filename(&name, rules);
            match &answer {
                Ok(()) => assert_eq!(expected, "ok", "{:?} under {rules:?}", case.name),
                Err(error) => {
                    assert_eq!(expected, kind_of(error), "{:?} under {rules:?}: {error:?}", case.name);
                    match error {
                        ValidationError::DisallowedCharacter { character } => {
                            assert_eq!(Some(character), case.character.as_ref(), "{:?}", case.name);
                        }
                        ValidationError::ReservedName { name: reserved } => {
                            assert_eq!(Some(reserved), case.reserved.as_ref(), "{:?}", case.name);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// The JSON's `nameTooLong` case leans on the constant: pin the two together.
#[test]
fn the_length_limit_is_255_bytes_inclusive() {
    assert_eq!(MAX_NAME_BYTES, 255);
    assert_eq!(
        validate_filename(&"é".repeat(128), NameRules::Posix),
        Err(ValidationError::NameTooLong { bytes: 256, max: 255 })
    );
}
