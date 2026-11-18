//! `validate_filename` against each destination's [`NameRules`].
//!
//! The cases live in `name_rules_cases.json`, which the frontend's
//! `filename-validation.test.ts` runs against its keystroke mirror too: both
//! suites reading ONE file is what keeps the two languages agreeing.

use std::collections::BTreeMap;

#[cfg(target_os = "macos")]
use super::NameTier;
use super::{
    LengthLimit, LengthUnit, MAX_NAME_BYTES, NameFormat, NameRules, PortabilityWarning, ValidationError,
    validate_filename, validate_path_length,
};
use crate::filesystem_kind::FilesystemKind;

#[derive(serde::Deserialize)]
struct Spec {
    rules: BTreeMap<String, NameRules>,
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    repeat: Option<usize>,
    all: Option<String>,
    character: Option<String>,
    reserved: Option<String>,
    #[serde(flatten)]
    per_rules: BTreeMap<String, serde_json::Value>,
}

impl Case {
    fn name(&self) -> String {
        self.name.repeat(self.repeat.unwrap_or(1))
    }

    fn expected(&self, rules_name: &str) -> &str {
        self.per_rules
            .get(rules_name)
            .and_then(serde_json::Value::as_str)
            .or(self.all.as_deref())
            .unwrap_or_else(|| panic!("{:?} names no answer for {rules_name}", self.name))
    }
}

/// The wire spelling of a refusal's kind, the one the case file uses.
fn kind_of(error: &ValidationError) -> &'static str {
    match error {
        ValidationError::Empty => "empty",
        ValidationError::DotName => "dotName",
        ValidationError::DisallowedCharacter { .. } => "disallowedCharacter",
        ValidationError::NameTooLong { .. } => "nameTooLong",
        ValidationError::PathTooLong { .. } => "pathTooLong",
        ValidationError::ReservedName { .. } => "reservedName",
        ValidationError::EndsWithSpaceOrPeriod => "endsWithSpaceOrPeriod",
    }
}

/// The case file's spelling of a warning: `warn:` plus its wire kind.
fn warning_of(warning: &PortabilityWarning) -> &'static str {
    match warning {
        PortabilityWarning::WindowsCharacter { .. } => "warn:windowsCharacter",
        PortabilityWarning::ReservedName { .. } => "warn:reservedName",
        PortabilityWarning::EndsWithSpaceOrPeriod => "warn:endsWithSpaceOrPeriod",
    }
}

fn spec() -> Spec {
    serde_json::from_str(include_str!("name_rules_cases.json")).expect("name_rules_cases.json parses")
}

#[test]
fn every_case_answers_as_the_shared_file_says() {
    let spec = spec();
    for case in &spec.cases {
        let name = case.name();
        for (rules_name, rules) in &spec.rules {
            let expected = case.expected(rules_name);
            let context = format!("{:?} under {rules_name}", case.name);
            match validate_filename(&name, *rules) {
                Ok(None) => assert_eq!(expected, "ok", "{context}"),
                Ok(Some(warning)) => {
                    assert_eq!(expected, warning_of(&warning), "{context}: {warning:?}");
                    match warning {
                        PortabilityWarning::WindowsCharacter { character } => {
                            assert_eq!(Some(&character), case.character.as_ref(), "{context}");
                        }
                        PortabilityWarning::ReservedName { name: reserved } => {
                            assert_eq!(Some(&reserved), case.reserved.as_ref(), "{context}");
                        }
                        PortabilityWarning::EndsWithSpaceOrPeriod => {}
                    }
                }
                Err(error) => {
                    assert_eq!(expected, kind_of(&error), "{context}: {error:?}");
                    match error {
                        ValidationError::DisallowedCharacter { character } => {
                            assert_eq!(Some(&character), case.character.as_ref(), "{context}");
                        }
                        ValidationError::ReservedName { name: reserved } => {
                            assert_eq!(Some(&reserved), case.reserved.as_ref(), "{context}");
                        }
                        ValidationError::NameTooLong { length, max, unit } => {
                            assert_eq!(max, rules.name_limit.max, "{context}");
                            assert_eq!(unit, rules.name_limit.unit, "{context}");
                            // `length` is the name's count in the unit that refused it.
                            assert_eq!(length, rules.name_limit.length_of(&name), "{context}");
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// The case file's rules sets are the ones the kinds and this host produce,
/// so the cases test what ships, not a set nobody builds.
#[test]
fn the_case_files_rules_are_the_ones_kinds_build() {
    let spec = spec();
    assert_eq!(spec.rules["posix"], NameRules::POSIX);
    assert_eq!(
        spec.rules["share"],
        FilesystemKind::Smb
            .name_rules()
            .with_path_limit(Some(LengthLimit::SMB_PATH))
    );
    assert_eq!(
        spec.rules["apfs"].with_path_limit(None),
        FilesystemKind::Apfs.name_rules(),
        "APFS counts UTF-16 units"
    );
    assert_eq!(
        spec.rules["hfsPlus"].with_path_limit(None),
        FilesystemKind::HfsPlus.name_rules(),
        "HFS+ counts the decomposed name's UTF-16 units"
    );
    let stick = FilesystemKind::ExFat.name_rules();
    assert_eq!(stick.format, NameFormat::Windows);
    assert_eq!(stick.name_limit, LengthLimit::UTF16_255);
    let host_stick = [
        &spec.rules["stickOnMac"],
        &spec.rules["stickOnLinux"],
        &spec.rules["windowsDisk"],
    ]
    .into_iter()
    .any(|rules| (rules.windows_characters, rules.reserved_names) == (stick.windows_characters, stick.reserved_names));
    assert!(
        host_stick,
        "this host's Windows-format tiers are one the case file covers: {stick:?}"
    );
}

/// macOS's drivers take every name and store stand-ins, so nothing about a
/// Windows-format name is refused there (`docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`).
#[cfg(target_os = "macos")]
#[test]
fn a_mac_warns_where_its_drivers_store_stand_ins() {
    let stick = FilesystemKind::Fat32.name_rules();
    assert_eq!(
        (stick.windows_characters, stick.reserved_names),
        (NameTier::Warn, NameTier::Warn)
    );
}

#[test]
fn the_name_floor_stays_255_bytes() {
    assert_eq!(MAX_NAME_BYTES, 255);
    assert_eq!(
        validate_filename(&"é".repeat(128), NameRules::POSIX),
        Err(ValidationError::NameTooLong {
            length: 256,
            max: 255,
            unit: LengthUnit::Bytes
        })
    );
}

#[test]
fn a_path_limit_counts_in_its_own_unit_inclusive() {
    let bytes = NameRules::POSIX.with_path_limit(Some(LengthLimit {
        max: 10,
        unit: LengthUnit::Bytes,
    }));
    assert_eq!(validate_path_length(std::path::Path::new("/aaaaaaaaa"), bytes), Ok(()));
    assert_eq!(
        validate_path_length(std::path::Path::new("/ééééé"), bytes),
        Err(ValidationError::PathTooLong {
            length: 11,
            max: 10,
            unit: LengthUnit::Bytes
        })
    );
    let units = bytes.with_path_limit(Some(LengthLimit {
        max: 10,
        unit: LengthUnit::Utf16Units,
    }));
    assert_eq!(validate_path_length(std::path::Path::new("/ééééé"), units), Ok(()));
    // No limit is the backend answering for itself.
    assert_eq!(
        validate_path_length(std::path::Path::new(&"/a".repeat(5000)), NameRules::POSIX),
        Ok(())
    );
}

/// Every name Win32 refuses or rewrites: a backslash (a path separator), a colon (an NTFS
/// alternate data stream), a control character, a device name, and a trailing space or period
/// (stripped, so `dup.` opens `dup`).
const WIN32_REFUSED: [&str; 7] = ["x\\y", "a:b", "tab\tname", "con", "con.txt", "dup.", "dup "];

/// Win32's floor: every class refused.
const WIN32_REFUSALS: super::WindowsFormatTiers = super::WindowsFormatTiers {
    characters: NameTier::Refuse,
    reserved_names: NameTier::Refuse,
};

#[test]
fn the_win32_floor_refuses_what_win32_refuses_whatever_the_kind_said() {
    // `Other` (a probe that can't tell) is the weakest answer a kind lookup gives.
    let floored = FilesystemKind::Other.name_rules().at_least(WIN32_REFUSALS);
    for name in WIN32_REFUSED {
        assert!(validate_filename(name, floored).is_err(), "{name:?} passed");
    }
    // A floor never weakens rules that were already stricter, and leaves the limits alone.
    assert_eq!(
        NameRules::windows_format().at_least(WIN32_REFUSALS).name_limit,
        LengthLimit::UTF16_255
    );
    assert_eq!(floored.name_limit, FilesystemKind::Other.name_rules().name_limit);
}

/// A local disk on a Windows host is held to Win32's refusals whatever its kind; elsewhere the
/// host adds nothing of its own (the kind's tiers stand).
#[test]
fn a_host_local_disk_is_held_to_this_hosts_own_refusals() {
    let rules = NameRules::POSIX.for_host_local_disk();
    if cfg!(target_os = "windows") {
        for name in WIN32_REFUSED {
            assert!(validate_filename(name, rules).is_err(), "{name:?} passed");
        }
    } else {
        assert_eq!(rules, NameRules::POSIX);
    }
}
