//! Text to app path: `parse` normalizes, `from_normalized` only validates.

use std::borrow::Cow;
use std::fmt;

use super::escape::{self, EscapeRefusal, Namer};
use super::shape::{is_scheme_url, is_unc};
use super::{AppPath, AppPathBuf, PathFlavor, PathKind};

/// Why a text is no app path. Carries no text: the input is someone's path, and the caller
/// that logs a refusal logs the input through `LogPrivate` beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAnAppPath {
    /// The empty text.
    Empty,
    /// No root this flavor knows: `foo/bar`, `~/x`, or `C:/x` under POSIX.
    Relative,
    /// A POSIX-shaped `/x` under the Windows flavor, where it would mean the CURRENT
    /// drive's root, or a drive or UNC path where only POSIX ones belong.
    WrongFlavor,
    /// `C:foo`: that drive's current directory, not a root.
    DriveRelative,
    /// A bare `C:` reported by the OS: also that drive's current directory to Win32. A
    /// person typing `C:` means `C:/`, which [`AppPathBuf::parse`] reads.
    BareDrive,
    /// A `\` in a drive or UNC path's text, where it's never a name character.
    Backslash,
    /// A UNC root with no server or share: `//server`, `//`.
    MissingShare,
    /// One of Win32's device namespaces: `\\.\…`, `\\?\Volume{…}`, `\\?\GLOBALROOT\…`, or
    /// a UNC server of `?` or `.`. None names a folder a pane can show.
    DevicePath,
    /// A `..` name. Resolving it by text can retarget a path across a symlink, so only a
    /// volume answers it (`..` typed by a person goes through the app's resolver first).
    DotName,
    /// An empty, `.`, or `..` name inside a verbatim `\\?\` path, which NTFS refuses.
    VerbatimName,
    /// A NUL anywhere but at the start of an MTP pick segment (NUL plus a decimal id).
    Nul,
    /// A name mixing byte escapes and unit escapes, which no namer makes.
    MixedEscape,
    /// Text [`AppPath::from_normalized`] was given that isn't in canonical form: a trailing
    /// `/`, a repeated `/`, a `.` name, a lowercase drive letter, or non-canonical escapes.
    NotCanonical,
}

impl fmt::Display for NotAnAppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NotAnAppPath::Empty => "an empty path",
            NotAnAppPath::Relative => "a path with no root",
            NotAnAppPath::WrongFlavor => "a path shaped for another OS",
            NotAnAppPath::DriveRelative => "a drive-relative path",
            NotAnAppPath::BareDrive => "a bare drive letter",
            NotAnAppPath::Backslash => "a backslash in a drive or UNC path",
            NotAnAppPath::MissingShare => "a UNC path with no server or share",
            NotAnAppPath::DevicePath => "a Windows device path",
            NotAnAppPath::DotName => "a `..` name",
            NotAnAppPath::VerbatimName => "an empty, `.`, or `..` name in a verbatim path",
            NotAnAppPath::Nul => "a NUL outside a pick segment",
            NotAnAppPath::MixedEscape => "a name mixing two escape alphabets",
            NotAnAppPath::NotCanonical => "a path not in canonical form",
        })
    }
}

impl std::error::Error for NotAnAppPath {}

impl From<EscapeRefusal> for NotAnAppPath {
    fn from(_: EscapeRefusal) -> Self {
        // Canonicalizing only meets `Mixed`: a name's escapes decode through their own alphabet.
        NotAnAppPath::MixedEscape
    }
}

/// Whether `name` holds a NUL only as an MTP pick segment does: the whole name is NUL plus
/// a decimal id (`twin_path::pick_segment`).
fn nul_ok(name: &str) -> bool {
    match name.strip_prefix('\0') {
        Some(id) => !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
        None => !name.contains('\0'),
    }
}

/// `name`, checked as one name of a `kind` path, in canonical form.
pub(super) fn checked_name(name: &str, kind: PathKind) -> Result<Cow<'_, str>, NotAnAppPath> {
    if !nul_ok(name) {
        return Err(NotAnAppPath::Nul);
    }
    if matches!(kind, PathKind::Drive | PathKind::Unc) && name.contains('\\') {
        return Err(NotAnAppPath::Backslash);
    }
    Ok(escape::canonical_name(name)?)
}

/// Appends `rest`'s names to `out` (which holds a root ending in `/`), normalized: empty and
/// `.` names dropped, `..` refused, each name canonical.
fn push_names(rest: &str, kind: PathKind, out: &mut String) -> Result<(), NotAnAppPath> {
    let root_len = out.len();
    for name in rest.split('/') {
        match name {
            "" | "." => continue,
            ".." => return Err(NotAnAppPath::DotName),
            _ => {}
        }
        let name = checked_name(name, kind)?;
        if out.len() > root_len {
            out.push('/');
        }
        out.push_str(&name);
    }
    Ok(())
}

/// The root of a scheme URL (`scheme://authority/`) and the text after it.
fn scheme_root(text: &str) -> Result<(&str, &str), NotAnAppPath> {
    let authority_at = text.find("://").expect("a scheme URL holds ://") + 3;
    let after = &text[authority_at..];
    let end = after.find('/').unwrap_or(after.len());
    let (root, rest) = text.split_at(authority_at + end);
    if root.contains('\0') {
        return Err(NotAnAppPath::Nul);
    }
    Ok((root, rest))
}

/// A Windows root at the start of `text`: the drive or UNC share, spelled canonically, and
/// the text after it.
fn windows_root(text: &str) -> Result<(String, &str), NotAnAppPath> {
    if text.contains('\\') {
        return Err(NotAnAppPath::Backslash);
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let rest = &text[2..];
        if !(rest.is_empty() || rest.starts_with('/')) {
            return Err(NotAnAppPath::DriveRelative);
        }
        let letter = char::from(bytes[0].to_ascii_uppercase());
        return Ok((format!("{letter}:/"), rest));
    }
    if is_unc(text) {
        let mut parts = text[2..].splitn(3, '/');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        if server == "?" || server == "." {
            return Err(NotAnAppPath::DevicePath);
        }
        if server.is_empty() || share.is_empty() {
            return Err(NotAnAppPath::MissingShare);
        }
        if matches!(share, "." | "..") {
            return Err(NotAnAppPath::DotName);
        }
        let server = checked_name(server, PathKind::Unc)?;
        let share = checked_name(share, PathKind::Unc)?;
        return Ok((format!("//{server}/{share}/"), rest));
    }
    if text.starts_with("//") {
        return Err(NotAnAppPath::MissingShare);
    }
    if text.starts_with('/') {
        return Err(NotAnAppPath::WrongFlavor);
    }
    Err(NotAnAppPath::Relative)
}

impl AppPathBuf {
    /// `text` as an app path in `flavor`, normalized: repeated `/` collapsed (except a UNC
    /// root's leading `//`), `.` names and a trailing `/` dropped, the drive letter
    /// uppercased, each name's escapes in canonical form. ❗ A `..` name is REFUSED, never
    /// resolved: by text it can name a different file across a symlink. Case and Unicode
    /// form are never touched.
    ///
    /// # Errors
    ///
    /// [`NotAnAppPath`], typed: empty, relative, wrong flavor, a `..` name, a misplaced NUL,
    /// mixed escapes, or a malformed Windows root.
    pub fn parse(text: &str, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
        if text.is_empty() {
            return Err(NotAnAppPath::Empty);
        }
        let mut out = String::with_capacity(text.len() + 1);
        let (kind, rest) = if is_scheme_url(text) {
            let (root, rest) = scheme_root(text)?;
            out.push_str(root);
            out.push('/');
            (PathKind::Scheme, rest)
        } else {
            match flavor {
                PathFlavor::Posix => {
                    let rest = text.strip_prefix('/').ok_or(NotAnAppPath::Relative)?;
                    out.push('/');
                    (PathKind::Posix, rest)
                }
                PathFlavor::Windows => {
                    let (root, rest) = windows_root(text)?;
                    out.push_str(&root);
                    let kind = if root.starts_with("//") {
                        PathKind::Unc
                    } else {
                        PathKind::Drive
                    };
                    (kind, rest)
                }
            }
        };
        push_names(rest, kind, &mut out)?;
        Ok(AppPathBuf(out))
    }

    /// An app path that came over IPC or out of a store: [`Self::parse`] with this host's
    /// flavor, so a POSIX `/x` never enters on Windows.
    pub fn from_wire(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
        Self::parse(text, PathFlavor::HOST)
    }

    /// Typed or pasted text: on Windows both `\` and `/` separate; `.` names drop; every
    /// escape-range code point is LITERAL text (escaped through its own units, the way the
    /// path's namer would store it), so a person can't type a live escape.
    ///
    /// # Errors
    ///
    /// As [`Self::parse`]. A `..` is refused here too: what it means is a volume's answer,
    /// which the app's typed-path resolver asks before it calls this on each resolved prefix.
    pub fn from_user_input(text: &str, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
        let separated: Cow<'_, str> = if flavor == PathFlavor::Windows && !is_scheme_url(text) {
            Cow::Owned(text.replace('\\', "/"))
        } else {
            Cow::Borrowed(text)
        };
        let namer = match flavor {
            PathFlavor::Windows if !is_scheme_url(&separated) => Namer::Units,
            _ => Namer::Bytes,
        };
        Self::parse(&escape::literal(&separated, namer), flavor)
    }
}

/// The text's kind and root length if `text` is canonical, else why not.
pub(super) fn validate_canonical(text: &str) -> Result<(), NotAnAppPath> {
    if text.is_empty() {
        return Err(NotAnAppPath::Empty);
    }
    let bytes = text.as_bytes();
    let (kind, rest) = if is_unc(text) {
        let mut parts = text[2..].splitn(3, '/');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        let rest = parts.next().ok_or(NotAnAppPath::NotCanonical)?;
        if server == "?" || server == "." {
            return Err(NotAnAppPath::DevicePath);
        }
        if share.is_empty() {
            return Err(NotAnAppPath::MissingShare);
        }
        if matches!(share, "." | "..") {
            return Err(NotAnAppPath::NotCanonical);
        }
        for part in [server, share] {
            if checked_name(part, PathKind::Unc)? != part {
                return Err(NotAnAppPath::NotCanonical);
            }
        }
        (PathKind::Unc, rest)
    } else if bytes.first() == Some(&b'/') {
        (PathKind::Posix, &text[1..])
    } else if is_scheme_url(text) {
        let (_, rest) = scheme_root(text)?;
        let rest = rest.strip_prefix('/').ok_or(NotAnAppPath::NotCanonical)?;
        (PathKind::Scheme, rest)
    } else if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        if !bytes[0].is_ascii_uppercase() {
            return Err(NotAnAppPath::NotCanonical);
        }
        match text[2..].strip_prefix('/') {
            Some(rest) => (PathKind::Drive, rest),
            None if text.len() == 2 => return Err(NotAnAppPath::NotCanonical),
            None => return Err(NotAnAppPath::DriveRelative),
        }
    } else {
        return Err(NotAnAppPath::Relative);
    };
    if rest.is_empty() {
        return Ok(());
    }
    for name in rest.split('/') {
        match name {
            "" | "." => return Err(NotAnAppPath::NotCanonical),
            ".." => return Err(NotAnAppPath::DotName),
            _ => {}
        }
        if checked_name(name, kind)? != name {
            return Err(NotAnAppPath::NotCanonical);
        }
    }
    Ok(())
}

impl AppPath {
    /// `text`, validated as already canonical and borrowed as is: the zero-copy door for a
    /// path a backend built (a `FileEntry.path`). Flavor-free, since canonical text says its
    /// own kind.
    ///
    /// # Errors
    ///
    /// [`NotAnAppPath`]: anything [`AppPathBuf::parse`] would refuse, and
    /// [`NotAnAppPath::NotCanonical`] for text `parse` would respell.
    pub fn from_normalized(text: &str) -> Result<&AppPath, NotAnAppPath> {
        validate_canonical(text)?;
        Ok(AppPath::new_unchecked(text))
    }
}
