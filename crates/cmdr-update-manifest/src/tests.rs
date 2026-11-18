use super::*;
use crate::history::{Reader, gate, read};
use proptest::prelude::*;
use serde_json::json;

/// A named change to a manifest.
type Edit = (&'static str, fn(&mut serde_json::Value));

/// A named body and the refusal it must get.
type RefusalCase = (&'static str, Vec<u8>, fn(&ManifestRefusal) -> bool);

fn keys(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_string()).collect()
}

fn manifest_json(version: &str, platforms: &[&str]) -> serde_json::Value {
    let platforms: serde_json::Map<String, serde_json::Value> = platforms
        .iter()
        .map(|key| {
            (
                (*key).to_string(),
                json!({ "url": format!("https://example.invalid/{key}.tar.gz"), "signature": format!("sig-{key}") }),
            )
        })
        .collect();
    json!({ "version": version, "platforms": platforms })
}

fn manifest(version: &str, platforms: &[&str]) -> UpdateManifest {
    parse(manifest_json(version, platforms).to_string().as_bytes()).expect("a well-formed manifest parses")
}

fn offer(m: &UpdateManifest, current: &str, wanted: &[&str]) -> Option<UpdateInfo> {
    match check_manifest(m, current, &keys(wanted)).expect("no refusal") {
        ManifestAnswer::Update(info) => Some(info),
        ManifestAnswer::UpToDate | ManifestAnswer::NoBuildForThisPlatform => None,
    }
}

#[test]
fn newer_version_with_this_platform_is_an_update() {
    let m = manifest("0.41.0", &["darwin-aarch64", "darwin-x86_64", "darwin-universal"]);
    let info = offer(&m, "0.40.0", &["darwin-x86_64"]).expect("0.41.0 is newer than 0.40.0");
    assert_eq!(info.version.to_string(), "0.41.0");
    assert_eq!(info.platform_key, "darwin-x86_64");
    assert_eq!(info.url, "https://example.invalid/darwin-x86_64.tar.gz");
    assert_eq!(info.signature, "sig-darwin-x86_64");
}

/// The installer-specific key wins over the bare one, the order the Tauri plugin reads them in.
#[test]
fn the_first_key_the_manifest_carries_wins() {
    let m = manifest("0.41.0", &["windows-x86_64", "windows-x86_64-nsis"]);
    let wanted = ["windows-x86_64-nsis", "windows-x86_64"];
    assert_eq!(
        offer(&m, "0.40.0", &wanted).map(|info| info.platform_key),
        Some("windows-x86_64-nsis".to_string())
    );
    let bare_only = manifest("0.41.0", &["windows-x86_64"]);
    assert_eq!(
        offer(&bare_only, "0.40.0", &wanted).map(|info| info.platform_key),
        Some("windows-x86_64".to_string())
    );
}

#[test]
fn same_or_older_version_is_up_to_date() {
    let m = manifest("0.40.0", &["darwin-aarch64"]);
    for current in ["0.40.0", "0.41.0"] {
        assert!(matches!(
            check_manifest(&m, current, &keys(&["darwin-aarch64"])),
            Ok(ManifestAnswer::UpToDate)
        ));
    }
}

/// A manifest that lost this build's platform entry is its own answer, never "you're up to date":
/// the install would otherwise sit on an old version with nothing said. The release gate makes it
/// impossible to ship.
#[test]
fn a_newer_version_with_no_entry_for_this_platform_is_no_build_for_this_platform() {
    let m = manifest("0.41.0", &["darwin-aarch64", "darwin-x86_64"]);
    assert!(matches!(
        check_manifest(&m, "0.40.0", &keys(&["windows-x86_64-nsis", "windows-x86_64"])),
        Ok(ManifestAnswer::NoBuildForThisPlatform)
    ));
}

#[test]
fn a_prerelease_is_older_than_its_release() {
    let m = manifest("0.41.0-rc1", &["darwin-aarch64"]);
    assert!(
        offer(&m, "0.41.0", &["darwin-aarch64"]).is_none(),
        "semver orders a prerelease below its release, so 0.41.0 must not downgrade"
    );
}

/// Required data that's wrong refuses the manifest, with a reason: never "you're up to date".
#[test]
fn bad_required_data_is_a_refusal_never_up_to_date() {
    let cases: Vec<RefusalCase> = vec![
        (
            "a non-semver version",
            br#"{"version":"latest","platforms":{}}"#.to_vec(),
            |r| matches!(r, ManifestRefusal::VersionNotSemver { .. }),
        ),
        (
            "a v-prefixed version",
            br#"{"version":"v0.41.0","platforms":{}}"#.to_vec(),
            |r| matches!(r, ManifestRefusal::VersionNotSemver { .. }),
        ),
        ("no version", br#"{"platforms":{}}"#.to_vec(), |r| {
            matches!(r, ManifestRefusal::NotJson { .. })
        }),
        ("a null version", br#"{"version":null,"platforms":{}}"#.to_vec(), |r| {
            matches!(r, ManifestRefusal::NotJson { .. })
        }),
        ("a numeric version", br#"{"version":41,"platforms":{}}"#.to_vec(), |r| {
            matches!(r, ManifestRefusal::NotJson { .. })
        }),
        ("no platforms", br#"{"version":"0.41.0"}"#.to_vec(), |r| {
            matches!(r, ManifestRefusal::NotJson { .. })
        }),
        (
            "platforms as a list",
            br#"{"version":"0.41.0","platforms":[]}"#.to_vec(),
            |r| matches!(r, ManifestRefusal::NotJson { .. }),
        ),
        ("a truncated body", br#"{"version":"0.41.0","platfo"#.to_vec(), |r| {
            matches!(r, ManifestRefusal::NotJson { .. })
        }),
        ("an empty body", Vec::new(), |r| {
            matches!(r, ManifestRefusal::NotJson { .. })
        }),
        (
            "an HTML error page with a 200",
            b"\n  <!doctype html><title>Bad gateway</title>".to_vec(),
            |r| matches!(r, ManifestRefusal::LooksLikeHtml),
        ),
        (
            "a body that isn't UTF-8",
            b"{\"version\":\"0.41.0\xff\"}".to_vec(),
            |r| matches!(r, ManifestRefusal::NotUtf8),
        ),
        ("a huge body", vec![b' '; MAX_MANIFEST_BYTES + 1], |r| {
            matches!(r, ManifestRefusal::TooLarge { .. })
        }),
        (
            "a duplicate version",
            br#"{"version":"0.41.0","version":"0.42.0","platforms":{}}"#.to_vec(),
            |r| matches!(r, ManifestRefusal::NotJson { .. }),
        ),
    ];
    for (name, body, is_expected) in cases {
        match parse(&body) {
            Ok(m) => panic!("{name}: parsed as {m:?}"),
            Err(refusal) => assert!(is_expected(&refusal), "{name}: {refusal:?}"),
        }
    }
}

/// This install's own entry broken is a refusal (the release meant to offer it); another
/// platform's broken entry costs that platform alone.
#[test]
fn a_broken_entry_refuses_only_the_install_it_belongs_to() {
    for broken in [
        json!({ "url": "https://example.invalid/a" }),
        json!({ "url": null, "signature": "s" }),
        json!({ "url": 7, "signature": "s" }),
        json!({ "url": "http://example.invalid/a", "signature": "s" }),
        json!({ "url": "Cmdr.app.tar.gz", "signature": "s" }),
        json!({ "url": "https://example.invalid/a", "signature": "" }),
        json!("https://example.invalid/a"),
        json!([]),
        json!(null),
    ] {
        let mut body = manifest_json("0.41.0", &["darwin-aarch64"]);
        body["platforms"]["windows-x86_64"] = broken.clone();
        let m = parse(body.to_string().as_bytes()).unwrap_or_else(|e| panic!("{broken}: {e}"));
        assert!(offer(&m, "0.40.0", &["darwin-aarch64"]).is_some(), "{broken}");
        assert!(
            matches!(
                check_manifest(&m, "0.40.0", &keys(&["windows-x86_64"])),
                Err(ManifestRefusal::BadEntry { .. })
            ),
            "{broken}"
        );
    }
}

/// A broken advisory entry costs the notice, never the update: the list only ever adds a notice
/// (no advisory refuses an install, on macOS either), so dropping it is the safe side. A strict
/// read once failed the whole parse, so every install on every OS stopped updating.
#[test]
fn a_malformed_advisory_entry_is_dropped_and_the_manifest_still_parses() {
    for broken in [
        json!({ "url": "https://example.invalid/list.json" }),
        json!({ "url": null, "signature": "s" }),
        json!({ "url": "http://example.invalid/list.json", "signature": "s" }),
        json!("https://example.invalid/list.json"),
        json!([]),
        json!(null),
    ] {
        let mut body = manifest_json("0.41.0", &["darwin-aarch64"]);
        body["securityAdvisories"] = broken.clone();
        let m = parse(body.to_string().as_bytes()).unwrap_or_else(|e| panic!("{broken}: {e}"));
        assert!(m.security_advisories.is_none(), "{broken}");
        assert!(offer(&m, "0.40.0", &["darwin-aarch64"]).is_some(), "{broken}");
    }
    let mut body = manifest_json("0.41.0", &["darwin-aarch64"]);
    body["securityAdvisories"] = json!({ "url": "https://example.invalid/list.json", "signature": "s" });
    let m = parse(body.to_string().as_bytes()).expect("parses");
    assert_eq!(
        offer(&m, "0.40.0", &["darwin-aarch64"])
            .and_then(|info| info.security_advisories)
            .map(|entry| entry.url),
        Some("https://example.invalid/list.json".to_string())
    );
}

// ---- The writer's output and every published shape, in every reader that shipped ----

/// The release writer's own fixtures, read at test time rather than embedded: the compiling lanes
/// fingerprint them (`rustEmbeddedInputs`), and the source scanners, which never run this,
/// needn't. The Go side pins both (`scripts/release-assets/latest_test.go`, `validate_test.go`).
fn writer_fixture(name: &str) -> String {
    let path = format!(
        "{}/../../scripts/release-assets/testdata/v0.61.0/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// v0.61.0 as `latest-json` builds it today (with `downloads`).
static WRITER_BUILT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| writer_fixture("latest.built.json"));

#[test]
fn every_published_shape_reads_in_every_reader_that_shipped() {
    // Every manifest shape that went out: the plugin era, the `dmgSizes` era, the versioned file
    // names, the mirror URLs (v0.61.0 as published), and today's writer.
    let published = writer_fixture("latest.json");
    let shapes = [
        ("v0.7.1", include_str!("history/v0.7.1.json")),
        ("v0.35.0", include_str!("history/v0.35.0.json")),
        ("v0.54.0", include_str!("history/v0.54.0.json")),
        ("v0.61.0", published.as_str()),
        ("today's writer", WRITER_BUILT.as_str()),
    ];
    for (name, body) in shapes {
        for reader in Reader::ALL {
            let reading = read(reader, body.as_bytes()).unwrap_or_else(|e| panic!("{name} in {reader:?}: {e}"));
            for key in history::LEGACY_KEYS {
                assert!(reading.urls.contains_key(key), "{name} in {reader:?} has no {key}");
            }
        }
        gate(body.as_bytes()).unwrap_or_else(|problems| panic!("{name}: {problems:?}"));
    }
}

/// Fields may be added: every reader ignores a member it doesn't know, at the top and in an entry.
#[test]
fn an_added_field_reads_in_every_reader() {
    let mut body: serde_json::Value = serde_json::from_str(&WRITER_BUILT).expect("JSON");
    body["channel"] = json!({ "name": "beta", "since": 3 });
    body["platforms"]["darwin-aarch64"]["size"] = json!(123);
    gate(body.to_string().as_bytes()).unwrap_or_else(|problems| panic!("{problems:?}"));
}

/// Never removed or retyped: each change below breaks a reader that shipped, which is why the
/// writer's validator refuses it.
#[test]
fn a_removed_or_retyped_field_breaks_a_shipped_reader() {
    let edits: [Edit; 7] = [
        ("no version", |b| drop(b.as_object_mut().map(|o| o.remove("version")))),
        ("a numeric version", |b| b["version"] = json!(61)),
        ("no platforms", |b| {
            drop(b.as_object_mut().map(|o| o.remove("platforms")))
        }),
        ("a numeric pub_date", |b| b["pub_date"] = json!(1)),
        ("a date-only pub_date", |b| b["pub_date"] = json!("2026-11-13")),
        ("numeric notes", |b| b["notes"] = json!(1)),
        ("another platform's entry retyped", |b| {
            b["platforms"]["linux-x86_64"] = json!("x")
        }),
    ];
    for (name, edit) in edits {
        let mut body: serde_json::Value = serde_json::from_str(&WRITER_BUILT).expect("JSON");
        edit(&mut body);
        assert!(gate(body.to_string().as_bytes()).is_err(), "{name} passed every reader");
    }
}

/// Dropping a platform (Intel Macs, say) is the release manifest's decision
/// (`scripts/release-assets/manifest`, a reviewed change the writer's validator holds
/// `platforms` to), so the gate lets a manifest without `darwin-x86_64` through and says who
/// gets no update. What it still refuses is an old reader missing an entry this build's reader
/// finds: a real reader split.
#[test]
fn a_platform_the_release_dropped_passes_the_gate_and_is_named() {
    let mut body: serde_json::Value = serde_json::from_str(&WRITER_BUILT).expect("JSON");
    drop(body["platforms"].as_object_mut().map(|p| p.remove("darwin-x86_64")));
    let report = gate(body.to_string().as_bytes()).unwrap_or_else(|problems| panic!("{problems:?}"));
    assert!(
        report
            .iter()
            .any(|line| line.contains("darwin-x86_64") && line.contains("no update")),
        "{report:?}"
    );
}

#[test]
fn the_gate_refuses_an_advisory_entry_this_build_would_drop() {
    let mut body: serde_json::Value = serde_json::from_str(&WRITER_BUILT).expect("JSON");
    body["securityAdvisories"] = json!({ "url": "https://download.getcmdr.com/v0.61.0/security-advisories.json" });
    let problems = gate(body.to_string().as_bytes()).expect_err("a broken advisory entry");
    assert!(
        problems.iter().any(|p| p.contains("securityAdvisories")),
        "{problems:?}"
    );
}

/// Fields this reader names that the writer doesn't emit yet, each with why. Shrink it as the
/// writer catches up; ❌ never add a field here to quiet a rename mismatch.
const NOT_WRITTEN_YET: &[&str] = &[
    // The Windows and Linux signed advisory list: no release job produces the asset or the entry
    // yet (`scripts/release-assets/DETAILS.md`, the release signing job's open work).
    "securityAdvisories",
];

/// The JSON names a derived `Deserialize` asks for, after every `rename`: serde hands them to
/// `deserialize_struct`, so a deserializer that records them and stops is a field-set probe.
fn field_names<'de, T: Deserialize<'de>>() -> &'static [&'static str] {
    struct Probe(&'static [&'static str]);
    #[derive(Debug)]
    struct Stop;
    impl std::fmt::Display for Stop {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("probe")
        }
    }
    impl std::error::Error for Stop {}
    impl serde::de::Error for Stop {
        fn custom<M: std::fmt::Display>(_: M) -> Self {
            Stop
        }
    }
    impl<'de> serde::Deserializer<'de> for &mut Probe {
        type Error = Stop;
        fn deserialize_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
            Err(Stop)
        }
        fn deserialize_struct<V: serde::de::Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Stop> {
            self.0 = fields;
            Err(Stop)
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
            ignored_any
        }
    }
    let mut probe = Probe(&[]);
    let _ = T::deserialize(&mut probe);
    assert!(!probe.0.is_empty(), "not a struct with named fields");
    probe.0
}

/// `rename_all = "camelCase"` reads a snake_case Rust field under its camelCase name, so a field
/// added for a snake_case key (the writer already emits `pub_date`) would silently never be read.
/// Every name this reader asks for must be one the release writer writes.
#[test]
fn every_field_the_reader_names_is_one_the_writer_writes() {
    let written: serde_json::Value = serde_json::from_str(&WRITER_BUILT).expect("the writer's output is JSON");
    let top = written.as_object().expect("an object");
    for name in field_names::<Wire>() {
        assert!(
            top.contains_key(*name) || NOT_WRITTEN_YET.contains(name),
            "the reader reads `{name}`, which the release writer never writes (a camelCase rename of a snake_case key?)"
        );
    }
    let entry = top["platforms"]
        .as_object()
        .and_then(|platforms| platforms.values().next())
        .and_then(serde_json::Value::as_object)
        .expect("the writer's output has a platform entry");
    for name in field_names::<PlatformEntry>() {
        assert!(
            entry.contains_key(*name),
            "`PlatformEntry` reads `{name}`, which the writer never writes"
        );
    }
}

// ---- Mutated manifests ----

/// A JSON value of any shape, for replacing a field with something it isn't.
fn any_json() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::from),
        any::<i64>().prop_map(serde_json::Value::from),
        ".{0,12}".prop_map(serde_json::Value::from),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(serde_json::Value::from),
            prop::collection::hash_map("[a-z]{1,6}", inner, 0..4)
                .prop_map(|m| serde_json::Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    /// Whatever the bytes (cut short, a byte flipped, garbage appended), the reader answers
    /// instead of panicking, and so do the history readers and the gate.
    #[test]
    fn damaged_bytes_never_panic(cut in 0usize..12_000, flip in 0usize..12_000, byte in any::<u8>(), tail in prop::collection::vec(any::<u8>(), 0..8)) {
        let mut body = WRITER_BUILT.as_bytes().to_vec();
        if let Some(b) = body.get_mut(flip) {
            *b = byte;
        }
        body.truncate(cut.max(1));
        body.extend(tail);
        let _ = parse(&body);
        for reader in Reader::ALL {
            let _ = read(reader, &body);
        }
        let _ = gate(&body);
    }

    /// Any value in place of an optional field, or of another platform's entry, never costs this
    /// install its update: the reader drops what it can't read.
    #[test]
    fn a_bad_optional_field_never_blocks_the_update(
        field in prop::sample::select(vec!["securityAdvisories", "notes", "pub_date", "dmgSizes", "downloads", "somethingNew"]),
        other_key in "[a-z]{1,8}-[a-z0-9_]{1,8}",
        value in any_json(),
    ) {
        let mut body: serde_json::Value = serde_json::from_str(&WRITER_BUILT).expect("JSON");
        body[field] = value.clone();
        if other_key != "darwin-aarch64" {
            body["platforms"][other_key.as_str()] = value;
        }
        let m = parse(body.to_string().as_bytes()).expect("still a manifest");
        let answer = check_manifest(&m, "0.60.0", &keys(&["darwin-aarch64"])).expect("no refusal");
        prop_assert!(matches!(answer, ManifestAnswer::Update(_)));
    }
}
