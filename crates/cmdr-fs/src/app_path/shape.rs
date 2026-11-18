//! Where an app path's root ends, read off the text alone, and the components its `Eq`,
//! `Hash`, and `Ord` compare.
//!
//! Canonical text is self-rooted: its shape says its kind without a flavor (`/x` is POSIX,
//! `C:/x` a drive, `//server/share/x` a UNC share, `scheme://authority/x` a scheme path),
//! since under the POSIX flavor a leading `//` collapses to `/` and `C:/x` is no app path.

use super::PathKind;

/// Whether `text` starts with `<scheme>://`, the scheme being RFC 3986's shape and at least
/// two characters long. ❗ A one-letter "scheme" is a drive: `C://x` is `C:/x` with a doubled
/// separator on Windows (what `from_person_typed` makes of a typed `C:\\x`), and no backend
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

/// A Windows path prefix, in the order `std::path::Prefix` sorts them, so a Windows host's
/// app path orders and dedups as its `PathBuf` did. The drive letter is uppercased, as std
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Prefix<'a> {
    Verbatim(&'a str),
    VerbatimUnc(&'a str, &'a str),
    VerbatimDisk(u8),
    DeviceNs(&'a str),
    Unc(&'a str, &'a str),
    Disk(u8),
}

/// One component, in the order `std::path::Component` sorts them (`Prefix` < `RootDir` <
/// `CurDir` < `ParentDir` < `Normal`), so an app path orders exactly as its `PathBuf` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Component<'a> {
    Prefix(Prefix<'a>),
    RootDir,
    CurDir,
    ParentDir,
    /// A name, or a scheme root as one token (`scheme://authority`), compared by bytes as
    /// `OsStr` compares them.
    Normal(&'a str),
}

/// The components `Eq`, `Hash`, and `Ord` compare: `Path::components` over the text, with
/// THIS host's rules, so a set keyed on app paths dedups and sorts as one keyed on `PathBuf`
/// did on the same host (unnormalized bridge values included: `/a/`, `/a/./b`, and on
/// Windows `C:\x` beside `C:/x`). One deliberate difference: a scheme root is ONE token,
/// since as std components `sftp:///x` would equal `sftp://x/`.
///
/// On macOS and Linux a leading `//` is a plain root, as std reads it, so a UNC value (made
/// only by `parse(_, Windows)`, test-only there) compares as the POSIX path with its text.
pub(super) fn components(text: &str) -> Components<'_> {
    components_for(text, super::PathFlavor::HOST)
}

/// [`components`] read with `flavor`'s rules, whatever the host.
pub(super) fn components_for(text: &str, flavor: super::PathFlavor) -> Components<'_> {
    let windows = flavor == super::PathFlavor::Windows;
    if is_scheme_url(text) {
        let (_, root_len) = root_of(text);
        let token = text[..root_len].strip_suffix('/').unwrap_or(&text[..root_len]);
        return Components {
            head: [Some(Component::Normal(token)), None],
            rest: &text[root_len..],
            windows: false,
            verbatim: false,
        };
    }
    let (prefix, after, verbatim) = if windows {
        windows_prefix(text)
    } else {
        (None, text, false)
    };
    let is_sep = |b: u8| b == b'/' || (windows && b == b'\\');
    let after_bytes = after.as_bytes();
    let physical_root = match after_bytes.first() {
        Some(&b) if verbatim => b == b'\\',
        Some(&b) => is_sep(b),
        None => false,
    };
    // As std: a UNC or device prefix implies a root, a drive or verbatim one doesn't.
    let implied_root = matches!(prefix, Some(Prefix::Unc(..) | Prefix::DeviceNs(_)));
    let rooted = physical_root || implied_root;
    let second = if rooted {
        Some(Component::RootDir)
    } else if prefix.is_none()
        && (after == "." || (after.len() > 1 && after.as_bytes()[0] == b'.' && is_sep(after.as_bytes()[1])))
    {
        Some(Component::CurDir)
    } else {
        None
    };
    let rest = if second == Some(Component::CurDir) {
        &after[1..]
    } else {
        after
    };
    Components {
        head: [prefix.map(Component::Prefix), second],
        rest,
        windows,
        verbatim,
    }
}

/// The prefix at the start of `text` as a Windows path, the text after it, and whether it's
/// a verbatim one (where only `\` separates, and `.` is a name). Mirrors std's
/// `parse_prefix` (Rust 1.100): a leading `\\` takes either separator, a verbatim `\\?\` only
/// backslashes, `UNC` is matched exactly, and a UNC prefix needs both a server and a share.
fn windows_prefix(text: &str) -> (Option<Prefix<'_>>, &str, bool) {
    let is_sep = |b: u8| b == b'/' || b == b'\\';
    let bytes = text.as_bytes();
    // The component up to the next separator (only `\` when verbatim), and the text after
    // that separator.
    let next = |s: &'_ str, verbatim: bool| -> (usize, usize) {
        let end = s
            .bytes()
            .position(|b| b == b'\\' || (!verbatim && b == b'/'))
            .unwrap_or(s.len());
        (end, (end + 1).min(s.len()))
    };
    if bytes.len() >= 2 && is_sep(bytes[0]) && is_sep(bytes[1]) {
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            let unc = rest.get(..4).is_some_and(|head| head == r"UNC\" || head == "UNC/");
            if unc {
                let after = &rest[4..];
                let (server_end, share_at) = next(after, true);
                let share_and_rest = &after[share_at..];
                let (share_end, _) = next(share_and_rest, true);
                let server = &after[..server_end];
                let consumed = if share_at > server_end {
                    share_at + share_end
                } else {
                    server_end
                };
                return (
                    Some(Prefix::VerbatimUnc(
                        server,
                        &share_and_rest[..share_end.min(share_and_rest.len())],
                    )),
                    &after[consumed.min(after.len())..],
                    true,
                );
            }
            let rb = rest.as_bytes();
            if rb.len() >= 2 && rb[0].is_ascii_alphabetic() && rb[1] == b':' && rb.get(2).is_none_or(|&b| b == b'\\') {
                return (Some(Prefix::VerbatimDisk(rb[0].to_ascii_uppercase())), &rest[2..], true);
            }
            let (end, _) = next(rest, true);
            return (Some(Prefix::Verbatim(&rest[..end])), &rest[end..], true);
        }
        if bytes.len() >= 4 && bytes[2] == b'.' && is_sep(bytes[3]) {
            let rest = &text[4..];
            let (end, _) = next(rest, false);
            return (Some(Prefix::DeviceNs(&rest[..end])), &rest[end..], false);
        }
        let after = &text[2..];
        let (server_end, share_at) = next(after, false);
        let share_and_rest = &after[share_at..];
        let (share_end, _) = next(share_and_rest, false);
        if server_end > 0 && share_at > server_end && share_end > 0 {
            return (
                Some(Prefix::Unc(&after[..server_end], &share_and_rest[..share_end])),
                &share_and_rest[share_end..],
                false,
            );
        }
        return (None, text, false);
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return (Some(Prefix::Disk(bytes[0].to_ascii_uppercase())), &text[2..], false);
    }
    (None, text, false)
}

/// The iterator [`components`] answers: no allocation, so `Eq`, `Hash`, and `Ord` stay free.
pub(super) struct Components<'a> {
    head: [Option<Component<'a>>; 2],
    rest: &'a str,
    windows: bool,
    verbatim: bool,
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Component<'a>> {
        for slot in &mut self.head {
            if let Some(component) = slot.take() {
                return Some(component);
            }
        }
        loop {
            if self.rest.is_empty() {
                return None;
            }
            let (windows, verbatim) = (self.windows, self.verbatim);
            let is_sep = |c: char| c == '\\' && windows || c == '/' && !verbatim;
            let end = self.rest.find(is_sep).unwrap_or(self.rest.len());
            let name = &self.rest[..end];
            self.rest = self.rest.get(end + 1..).unwrap_or("");
            match name {
                "" => {}
                "." if verbatim => return Some(Component::CurDir),
                "." => {}
                ".." => return Some(Component::ParentDir),
                name => return Some(Component::Normal(name)),
            }
        }
    }
}
