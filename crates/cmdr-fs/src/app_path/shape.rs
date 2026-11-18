//! Where an app path's root ends, read off the text alone, and the components its `Eq`,
//! `Hash`, and `Ord` compare.
//!
//! Canonical text is self-rooted: its shape says its kind without a flavor (`/x` is POSIX,
//! `C:/x` a drive, `//server/share/x` a UNC share, `scheme://authority/x` a scheme path),
//! since under the POSIX flavor a leading `//` collapses to `/` and `C:/x` is no app path.

use super::PathKind;

/// Whether `text` starts with `<scheme>://`, the scheme being RFC 3986's shape and at least
/// two characters long. ❗ A one-letter "scheme" is a drive: `C://x` is `C:/x` with a doubled
/// separator on Windows (what `from_user_input` makes of a typed `C:\\x`), and no backend
/// mints a one-letter scheme.
pub fn is_scheme_url(text: &str) -> bool {
    let Some((scheme, _)) = text.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    scheme.len() >= 2
        && chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// `text`'s kind and the byte length of its root, the root's own trailing `/` included.
///
/// Total, so arithmetic on a bridge value (unvalidated, see `bridge.rs`) never panics: text
/// with no root it knows reads as POSIX with an empty root.
pub(super) fn root_of(text: &str) -> (PathKind, usize) {
    let bytes = text.as_bytes();
    if is_unc(text) {
        // `//server/share/…`: the root runs through the `/` after the share.
        let after = &text[2..];
        let server_end = after.find('/').map_or(after.len(), |i| i + 1);
        let share = &after[server_end..];
        let share_end = share.find('/').map_or(share.len(), |i| i + 1);
        return (PathKind::Unc, 2 + server_end + share_end);
    }
    if bytes.first() == Some(&b'/') {
        return (PathKind::Posix, 1);
    }
    if is_scheme_url(text) {
        let authority_at = text.find("://").expect("a scheme URL holds ://") + 3;
        let rest = &text[authority_at..];
        let end = rest.find('/').map_or(rest.len(), |i| i + 1);
        return (PathKind::Scheme, authority_at + end);
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes.get(2).is_none_or(|&b| b == b'/')
    {
        return (PathKind::Drive, bytes.len().min(3));
    }
    (PathKind::Posix, 0)
}

/// Whether `text` starts with exactly two `/` and then a server: a UNC root. POSIX gives a
/// leading `//` no special meaning, but a canonical POSIX path never has one, so the shape
/// is unambiguous for canonical text.
pub(super) fn is_unc(text: &str) -> bool {
    text.starts_with("//") && text.as_bytes().get(2).is_some_and(|&b| b != b'/')
}

/// One component, in the order `std::path::Component` sorts them (`RootDir` < `CurDir` <
/// `ParentDir` < `Normal`), so a POSIX app path orders exactly as its `PathBuf` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Component<'a> {
    RootDir,
    CurDir,
    ParentDir,
    /// A name, or a non-POSIX root as one token (`C:`, `//server/share`,
    /// `scheme://authority`), compared by bytes as `OsStr` compares them.
    Normal(&'a str),
}

/// The components `Eq`, `Hash`, and `Ord` compare: POSIX `Path::components` over the text
/// (empty and `.` names skipped, a leading `.` of a relative text kept, `..` kept), except
/// that a UNC or scheme root is ONE token. Without that, `//srv/share/x` would equal
/// `/srv/share/x`, and `sftp:///x` would equal `sftp://x/`: two canonical values naming
/// different places.
pub(super) fn components(text: &str) -> impl Iterator<Item = Component<'_>> {
    let (head, rest) = if is_unc(text) {
        let (_, root_len) = root_of(text);
        let token = text[..root_len].trim_end_matches('/');
        (Some(Component::Normal(token)), &text[root_len..])
    } else if text.starts_with('/') {
        (Some(Component::RootDir), text)
    } else if is_scheme_url(text) {
        let (_, root_len) = root_of(text);
        let token = text[..root_len].strip_suffix('/').unwrap_or(&text[..root_len]);
        (Some(Component::Normal(token)), &text[root_len..])
    } else if text == "." || text.starts_with("./") {
        (Some(Component::CurDir), &text[1..])
    } else {
        (None, text)
    };
    head.into_iter().chain(rest.split('/').filter_map(|name| match name {
        "" | "." => None,
        ".." => Some(Component::ParentDir),
        name => Some(Component::Normal(name)),
    }))
}
