use super::*;
use std::collections::HashSet;

#[test]
fn every_marker_is_a_distinct_reserved_char_outside_the_reader_scratch() {
    let mut seen = HashSet::new();
    for c in LogKind::ALL.iter().map(|kind| kind.opener()).chain([CLOSER]) {
        assert!(is_reserved(c), "{c:?}");
        assert!(!READER_SCRATCH.contains(&c), "{c:?}");
        assert!(seen.insert(c), "{c:?} used twice");
    }
    assert!(READER_SCRATCH.start() >= RESERVED.start() && READER_SCRATCH.end() <= RESERVED.end());
}

#[test]
fn from_opener_inverts_opener_and_rejects_everything_else() {
    for kind in LogKind::ALL {
        assert_eq!(LogKind::from_opener(kind.opener()), Some(kind));
    }
    for c in [CLOSER, *READER_SCRATCH.start(), *RESERVED.end(), 'a', '\u{FFFD}'] {
        assert_eq!(LogKind::from_opener(c), None, "{c:?}");
    }
}

/// The reader recognizes a marker that went through `{:?}` by its escaped spelling, which
/// is Rust's, in lowercase hex (verified on rustc 1.99.0, 2026-10-19).
#[test]
fn debug_formatting_escapes_every_reserved_char_in_lowercase_hex() {
    for c in RESERVED {
        let escaped = format!("{:?}", c.to_string());
        assert_eq!(escaped, format!("\"\\u{{{:x}}}\"", c as u32), "{c:?}");
    }
}

/// Names that try every way out of a span: each reserved char, literal escaped-marker text at
/// two depths, a quote, a backslash (last, too), control chars, a bidi override, an NFD accent,
/// and token-looking text. Synthetic shapes, no real names.
fn hostile_values() -> Vec<String> {
    let mut values: Vec<String> = RESERVED.map(|c| format!("Summer{c}Lake House")).collect();
    values.extend(
        [
            "Summer (Lake House) 2026",
            "evil\\u{fdd1}rest",
            "evil\\\\u{fdd1}rest",
            "a \"quoted\" name",
            "ends in a backslash\\",
            "two\nlines\rand\ttab\u{7}",
            "abc\u{202E}fdp.exe",
            "Cafe\u{301}",
            "<dir:abc>",
            "path: /x, y; z key=v -> w",
            "",
        ]
        .map(str::to_string),
    );
    values
}

/// What `<str as Debug>` writes between its quotes.
fn str_debug_body(value: &str) -> String {
    let quoted = format!("{value:?}");
    quoted[1..quoted.len() - 1].to_string()
}

#[test]
fn display_writes_the_opener_the_debug_body_and_the_closer() {
    for kind in LogKind::ALL {
        for value in hostile_values() {
            let shown = LogPrivate::new(kind, value.as_str()).to_string();
            assert_eq!(
                shown,
                format!("{}{}{CLOSER}", kind.opener(), str_debug_body(&value)),
                "{kind:?} {value:?}"
            );
        }
    }
}

#[test]
fn debug_writes_the_same_inside_quotes() {
    let value = "Photos/Summer (Lake House)/a \"b\".jpg";
    assert_eq!(
        format!("path={:?}", LogPrivate::file_path(value)),
        format!("path=\"{}\"", LogPrivate::file_path(value))
    );
}

#[test]
fn every_constructor_marks_its_own_kind() {
    let v = "x";
    let cases = [
        (LogPrivate::path(v), LogKind::Path),
        (LogPrivate::file_path(v), LogKind::FilePath),
        (LogPrivate::dir_path(v), LogKind::DirPath),
        (LogPrivate::name(v), LogKind::Name),
        (LogPrivate::file_name(v), LogKind::FileName),
        (LogPrivate::dir_name(v), LogKind::DirName),
        (LogPrivate::volume(v), LogKind::Volume),
        (LogPrivate::host(v), LogKind::Host),
        (LogPrivate::user(v), LogKind::User),
    ];
    for (wrapped, kind) in cases {
        assert_eq!(wrapped.kind(), kind);
        assert!(wrapped.to_string().starts_with(kind.opener()), "{kind:?}");
    }
}

/// A hostile name holds no raw marker inside its span and can't split the line: the only raw
/// reserved chars are the wrapper's own two, so the lexer reads exactly one pair at depth 0, and
/// any reserved char the name held sits deeper, where it can't close anything.
#[test]
fn a_hostile_value_never_adds_a_marker_or_a_line() {
    for value in hostile_values() {
        let shown = LogPrivate::dir_path(value.as_str()).to_string();
        assert_eq!(shown.chars().filter(|&c| is_reserved(c)).count(), 2, "{value:?}");
        assert!(!shown.contains(['\n', '\r', '\u{202E}']), "{shown:?}");
        let all = markers(&shown);
        let found: Vec<_> = all.into_iter().filter(|m| m.depth == 0).collect();
        assert_eq!(found.len(), 2, "{value:?}: {found:?}");
        assert_eq!(found[0].role, MarkerRole::Open(LogKind::DirPath));
        assert_eq!(found[1].role, MarkerRole::Close);
        assert_eq!(found[1].end, shown.len());
    }
}

/// Every `{:?}` the marked text goes through moves its markers one depth down, never apart.
#[test]
fn each_debug_layer_moves_the_span_one_depth_down() {
    for value in hostile_values() {
        let mut text = format!("listed {} in 3ms", LogPrivate::file_path(value.as_str()));
        for depth in 0..4 {
            let all = markers(&text);
            assert!(all.iter().all(|m| m.depth >= depth), "{value:?}: {all:?}");
            let own = all.iter().filter(|m| m.depth == depth).count();
            assert_eq!(own, 2, "depth {depth}, {value:?}: {text}");
            text = format!("{text:?}");
        }
    }
}

#[cfg(unix)]
#[test]
fn a_non_utf8_value_prints_the_way_path_display_does() {
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    let path = Path::new(OsStr::from_bytes(b"/Volumes/Backup/caf\xe9 dir"));
    assert_eq!(
        LogPrivate::dir_path(path).to_string(),
        format!(
            "{}{}{CLOSER}",
            LogKind::DirPath.opener(),
            str_debug_body(&path.display().to_string())
        )
    );
}

/// The body is per char, so a writer can escape a stream piece by piece.
#[test]
fn the_body_of_a_concatenation_is_the_concatenation_of_the_bodies() {
    let values = hostile_values();
    let whole: String = values.concat();
    let mut pieces = String::new();
    for value in &values {
        write_debug_body(&mut pieces, value).expect("writing to a String");
    }
    assert_eq!(pieces, str_debug_body(&whole));
    let mut escaped = String::new();
    write!(DebugEscaping(&mut escaped), "{}", values.join("|")).expect("writing to a String");
    assert_eq!(escaped, str_debug_body(&values.join("|")));
}
