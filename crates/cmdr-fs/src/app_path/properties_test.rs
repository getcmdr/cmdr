//! The properties the app path's safety rests on, over generated input: the escape is
//! lossless and injective both ways, the canonical form is a fixed point, normalization is
//! idempotent, the arithmetic is self-consistent, `Eq`/`Hash`/`Ord` agree with `PathBuf` on
//! POSIX text (`Ord` except on escaped names, `bridge_unix`), and the transition bridge never
//! changes a byte.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use proptest::prelude::*;

use super::super::*;

/// Names built from the characters that matter: separators' neighbors, dots, a space, a `-`
/// (all sorting below `/` bytewise), `\` and `:` (name characters on POSIX), non-ASCII, and
/// both escape alphabets.
fn name_char() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('a'),
        Just('b'),
        Just('.'),
        Just(' '),
        Just('-'),
        Just('\\'),
        Just(':'),
        Just('\u{e9}'),
        Just('\u{1F600}'),
        (0x10_FE80u32..=0x10_FEFF).prop_map(|c| char::from_u32(c).expect("a scalar")),
        (0x10_E800u32..=0x10_EFFF).prop_map(|c| char::from_u32(c).expect("a scalar")),
    ]
}

fn name() -> impl Strategy<Value = String> {
    prop::collection::vec(name_char(), 0..5).prop_map(|chars| chars.into_iter().collect())
}

/// Text shaped like a POSIX path, unnormalized: `//` (leading too), a trailing `/`, `.`,
/// `..`, every name character.
fn posix_text() -> impl Strategy<Value = String> {
    let segment = prop_oneof![
        3 => prop::collection::vec(prop_oneof![Just('a'), Just('b'), Just('.'), Just(' '), Just('-'), Just('\u{e9}')], 1..4)
            .prop_map(|chars| chars.into_iter().collect::<String>()),
        1 => Just(String::new()),
        1 => Just(".".to_string()),
        1 => Just("..".to_string()),
    ];
    prop::collection::vec(segment, 0..6).prop_map(|segments| format!("/{}", segments.join("/")))
}

/// Any text an app path might be parsed from, every root shape included.
fn any_text() -> impl Strategy<Value = String> {
    let root = prop_oneof![
        Just("/".to_string()),
        Just("//".to_string()),
        Just("C:/".to_string()),
        Just("c:".to_string()),
        Just("C:".to_string()),
        Just("//srv/share/".to_string()),
        Just("//srv/share".to_string()),
        Just("sftp://u@h:22/".to_string()),
        Just("sftp://u@h:22".to_string()),
        Just("s3:///".to_string()),
        Just(String::new()),
    ];
    let names = prop::collection::vec(prop_oneof![name(), Just(String::new()), Just(".".to_string())], 0..4);
    (root, names, any::<bool>()).prop_map(|(root, names, trailing)| {
        let mut text = format!("{root}{}", names.join("/"));
        if trailing {
            text.push('/');
        }
        text
    })
}

fn hash_of(path: &AppPath) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn is_unit_escape(c: char) -> bool {
    escape::unit_of(c).is_some()
}

fn is_byte_escape(c: char) -> bool {
    escape::byte_of(c).is_some()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn decoding_the_escape_gives_back_every_byte(bytes in prop::collection::vec(any::<u8>(), 0..24)) {
        let text = escape_bytes(&bytes);
        let decoded = unescape_bytes(&text).expect("only byte escapes");
        prop_assert_eq!(decoded.as_ref(), &bytes[..]);
        let canon = canonical_name(&text).expect("one alphabet");
        prop_assert_eq!(canon.as_ref(), text.as_ref());
    }

    #[test]
    fn decoding_the_escape_gives_back_every_unit(units in prop::collection::vec(prop_oneof![any::<u16>(), 0xD800u16..=0xDFFF], 0..24)) {
        let text = escape_units(&units);
        prop_assert_eq!(unescape_units(&text).expect("only unit escapes"), units);
        let canon = canonical_name(&text).expect("one alphabet");
        prop_assert_eq!(canon.as_ref(), text.as_str());
    }

    /// Injective both ways: equal spellings mean equal bytes (the round trip above), and
    /// different bytes mean different spellings.
    #[test]
    fn different_bytes_get_different_spellings(a in prop::collection::vec(any::<u8>(), 0..12), b in prop::collection::vec(any::<u8>(), 0..12)) {
        prop_assert_eq!(a == b, escape_bytes(&a) == escape_bytes(&b));
    }

    #[test]
    fn different_units_get_different_spellings(a in prop::collection::vec(any::<u16>(), 0..12), b in prop::collection::vec(any::<u16>(), 0..12)) {
        prop_assert_eq!(a == b, escape_units(&a) == escape_units(&b));
    }

    /// #1310's door: `listed_text` gives back exactly what a backend listed, through either
    /// alphabet's literal spelling and through the path door (`from_listed_text`, which the
    /// wire's `from_literal_store` spells alike), separators and all.
    #[test]
    fn listed_text_inverts_the_listing_door(names in prop::collection::vec(name().prop_filter("one name", |n| !n.is_empty() && n != "." && n != ".."), 0..4)) {
        let listed = format!("sftp://u@h:22/{}", names.join("/"));
        for namer in [Namer::Bytes, Namer::Units] {
            let text = escape::literal(&listed, namer);
            let back = listed_text(&text).map(Cow::into_owned);
            prop_assert_eq!(back.as_deref(), Some(listed.as_str()));
        }
        let own = AppPathBuf::from_listed_text(&listed, PathFlavor::Posix).expect("an app path");
        prop_assert_eq!(&own, &AppPathBuf::from_literal_store(&listed, Namer::HOST).expect("an app path"));
        let back = listed_text(own.as_str()).map(Cow::into_owned);
        prop_assert_eq!(back.as_deref(), Some(listed.as_str()));
    }

    /// Fail-closed and collision-free: any app text `listed_text` answers for is one of the
    /// two literal spellings of that answer, so two different listed names never collapse
    /// into one server name, and the answer keeps every separator and NUL where it was (an
    /// escape decodes to no ASCII).
    #[test]
    fn listed_text_answers_only_a_literal_spelling(names in prop::collection::vec(name(), 0..4)) {
        let text = names.join("/");
        if let Some(listed) = listed_text(&text) {
            prop_assert!(
                escape::literal(&listed, Namer::Bytes) == text || escape::literal(&listed, Namer::Units) == text,
                "{:?} -> {:?}", text, listed
            );
            prop_assert_eq!(listed.matches('/').count(), text.matches('/').count());
        }
    }

    /// `to_native(canon(s)) == to_native(s)` and `from_native(to_native(s)) == canon(s)`, for
    /// arbitrary text through each alphabet it can be read in.
    #[test]
    fn the_canonical_form_names_the_same_native_name(text in name()) {
        let Ok(canon) = canonical_name(&text) else {
            prop_assert!(text.chars().any(is_byte_escape) && text.chars().any(is_unit_escape));
            return Ok(());
        };
        let again = canonical_name(&canon).expect("idempotent");
        prop_assert_eq!(again.as_ref(), canon.as_ref());
        if !text.chars().any(is_unit_escape) {
            let native = unescape_bytes(&text).expect("byte escapes only");
            prop_assert_eq!(unescape_bytes(&canon).expect("byte escapes only"), native.clone());
            let respelled = escape_bytes(&native);
            prop_assert_eq!(respelled.as_ref(), canon.as_ref());
        }
        if !text.chars().any(is_byte_escape) {
            let native = unescape_units(&text).expect("unit escapes only");
            prop_assert_eq!(unescape_units(&canon).expect("unit escapes only"), native.clone());
            prop_assert_eq!(escape_units(&native), canon.as_ref());
        }
    }

    /// A native name through a whole path and back, exactly: POSIX bytes.
    #[test]
    fn a_posix_native_path_round_trips(name in prop::collection::vec(any::<u8>().prop_filter("no separator", |b| *b != b'/' && *b != 0), 1..16)) {
        prop_assume!(name != b"." && name != b"..");
        let mut native = b"/d/".to_vec();
        native.extend_from_slice(&name);
        let path = AppPathBuf::from_native_posix(&native, PathFlavor::Posix).expect("an absolute path");
        let back = path.to_native_posix().expect("crosses back");
        prop_assert_eq!(back.as_ref(), &native[..]);
        prop_assert_eq!(AppPath::from_normalized_as(path.as_str(), PathFlavor::Posix).map(AppPath::as_str), Ok(path.as_str()));
    }

    /// A native name through a whole verbatim Windows path and back, exactly: UTF-16 units,
    /// unpaired surrogates included.
    #[test]
    fn a_windows_native_path_round_trips(name in prop::collection::vec(prop_oneof![
        Just(u16::from(b'a')), Just(u16::from(b' ')), Just(0x00E9), 0xD800u16..=0xDFFF, Just(0xD83D), Just(0xDE00), Just(0xDBFF), Just(0xDE80),
    ], 1..16)) {
        let mut native: Vec<u16> = r"\\?\C:\d\".encode_utf16().collect();
        native.extend_from_slice(&name);
        let path = AppPathBuf::from_native_windows(&native, PathFlavor::Windows).expect("an absolute path");
        prop_assert_eq!(path.to_native_windows().expect("crosses back"), native);
        prop_assert_eq!(AppPath::from_normalized_as(path.as_str(), PathFlavor::Windows).map(AppPath::as_str), Ok(path.as_str()));
    }

    #[test]
    fn normalizing_is_idempotent(text in any_text(), windows in any::<bool>()) {
        let flavor = if windows { PathFlavor::Windows } else { PathFlavor::Posix };
        if let Ok(path) = AppPathBuf::parse(&text, flavor) {
            let again = AppPathBuf::parse(path.as_str(), flavor);
            prop_assert_eq!(again.as_ref().map(|p| p.as_str()), Ok(path.as_str()));
        }
    }

    /// `from_normalized_as` takes exactly what `parse` leaves alone (on Windows, a POSIX
    /// root's value too: the in-memory volume's).
    #[test]
    fn from_normalized_accepts_exactly_the_fixed_points_of_parse(text in any_text()) {
        let fixed = |flavor| AppPathBuf::parse(&text, flavor).is_ok_and(|p| p.as_str() == text);
        prop_assert_eq!(AppPath::from_normalized_as(&text, PathFlavor::Posix).is_ok(), fixed(PathFlavor::Posix), "{:?}", text);
        prop_assert_eq!(
            AppPath::from_normalized_as(&text, PathFlavor::Windows).is_ok(),
            fixed(PathFlavor::Windows) || fixed(PathFlavor::Posix),
            "{:?}", text
        );
    }

    #[test]
    fn parent_and_join_are_inverse(text in any_text(), windows in any::<bool>(), child in name()) {
        let flavor = if windows { PathFlavor::Windows } else { PathFlavor::Posix };
        let Ok(path) = AppPathBuf::parse(&text, flavor) else { return Ok(()) };
        if let (Some(parent), Some(file)) = (path.parent(), path.file_name()) {
            prop_assert_eq!(parent.join(AppName::from_app_spelling(file).expect("a canonical name")).expect("it was there"), path.clone());
        } else {
            prop_assert!(path.is_root());
        }
        if let Ok(child) = AppName::from_app_spelling(&child)
            && let Ok(joined) = path.join(child) {
                prop_assert_eq!(joined.parent(), Some(path.as_app_path()));
                prop_assert_eq!(joined.file_name(), Some(child.as_str()));
                prop_assert!(joined.starts_with(&path));
                prop_assert_eq!(AppPath::from_normalized_as(joined.as_str(), flavor).map(AppPath::as_str), Ok(joined.as_str()));
            }
    }

    #[test]
    fn starts_with_and_strip_prefix_agree(a in any_text(), b in any_text(), windows_a in any::<bool>(), windows_b in any::<bool>()) {
        let flavor = |windows: bool| if windows { PathFlavor::Windows } else { PathFlavor::Posix };
        let (Ok(a), Ok(b)) = (AppPathBuf::parse(&a, flavor(windows_a)), AppPathBuf::parse(&b, flavor(windows_b))) else { return Ok(()) };
        for (path, ancestor) in [(&a, &b), (&b, &a), (&a, &a.root().to_owned()), (&b, &a.root().to_owned())] {
            let rel = path.strip_prefix(ancestor);
            prop_assert_eq!(path.starts_with(ancestor), rel.is_some());
            if let Some(rel) = rel {
                // Never across roots, and the tail is always a valid one.
                prop_assert_eq!(path.root(), ancestor.root());
                prop_assert!(RelativeAppPath::new(rel.as_str()).is_ok(), "{:?} under {:?}: {:?}", path, ancestor, rel);
                prop_assert_eq!(&ancestor.join_relative(&rel).expect("it was there"), path);
                if let Some(moved) = path.rebase(ancestor, &b) {
                    prop_assert!(AppPath::from_normalized_as(moved.as_str(), PathFlavor::Windows).is_ok(), "{:?}", moved);
                }
            }
            if let Some(rel) = path.strip_volume_prefix(ancestor) {
                prop_assert!(path.same_root(ancestor));
                prop_assert!(RelativeAppPath::new(rel.as_str()).is_ok(), "{:?} on {:?}: {:?}", path, ancestor, rel);
            }
        }
    }

    /// `Eq`, `Hash`, and `Ord` answer what `PathBuf` answered for the same POSIX text,
    /// unnormalized text included (a bridge value keeps its `/a/` and `/a/./b`), so a set or
    /// map keyed on app paths dedups and sorts exactly as before.
    #[test]
    fn comparison_matches_pathbuf_on_posix_text(a in posix_text(), b in posix_text()) {
        let (typed_a, typed_b) = (AppPath::legacy_unchecked(Path::new(&a)), AppPath::legacy_unchecked(Path::new(&b)));
        let (std_a, std_b) = (PathBuf::from(&a), PathBuf::from(&b));
        prop_assert_eq!(typed_a == typed_b, std_a == std_b, "{:?} {:?}", a, b);
        prop_assert_eq!(typed_a.cmp(&typed_b), std_a.cmp(&std_b), "{:?} {:?}", a, b);
        if typed_a == typed_b {
            prop_assert_eq!(hash_of(&typed_a), hash_of(&typed_b));
        }
        // And on canonical values, `Eq` is the text's.
        if let (Ok(pa), Ok(pb)) = (AppPathBuf::parse(&a, PathFlavor::Posix), AppPathBuf::parse(&b, PathFlavor::Posix)) {
            prop_assert_eq!(pa == pb, pa.as_str() == pb.as_str());
            prop_assert_eq!(pa.cmp(&pb), PathBuf::from(pa.as_str()).cmp(&PathBuf::from(pb.as_str())));
        }
    }

    /// A plain Windows path the OS reported (a `read_dir` child under a plain root) keeps
    /// every name exactly, trailing dots and spaces included: what the reader joined is the
    /// file it names. Only a whole `.` or `..` name resolves.
    #[test]
    fn a_reported_plain_windows_path_keeps_every_name(names in prop::collection::vec(
        prop::collection::vec(prop_oneof![Just('a'), Just('.'), Just(' '), Just('\u{e9}')], 1..5)
            .prop_map(|chars| chars.into_iter().collect::<String>())
            .prop_filter("no dot names", |n| n != "." && n != ".."),
        1..4,
    ), unc in any::<bool>()) {
        let (native_root, app_root) = if unc { (r"\\srv\sh\", "//srv/sh/") } else { (r"C:\", "C:/") };
        let native = format!("{native_root}{}", names.join(r"\"));
        let units: Vec<u16> = native.encode_utf16().collect();
        let path = AppPathBuf::from_native_windows(&units, PathFlavor::Windows).expect("a plain path");
        prop_assert_eq!(path.as_str(), format!("{app_root}{}", names.join("/")));
        prop_assert_eq!(path.names().collect::<Vec<_>>(), names.iter().map(String::as_str).collect::<Vec<_>>());
    }

    /// Typed text never carries a live escape: an escape-range code point is the literal
    /// character, so the OS gets the bytes the person typed.
    #[test]
    fn typed_text_means_what_it_says(typed in name()) {
        let text = format!("/d/{typed}");
        let names: Vec<&str> = text.split('/').filter(|n| !n.is_empty() && *n != ".").collect();
        match AppPathBuf::from_person_typed(&text, PathFlavor::Posix, Namer::Bytes) {
            Ok(path) => {
                let typed_bytes = format!("/{}", names.join("/"));
                let native = path.to_native_posix().expect("local");
                prop_assert_eq!(native.as_ref(), typed_bytes.as_bytes());
                prop_assert!(path.names().all(|n| !n.chars().any(is_unit_escape)));
            }
            Err(refusal) => prop_assert_eq!(refusal, NotAnAppPath::DotName),
        }
    }
}

/// A name this host's `OsStr` holds natively reaches the OS again byte for byte through
/// `from_native_os` and a typed join, whatever its bytes.
#[cfg(unix)]
mod native_name_unix {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use proptest::prelude::*;

    use super::super::super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2_000))]

        #[test]
        fn a_native_name_joins_to_its_own_bytes(name in prop::collection::vec(any::<u8>().prop_filter("no separator", |b| *b != b'/' && *b != 0), 1..16)) {
            prop_assume!(name != b"." && name != b"..");
            let app = AppNameBuf::from_native_os(OsStr::from_bytes(&name)).expect("a plain name");
            let joined = AppPath::from_normalized("/d").expect("canonical").join(&app).expect("joins");
            let native = joined.to_native_posix().expect("crosses");
            let mut expected = b"/d/".to_vec();
            expected.extend_from_slice(&name);
            prop_assert_eq!(native.as_ref(), &expected[..]);
        }
    }
}

/// On Windows, `Eq`, `Hash`, and `Ord` answer what `PathBuf` answers there, bridge text with
/// `\`, drive and UNC prefixes, and verbatim forms included.
#[cfg(windows)]
mod comparison_windows {
    use std::path::{Path, PathBuf};

    use proptest::prelude::*;

    use super::super::super::*;
    use super::hash_of;

    fn windows_text() -> impl Strategy<Value = String> {
        let prefix = prop_oneof![
            Just(String::new()),
            Just("C:".to_string()),
            Just("c:".to_string()),
            Just(r"\\srv\share".to_string()),
            Just("//srv/share".to_string()),
            Just(r"\\?\C:".to_string()),
            Just(r"\\?\UNC\srv\share".to_string()),
            Just("/".to_string()),
        ];
        let segment = prop_oneof![
            3 => prop::collection::vec(prop_oneof![Just('a'), Just('b'), Just('.'), Just(' '), Just('-')], 1..4)
                .prop_map(|chars| chars.into_iter().collect::<String>()),
            1 => Just(String::new()),
            1 => Just(".".to_string()),
            1 => Just("..".to_string()),
        ];
        let sep = prop_oneof![Just('/'), Just('\\')];
        (prefix, prop::collection::vec((sep, segment), 0..5)).prop_map(|(prefix, parts)| {
            let mut text = prefix;
            for (sep, segment) in parts {
                text.push(sep);
                text.push_str(&segment);
            }
            text
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4_000))]

        /// Every text Win32 opens compares as `PathBuf` does here. A POSIX-kind `/x` is the
        /// app's own (the hybrid, `shape::components`), so it's left to the all-host
        /// properties.
        #[test]
        fn comparison_matches_pathbuf_on_windows_text(
            a in windows_text().prop_filter("not POSIX-kind", |t| !super::posix_kind(t)),
            b in windows_text().prop_filter("not POSIX-kind", |t| !super::posix_kind(t)),
        ) {
            let (typed_a, typed_b) = (AppPath::legacy_unchecked(Path::new(&a)), AppPath::legacy_unchecked(Path::new(&b)));
            let (std_a, std_b) = (PathBuf::from(&a), PathBuf::from(&b));
            prop_assert_eq!(typed_a == typed_b, std_a == std_b, "{:?} {:?}", a, b);
            prop_assert_eq!(typed_a.cmp(&typed_b), std_a.cmp(&std_b), "{:?} {:?}", a, b);
            if typed_a == typed_b {
                prop_assert_eq!(hash_of(&typed_a), hash_of(&typed_b));
            }
        }
    }
}

/// The bridge never normalizes: `legacy_unchecked(p).as_std_legacy()` has `p`'s exact bytes.
/// Compared as bytes, ❌ never with `Path ==`, which ignores a trailing `/` and `.`.
#[cfg(unix)]
mod bridge_unix {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use proptest::prelude::*;

    use super::super::super::AppPath;

    fn byte() -> impl Strategy<Value = u8> {
        prop_oneof![Just(b'/'), Just(b'.'), Just(b'\\'), Just(b'a'), Just(b':'), any::<u8>()]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4_000))]

        #[test]
        fn the_bridge_gives_back_every_byte(bytes in prop::collection::vec(byte(), 0..32)) {
            let path = Path::new(OsStr::from_bytes(&bytes));
            let bridged = AppPath::legacy_unchecked(path);
            let back = bridged.as_std_legacy();
            prop_assert_eq!(back.as_os_str().as_bytes(), &bytes[..]);
        }
    }

    #[test]
    fn the_bridge_keeps_what_path_eq_would_hide() {
        for text in ["/a/", "/a/./b", "//a", "/a/../b", "a\\b", "", ".", "/x\u{10FE80}"] {
            let bridged = AppPath::legacy_unchecked(Path::new(text));
            assert_eq!(
                bridged.as_std_legacy().as_os_str().as_bytes(),
                text.as_bytes(),
                "{text:?}"
            );
        }
        let raw = Path::new(OsStr::from_bytes(b"/d/caf\xe9"));
        assert_eq!(AppPath::legacy_unchecked(raw).as_str(), "/d/caf\u{10FEE9}");
    }

    /// Bytes for a rooted path's tail: separators, dots, plain names, the two bytes of `é`
    /// (so valid UTF-8 shows up beside the invalid kind), and any byte at all.
    fn tail_byte() -> impl Strategy<Value = u8> {
        prop_oneof![
            Just(b'/'),
            Just(b'.'),
            Just(b'a'),
            Just(b'-'),
            Just(0xC3),
            Just(0xA9),
            any::<u8>()
        ]
    }

    fn rooted(tail: &[u8]) -> Vec<u8> {
        [b"/".as_slice(), tail].concat()
    }

    fn has_escape(path: &AppPath) -> bool {
        path.as_str().chars().any(super::super::super::escape::is_escape)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4_000))]

        /// On ANY bytes, escaped names included, `Eq` and `Hash` answer what `PathBuf`
        /// answered, so a set or map keyed on a bridged path dedups as before. `Ord` agrees
        /// only while neither side holds an escape: an escaped name (a non-UTF-8 byte, or a
        /// literal escape-alphabet code point) sorts by its escape code point, above every raw
        /// byte, where `PathBuf` sorted the raw byte (`crates/cmdr-fs/DETAILS.md` § "App paths").
        #[test]
        fn comparison_matches_pathbuf_on_any_bytes_but_orders_escapes_apart(
            a in prop::collection::vec(tail_byte(), 0..12),
            b in prop::collection::vec(tail_byte(), 0..12),
        ) {
            let (a, b) = (rooted(&a), rooted(&b));
            let (std_a, std_b) = (Path::new(OsStr::from_bytes(&a)), Path::new(OsStr::from_bytes(&b)));
            let (typed_a, typed_b) = (AppPath::legacy_unchecked(std_a), AppPath::legacy_unchecked(std_b));
            prop_assert_eq!(typed_a == typed_b, std_a == std_b, "{:?} {:?}", std_a, std_b);
            if typed_a == typed_b {
                prop_assert_eq!(super::hash_of(&typed_a), super::hash_of(&typed_b));
            }
            if !has_escape(&typed_a) && !has_escape(&typed_b) {
                prop_assert_eq!(typed_a.cmp(&typed_b), std_a.cmp(std_b), "{:?} {:?}", std_a, std_b);
            }
        }
    }

    /// The divergence, stated: a raw `0x80` byte sorts BELOW `é` (`C3 A9`) as `PathBuf`, and
    /// its escape (U+10FE80, `F4 8F BA 80` in UTF-8) sorts ABOVE it as an app path. Accepted,
    /// not fixed (no path order is persisted); the first milestone that sorts typed paths
    /// meets it (`crates/cmdr-fs/DETAILS.md` § "App paths").
    #[test]
    fn an_escaped_name_sorts_at_its_escape_code_point() {
        let (raw, accented) = (Path::new(OsStr::from_bytes(b"/\x80")), Path::new("/\u{e9}"));
        assert!(raw < accented, "PathBuf sorts the raw byte");
        assert!(
            AppPath::legacy_unchecked(raw) > AppPath::legacy_unchecked(accented),
            "an app path sorts the escape"
        );
    }
}

#[cfg(windows)]
mod bridge_windows {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;

    use proptest::prelude::*;

    use super::super::super::AppPath;

    fn unit() -> impl Strategy<Value = u16> {
        prop_oneof![
            Just(u16::from(b'/')),
            Just(u16::from(b'\\')),
            Just(u16::from(b'.')),
            Just(u16::from(b':')),
            Just(u16::from(b'C')),
            0xD800u16..=0xDFFF,
            any::<u16>(),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4_000))]

        #[test]
        fn the_bridge_gives_back_every_unit(units in prop::collection::vec(unit(), 0..32)) {
            let path = PathBuf::from(OsString::from_wide(&units));
            let bridged = AppPath::legacy_unchecked(&path);
            let back = bridged.as_std_legacy();
            prop_assert_eq!(back.as_os_str().encode_wide().collect::<Vec<u16>>(), units);
        }
    }

    #[test]
    fn the_bridge_keeps_what_path_eq_would_hide() {
        for text in [r"C:\x\", r"C:foo", r"\\?\C:\x", "C:/a/./b", r"a\..\b", "/a/"] {
            let path = PathBuf::from(text);
            let bridged = AppPath::legacy_unchecked(&path);
            let units: Vec<u16> = bridged.as_std_legacy().as_os_str().encode_wide().collect();
            assert_eq!(units, text.encode_utf16().collect::<Vec<u16>>(), "{text:?}");
        }
    }
}

/// Typed arithmetic on a POSIX bridge value answers what `Path` answers, in release builds
/// too (review 2's N6), so a guard written in typed arithmetic during the transition can't
/// answer "no" where `std` said "yes".
#[cfg(unix)]
mod bridge_arithmetic_unix {
    use std::path::Path;

    use proptest::prelude::*;

    use super::super::super::*;
    use super::posix_text;

    fn full_unc(text: &str) -> bool {
        text.starts_with("//") && AppPath::legacy_unchecked(Path::new(text)).is_canonical()
    }

    proptest! {
        #[test]
        fn a_bridge_values_arithmetic_is_paths(
            // A full `//server/share/x` reads as UNC here (`lenient::is_canonical` says why).
            a in posix_text().prop_filter("not UNC-shaped", |t| !full_unc(t)),
            b in posix_text().prop_filter("not UNC-shaped", |t| !full_unc(t)),
        ) {
            let (typed, std_path) = (AppPath::legacy_unchecked(Path::new(&a)), Path::new(&a));
            let parent = typed.parent().map(|p| AppPath::legacy_unchecked(Path::new(p.as_str())).into_owned());
            let std_parent = std_path.parent().map(|p| AppPath::legacy_unchecked(p).into_owned());
            prop_assert_eq!(parent, std_parent, "parent of {:?}", a);
            prop_assert_eq!(typed.file_name(), std_path.file_name().and_then(|n| n.to_str()), "file name of {:?}", a);
            prop_assert_eq!(typed.extension(), std_path.extension().and_then(|n| n.to_str()), "extension of {:?}", a);
            prop_assert_eq!(typed.file_stem(), std_path.file_stem().and_then(|n| n.to_str()), "stem of {:?}", a);
            // `with_file_name` names `Path`'s place, or refuses where that place climbs.
            let name = AppName::from_app_spelling("z").expect("a name");
            let std_renamed = std_path.with_file_name("z");
            match typed.with_file_name(name) {
                Ok(renamed) => {
                    prop_assert!(!std_renamed.components().any(|c| c == std::path::Component::ParentDir), "{:?} renamed through `..`", a);
                    prop_assert_eq!(renamed, AppPath::legacy_unchecked(&std_renamed).into_owned(), "{:?} renamed", a);
                }
                Err(refusal) => {
                    prop_assert_eq!(refusal, NotAnAppPath::DotName, "{:?}", a);
                    prop_assert!(std_renamed.components().any(|c| c == std::path::Component::ParentDir), "{:?} refused though `Path` names a plain place", a);
                }
            }
            // `..` is the one place they part: by text it can name another file, so the type
            // places a `..`-holding value under nothing.
            if !a.split('/').any(|n| n == "..") && !b.split('/').any(|n| n == "..") {
                let ancestor = AppPath::legacy_unchecked(Path::new(&b));
                prop_assert_eq!(typed.starts_with(&ancestor), std_path.starts_with(&b), "{:?} under {:?}", a, b);
                if let Some(rel) = typed.strip_prefix(&ancestor) {
                    prop_assert!(RelativeAppPath::new(rel.as_str()).is_ok(), "a typed tail is canonical: {:?}", rel);
                }
            }
        }
    }
}

/// Whether `text` is POSIX-kind (`shape::components`' test): one `/`, then no second
/// separator.
fn posix_kind(text: &str) -> bool {
    text.starts_with('/') && !text[1..].starts_with(['/', '\\'])
}

/// The hybrid comparison, read as either host reads it, on every OS: it stays an equivalence
/// with a total order and an agreeing hash, whatever mix of shapes it meets (canonical
/// values, bridge text with `\`, `//`, `.`, verbatim prefixes, schemes).
mod hybrid_comparison {
    use std::cmp::Ordering;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use proptest::prelude::*;

    use super::super::super::shape::{Component, components_for};
    use super::super::super::*;

    fn mixed_text() -> impl Strategy<Value = String> {
        let prefix = prop_oneof![
            Just(String::new()),
            Just("/".to_string()),
            Just("//".to_string()),
            Just("C:".to_string()),
            Just(r"\\srv\share".to_string()),
            Just("//srv/share".to_string()),
            Just(r"\\?\C:".to_string()),
            Just("sftp://h".to_string()),
            Just("sftp://".to_string()),
        ];
        let segment = prop_oneof![
            3 => prop::collection::vec(prop_oneof![Just('a'), Just('b'), Just('\\'), Just('.'), Just(':')], 1..4)
                .prop_map(|chars| chars.into_iter().collect::<String>()),
            1 => Just(String::new()),
            1 => Just(".".to_string()),
            1 => Just("..".to_string()),
        ];
        let sep = prop_oneof![Just('/'), Just('\\')];
        (prefix, prop::collection::vec((sep, segment), 0..4)).prop_map(|(prefix, parts)| {
            let mut text = prefix;
            for (sep, segment) in parts {
                text.push(sep);
                text.push_str(&segment);
            }
            text
        })
    }

    fn seq(text: &str, host: PathFlavor) -> Vec<Component<'_>> {
        components_for(text, host).collect()
    }

    fn hash_seq(text: &str, host: PathFlavor) -> u64 {
        let mut hasher = DefaultHasher::new();
        for component in components_for(text, host) {
            component.hash(&mut hasher);
        }
        hasher.finish()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2_000))]

        #[test]
        fn eq_hash_and_ord_agree_under_either_host(a in mixed_text(), b in mixed_text(), c in mixed_text(), windows in any::<bool>()) {
            let host = if windows { PathFlavor::Windows } else { PathFlavor::Posix };
            let (sa, sb, sc) = (seq(&a, host), seq(&b, host), seq(&c, host));
            let ab = sa.cmp(&sb);
            prop_assert_eq!(ab == Ordering::Equal, sa == sb);
            prop_assert_eq!(ab, sb.cmp(&sa).reverse());
            if sa == sb {
                prop_assert_eq!(hash_seq(&a, host), hash_seq(&b, host));
            }
            let mut sorted = [sa, sb, sc];
            sorted.sort();
            prop_assert!(sorted[0] <= sorted[1] && sorted[1] <= sorted[2] && sorted[0] <= sorted[2]);
        }

        /// The real type, on this host: `Eq` holds exactly when `Ord` says `Equal`, and equal
        /// values hash alike.
        #[test]
        fn the_types_comparison_is_consistent_on_this_host(a in mixed_text(), b in mixed_text()) {
            let (pa, pb) = (
                AppPath::legacy_unchecked(std::path::Path::new(&a)),
                AppPath::legacy_unchecked(std::path::Path::new(&b)),
            );
            prop_assert_eq!(pa == pb, pa.cmp(&pb) == Ordering::Equal);
            if pa == pb {
                prop_assert_eq!(super::hash_of(&pa), super::hash_of(&pb));
            }
        }

        /// Under Windows' rules a POSIX-kind path reads with the app's grammar, which is
        /// POSIX's: a `\` is a name character there.
        #[test]
        fn a_posix_kind_path_reads_as_posix_under_windows_rules(tail in mixed_text()) {
            // Built POSIX-kind (one `/`, then a name), not filtered to it: a filter that rejects
            // most cases timed out on the 2-core Windows builder.
            let a = format!("/a{tail}");
            prop_assert!(super::posix_kind(&a));
            prop_assert_eq!(seq(&a, PathFlavor::Windows), seq(&a, PathFlavor::Posix));
        }
    }

    /// Review 2's N4 inputs, on every OS.
    #[test]
    fn a_backslash_in_an_in_memory_path_never_merges_with_a_slash() {
        let windows = PathFlavor::Windows;
        assert_ne!(seq(r"/d/a\b", windows), seq("/d/a/b", windows));
        assert_ne!(seq(r"/d\", windows), seq("/d", windows));
        assert_ne!(seq(r"/d/a\b", PathFlavor::Posix), seq("/d/a/b", PathFlavor::Posix));
        // What Win32 opens still compares as `PathBuf` does there.
        assert_eq!(seq(r"C:\x", windows), seq("C:/x", windows));
        assert_eq!(seq(r"C:\x\", windows), seq("C:/x", windows));
        assert_eq!(seq(r"\\srv\sh\x", windows), seq("//srv/sh/x", windows));
        assert_eq!(seq("/a/./b/", windows), seq("/a/b", windows));
        // On POSIX a sloppy `//` join is the path std opens.
        assert_eq!(seq("//Users", PathFlavor::Posix), seq("/Users", PathFlavor::Posix));
    }
}
