//! The app-path grammar as TEXT: the string helpers the frontend's twin mirrors, answering
//! exactly what the slash arithmetic did before the typed app path existed.
//!
//! An app path is **a scheme URL, or a `/`-separated absolute path whose first component
//! is the root**. The roots, by the local backend's [`PathFlavor`]:
//!
//! - [`PathFlavor::Posix`] (macOS, Linux): `/`.
//! - [`PathFlavor::Windows`]: a drive, `C:/` (the letter always uppercase), or a UNC
//!   share, `//server/share/` (server and share are ONE root component). Each is its own
//!   volume. A bare `/x` is no app path there, and neither is anything holding a `\`.
//!
//! It extends the rule [`super::remote_paths`] sets for remote volumes, that an app path
//! names its own volume. A scheme URL's root is its volume's business, so these helpers only
//! do slash arithmetic on one.
//!
//! ❗ These helpers resolve `.` and `..` LEXICALLY, clamped at the root, as they always
//! have; the shared case table pins it. The typed path, [`mod@crate::app_path`], refuses a `..`
//! instead (by text it can name a different file across a symlink), and new code speaks the
//! typed path. Each helper here keeps its answer until its callers convert.
//!
//! The frontend holds the same grammar (`apps/desktop/src/lib/path/grammar.ts`), and both
//! suites run one case table, `path_grammar_cases.json` beside this module, whose `escapes`
//! rows the typed path answers too. Case sensitivity is never the grammar's: it's a
//! per-volume property (the app's `LocationInfo.case_sensitive`).

pub use crate::app_path::{PathFlavor, PathKind};

/// Whether `text` starts with `<scheme>://`, the scheme being RFC 3986's shape. The
/// frontend twin's `isSchemeUrl`; the typed path's own reads a one-letter scheme as a drive
/// (`crate::app_path::is_scheme_url`).
pub fn is_scheme_url(text: &str) -> bool {
    let Some((scheme, _)) = text.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Win32's long-path prefix, and its UNC form.
const VERBATIM: &str = r"\\?\";
const VERBATIM_UNC: &str = r"\\?\UNC\";

/// A local app path read lexically: its root's wire spelling (`/`, `C:/`,
/// `//server/share/`) and the names under it, `.` and `..` resolved and clamped at the root.
struct Lexical<'a> {
    kind: PathKind,
    root: String,
    names: Vec<&'a str>,
}

impl Lexical<'_> {
    fn text(&self) -> String {
        format!("{}{}", self.root, self.names.join("/"))
    }
}

/// `text` read lexically, or `None` for a scheme URL and for text that is no local app path
/// in `flavor`: relative, `~`-rooted, empty, or malformed (`//server`, `C:x`, anything with a
/// `\` on Windows).
fn lexical(text: &str, flavor: PathFlavor) -> Option<Lexical<'_>> {
    if is_scheme_url(text) {
        return None;
    }
    let (kind, root, rest) = match flavor {
        PathFlavor::Posix => (PathKind::Posix, "/".to_string(), text.strip_prefix('/')?),
        PathFlavor::Windows => {
            if text.contains('\\') {
                return None;
            }
            let bytes = text.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                let rest = &text[2..];
                if !(rest.is_empty() || rest.starts_with('/')) {
                    return None;
                }
                let letter = char::from(bytes[0].to_ascii_uppercase());
                (PathKind::Drive, format!("{letter}:/"), rest)
            } else {
                let after = text.strip_prefix("//")?;
                let mut parts = after.splitn(3, '/');
                let server = parts.next().filter(|s| !s.is_empty() && *s != "?" && *s != ".")?;
                let share = parts.next().filter(|s| !s.is_empty())?;
                (
                    PathKind::Unc,
                    format!("//{server}/{share}/"),
                    parts.next().unwrap_or(""),
                )
            }
        }
    };
    let mut names = Vec::new();
    for name in rest.split('/') {
        match name {
            "" | "." => {}
            ".." => {
                names.pop();
            }
            _ => names.push(name),
        }
    }
    Some(Lexical { kind, root, names })
}

/// `text` as a local app path in `flavor`, `.` and `..` resolved lexically and clamped at the
/// root, or why it's no local app path. The typed path's `from_native` uses it for a PLAIN
/// Windows path, where lexical is exactly what Win32 does.
pub fn normalize_local(text: &str, flavor: PathFlavor) -> Option<String> {
    lexical(text, flavor).map(|path| path.text())
}

/// The shape of `text`'s root, or `None` when it's no app path in `flavor`.
pub fn path_kind(text: &str, flavor: PathFlavor) -> Option<PathKind> {
    if is_scheme_url(text) {
        return Some(PathKind::Scheme);
    }
    lexical(text, flavor).map(|path| path.kind)
}

/// Whether `text` is a local app path (`/…`, `C:/…`, `//server/share/…`) in `flavor`.
pub fn is_local_path(text: &str, flavor: PathFlavor) -> bool {
    lexical(text, flavor).is_some()
}

/// The root of the local app path `text` (`/`, `C:/`, `//server/share/`), or `None` for no
/// local app path.
pub fn root_of(text: &str, flavor: PathFlavor) -> Option<String> {
    lexical(text, flavor).map(|path| path.root)
}

/// The names under the root of the local app path `text`, resolved lexically.
pub fn names_of(text: &str, flavor: PathFlavor) -> Option<Vec<String>> {
    lexical(text, flavor).map(|path| path.names.iter().map(|name| (*name).to_string()).collect())
}

/// The one spelling of an app path: a local one normalized, a scheme URL as is, `None` for
/// no app path.
pub fn normalize_path(text: &str, flavor: PathFlavor) -> Option<String> {
    if is_scheme_url(text) {
        return Some(text.to_string());
    }
    normalize_local(text, flavor)
}

/// `relative` (one name or several, `/`-separated) inside the local app path `base`, `.` and
/// `..` resolved lexically and clamped at the root: never off the volume.
pub fn join_lexically(base: &str, relative: &str, flavor: PathFlavor) -> Option<String> {
    let mut text = lexical(base, flavor)?.text();
    if !text.ends_with('/') {
        text.push('/');
    }
    text.push_str(relative.trim_start_matches('/'));
    normalize_local(&text, flavor)
}

/// The folder holding `text`, normalized, or `None` at a root and for no app path. A scheme
/// URL is plain slash arithmetic, as the frontend's twin does it.
pub fn parent_in_volume(text: &str, flavor: PathFlavor) -> Option<String> {
    if is_scheme_url(text) {
        return Some(text[..text.rfind('/').unwrap_or(0)].to_string());
    }
    let mut path = lexical(text, flavor)?;
    path.names.pop()?;
    Some(path.text())
}

/// The last name in `text` (`""` at a root), or `None` for no app path.
pub fn basename_in_volume(text: &str, flavor: PathFlavor) -> Option<String> {
    if is_scheme_url(text) {
        return Some(text[text.rfind('/').map_or(0, |i| i + 1)..].to_string());
    }
    let path = lexical(text, flavor)?;
    Some(path.names.last().copied().unwrap_or("").to_string())
}

/// The app path for a path the local OS reported, as text: on Windows the long-path forms
/// and the plain ones (`C:\x`, `\\server\share\x`, either separator) all land on one
/// spelling. `None` for anything that isn't absolute, and for Win32's other device paths
/// (`\\.\PhysicalDrive0`, `\\?\Volume{…}`).
pub fn native_to_app(native: &str, flavor: PathFlavor) -> Option<String> {
    match flavor {
        PathFlavor::Posix => normalize_local(native, flavor),
        PathFlavor::Windows => {
            let slashed = if let Some(unc) = native.strip_prefix(VERBATIM_UNC) {
                format!("//{}", unc.replace('\\', "/"))
            } else if let Some(local) = native.strip_prefix(VERBATIM) {
                local.replace('\\', "/")
            } else {
                native.replace('\\', "/")
            };
            normalize_local(&slashed, flavor)
        }
    }
}

/// The `\\?\` path Win32 opens for the Windows app path `text`, resolved lexically, with NO
/// refusals: the local Windows volume's crossing until it takes the typed
/// `AppPath::to_native`, which refuses a `:` (an alternate data stream) and the rest typed.
/// `None` for no Windows app path.
pub fn verbatim_lexically(text: &str) -> Option<String> {
    let path = lexical(text, PathFlavor::Windows)?;
    let names = path.names.join("\\");
    Some(match path.kind {
        PathKind::Unc => {
            let server_share = path
                .root
                .trim_start_matches('/')
                .trim_end_matches('/')
                .replace('/', "\\");
            format!("{VERBATIM_UNC}{server_share}\\{names}")
        }
        _ => format!("{VERBATIM}{}\\{names}", path.root.trim_end_matches('/')),
    })
}

/// The app-path spelling of a path the local OS reported (`C:\Users\test_user_1` becomes
/// `C:/Users/test_user_1` on Windows; macOS and Linux keep it), the one form a pane, a
/// favorite, and a switcher row hold. Text the grammar can't read keeps its native spelling.
pub fn app_spelling(native: &str, flavor: PathFlavor) -> String {
    native_to_app(native, flavor).unwrap_or_else(|| native.to_string())
}

/// What a person reads for `text`: a Windows local path with `\`, every other text as is.
pub fn shown_path(text: &str, flavor: PathFlavor) -> String {
    if flavor == PathFlavor::Posix {
        return text.to_string();
    }
    match lexical(text, flavor) {
        Some(path) if path.kind == PathKind::Unc && path.names.is_empty() => {
            path.root.trim_end_matches('/').replace('/', "\\")
        }
        Some(path) => path.text().replace('/', "\\"),
        None => text.to_string(),
    }
}

/// What typed or pasted text becomes on the wire. On Windows both `\` and `/` separate and a
/// local path comes out normalized; text that isn't one (relative, `~`) only has its
/// separators turned. On macOS and Linux the text is untouched: `\` is a legal name character.
pub fn from_user_input(text: &str, flavor: PathFlavor) -> String {
    if flavor == PathFlavor::Posix || is_scheme_url(text) {
        return text.to_string();
    }
    let slashed = text.replace('\\', "/");
    normalize_path(&slashed, flavor).unwrap_or(slashed)
}

#[cfg(test)]
#[path = "../path_grammar_test.rs"]
mod path_grammar_test;
