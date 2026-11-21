//! Text a person or an agent typed or pasted, turned into an app path, with every
//! `..` answered as the volume holding the path answers it.
//!
//! The rule (`docs/specs/app-path-plan.md` § "Typed and pasted `..`"): `..` means what
//! the volume means. A relative input's LEADING `..`s step up from the base folder by
//! text, the way the pane's own `..` row and a shell's `cd ..` do. Every other `..`
//! (in an absolute input, or after a name) asks the volume, through
//! [`TypedPathHost::parent_for_dot_dot`] (the app's host picks the volume and calls
//! `Volume::parent_for_dot_dot`), so `/x/link/../y` on a POSIX disk lands beside the
//! link's TARGET, where `ls` would. Every other part stays as typed: only a `..` that
//! crosses a link changes the text, and [`TypedPath::crossed_link`] says so.
//!
//! A `..` above a root clamps there for a person and is refused for an agent
//! ([`TypedPathRefusal::OffRoot`]). A volume that can't answer (a hung mount, a dead
//! connection) refuses, ❌ never a lexical guess.
//!
//! The app's `go_to_path::resolve_typed` is the host over the volume registry; the
//! browser demo's go-to is the host over its in-memory volume.

use std::fmt;
use std::future::Future;

use crate::app_path::{
    AppNameBuf, AppPath, AppPathBuf, Namer, NotAnAppPath, PathArithmetic, PathFlavor, explorer_trimmed, is_scheme_url,
};
use crate::volume::VolumeError;
use crate::volume::dot_dot::lexical_parent;

/// Who typed the text, which decides how it's read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedBy {
    /// A person in the address bar or a go-to dialog: `~` expands, and on Windows each
    /// name's trailing dots and spaces go, as Explorer's address bar reads them.
    Person,
    /// A person through a machine: a CLI argument, a deep link. The shell already
    /// expanded `~`, and every name is literal.
    PersonViaCommandLine,
    /// An agent (an MCP tool's argument): `~` expands, every name is literal, and a `..`
    /// above a root is refused rather than clamped.
    Agent,
}

impl TypedBy {
    fn expands_tilde(self) -> bool {
        !matches!(self, Self::PersonViaCommandLine)
    }

    fn trims_like_explorer(self) -> bool {
        matches!(self, Self::Person)
    }

    fn clamps_above_root(self) -> bool {
        !matches!(self, Self::Agent)
    }
}

/// A resolved typed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedPath {
    /// Where the text leads, as an app path.
    pub path: AppPathBuf,
    /// Whether some `..` crossed a link, so `path` differs from what the text reads as
    /// by itself: what go-to's preview line shows.
    pub crossed_link: bool,
}

/// Why typed text leads nowhere. Typed, so an agent's tool error and a log line name
/// the reason; ❌ never classified by its `Display`.
#[derive(Debug, Clone)]
pub enum TypedPathRefusal {
    /// Nothing typed.
    Empty,
    /// A relative path with no folder to read it from.
    NoBase,
    /// Text no app path can be made of (a drive-relative `C:foo`, a device path, a NUL).
    NotAPath(NotAnAppPath),
    /// A name no app path can hold.
    NotAName,
    /// A `..` above a root, from an agent.
    OffRoot,
    /// The volume couldn't say what a `..` means there (a hung mount, a dead
    /// connection, a link loop, a refusal).
    Unanswered(VolumeError),
}

impl fmt::Display for TypedPathRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("nothing typed"),
            Self::NoBase => f.write_str("a relative path with no folder to start from"),
            Self::NotAPath(why) => write!(f, "not a path: {why}"),
            Self::NotAName => f.write_str("a name no path can hold"),
            Self::OffRoot => f.write_str("a `..` above the root"),
            Self::Unanswered(err) => write!(f, "the volume couldn't answer a `..`: {err}"),
        }
    }
}

impl std::error::Error for TypedPathRefusal {}

/// What a resolver host answers.
pub trait TypedPathHost {
    /// The flavor typed text reads with (this host's).
    fn flavor(&self) -> PathFlavor;

    /// `text` with a leading `~` expanded to the home folder; any other text unchanged.
    fn expand_tilde(&self, text: &str) -> String;

    /// What `<path>/..` names: the volume holding `path` answers
    /// (`Volume::parent_for_dot_dot`), within the host's timeout. ❌ Never a lexical
    /// guess when the volume can't answer.
    fn parent_for_dot_dot(&self, path: &AppPath) -> impl Future<Output = Result<AppPathBuf, VolumeError>>;
}

/// Resolves `text`, typed `by` someone, against `base` (the pane's folder, for a relative
/// input).
///
/// # Errors
///
/// [`TypedPathRefusal`]: nothing typed, a relative path with no base, text no app path is
/// made of, an agent's `..` above a root, or a volume that couldn't answer a `..`.
pub async fn resolve_typed_with(
    text: &str,
    base: Option<&AppPath>,
    by: TypedBy,
    host: &impl TypedPathHost,
) -> Result<TypedPath, TypedPathRefusal> {
    if text.is_empty() {
        return Err(TypedPathRefusal::Empty);
    }
    let flavor = host.flavor();
    let expanded = if by.expands_tilde() {
        host.expand_tilde(text)
    } else {
        text.to_string()
    };
    let text = separated(&expanded, flavor, by)?;
    let namer = namer_of(&text);

    // The head is everything before the first `..` name: parsed whole, so the root's own
    // grammar (a drive, a UNC share, a scheme authority) is the parser's. The rest walks.
    let head_end = first_dot_dot(&text).unwrap_or(text.len());
    let (mut at, rest, mut leading) = match AppPathBuf::from_external_text(&text[..head_end], flavor, namer) {
        Ok(head) => (head, &text[head_end..], false),
        Err(NotAnAppPath::Relative | NotAnAppPath::Empty) => {
            let base = base.ok_or(TypedPathRefusal::NoBase)?;
            (base.to_owned(), text.as_str(), true)
        }
        Err(why) => return Err(TypedPathRefusal::NotAPath(why)),
    };

    let mut crossed_link = false;
    for name in rest.split('/') {
        match name {
            "" | "." => {}
            ".." if at.is_root() => {
                if !by.clamps_above_root() {
                    return Err(TypedPathRefusal::OffRoot);
                }
            }
            // "Up from where I am", by text, as the pane's `..` row goes.
            ".." if leading => at = lexical_parent(&at),
            ".." => {
                let asked = host
                    .parent_for_dot_dot(&at)
                    .await
                    .map_err(TypedPathRefusal::Unanswered)?;
                crossed_link |= asked != lexical_parent(&at);
                at = asked;
            }
            name => {
                leading = false;
                let name = AppNameBuf::from_typed(name, namer).map_err(|_| TypedPathRefusal::NotAName)?;
                at = at.join(name.as_app_name()).map_err(TypedPathRefusal::NotAPath)?;
            }
        }
    }
    Ok(TypedPath { path: at, crossed_link })
}

/// `text` with `/` as its only separator: on Windows `\` separates too (except in a scheme
/// path, where it's a name character), and a person's names lose their trailing dots and
/// spaces as Explorer's address bar drops them.
fn separated(text: &str, flavor: PathFlavor, by: TypedBy) -> Result<String, TypedPathRefusal> {
    if flavor != PathFlavor::Windows || is_scheme_url(text) {
        return Ok(text.to_string());
    }
    let slashed = text.replace('\\', "/");
    if by.trims_like_explorer() {
        return explorer_trimmed(&slashed).map_err(TypedPathRefusal::NotAPath);
    }
    Ok(slashed)
}

/// The byte offset of the first `..` name in `/`-separated text.
fn first_dot_dot(text: &str) -> Option<usize> {
    let mut offset = 0;
    for name in text.split('/') {
        if name == ".." {
            return Some(offset);
        }
        offset += name.len() + 1;
    }
    None
}

/// How the volume `text` names spells its names, which picks the escape alphabet a typed
/// escape-range code point is escaped through: UTF-16 for SMB and MTP, bytes for the
/// other servers, the host's own for a local path.
fn namer_of(text: &str) -> Namer {
    if !is_scheme_url(text) {
        return Namer::HOST;
    }
    match text.split_once("://").map(|(scheme, _)| scheme) {
        Some("smb" | "mtp") => Namer::Units,
        _ => Namer::Bytes,
    }
}

#[cfg(test)]
#[path = "typed_path_test.rs"]
mod tests;
