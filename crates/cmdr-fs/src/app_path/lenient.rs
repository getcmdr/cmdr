//! Typed arithmetic on a transition-bridge value: text that isn't canonical (`/a/`, `/a//b`,
//! and on Windows `C:\x\y`), which `AppPath::legacy_unchecked` keeps as is.
//!
//! ❗ Release builds answer it RIGHT, never by reading the text as if it were canonical (the
//! second review's N6: `C:\x\y` would have had no parent and sat under no `C:\x`, so a
//! "destination inside source" guard said no). Two kinds of answer, both a function of the
//! same hybrid component reading `Eq` uses (`shape::components`):
//!
//! - Slices (`parent`, `file_name`, `names`, `root`, `kind`, `is_root`): what `Path` answers
//!   for the text on this host, cut from the text itself, so they borrow as the canonical
//!   ones do. `parent("/a//b/")` is `/a`, `parent(r"C:\x\y")` is `C:\x`.
//! - Answers that must be canonical to be typed (`join`, `strip_prefix`, the volume-root
//!   matches): read through [`canonical`], the canonical value naming the same place, or a
//!   typed refusal where there is none (a relative text, a device path, and a `..`, which by
//!   text can name a different file across a symlink, so it sits under nothing).

use super::parse;
use super::shape::{self, Component, Prefix};
use super::{AppPathBuf, NotAnAppPath, PathFlavor, PathKind};

/// The text's components with their byte ranges, and where its root ends.
pub(super) struct Read<'a> {
    spans: Vec<(Component<'a>, usize, usize)>,
    root_end: usize,
    scheme: bool,
}

pub(super) fn read(text: &str) -> Read<'_> {
    read_as(text, PathFlavor::HOST)
}

/// [`read`] as `host`'s comparison reads `text`, whatever this build's host is (so every OS
/// tests both).
pub(super) fn read_as(text: &str, host: PathFlavor) -> Read<'_> {
    let mut components = shape::components_for(text, host);
    let mut spans = Vec::new();
    while let Some(spanned) = components.next_spanned() {
        spans.push(spanned);
    }
    let scheme = shape::is_scheme_url(text);
    let root_end = if scheme {
        spans.first().map_or(0, |&(_, _, end)| end)
    } else {
        spans
            .iter()
            .take_while(|(component, ..)| matches!(component, Component::Prefix(_) | Component::RootDir))
            .last()
            .map_or(0, |&(_, _, end)| end)
    };
    Read {
        spans,
        root_end,
        scheme,
    }
}

impl<'a> Read<'a> {
    /// The components below the root, with their ranges.
    fn below_root(&self) -> impl DoubleEndedIterator<Item = &(Component<'a>, usize, usize)> + '_ {
        let skip = if self.scheme {
            1
        } else {
            self.spans
                .iter()
                .take_while(|(component, ..)| matches!(component, Component::Prefix(_) | Component::RootDir))
                .count()
        };
        self.spans.iter().skip(skip)
    }

    pub(super) fn root_end(&self) -> usize {
        self.root_end
    }

    pub(super) fn kind(&self) -> PathKind {
        if self.scheme {
            return PathKind::Scheme;
        }
        match self.spans.first() {
            Some((Component::Prefix(Prefix::Unc(..) | Prefix::VerbatimUnc(..)), ..)) => PathKind::Unc,
            Some((Component::Prefix(_), ..)) => PathKind::Drive,
            _ => PathKind::Posix,
        }
    }

    pub(super) fn is_root(&self) -> bool {
        self.below_root().next().is_none()
    }

    /// The names below the root, `..` and a verbatim `.` included as `Path` lists them.
    pub(super) fn names(&self, text: &'a str) -> Vec<&'a str> {
        self.below_root().map(|&(_, start, end)| &text[start..end]).collect()
    }

    /// `Path::parent`'s answer, as a slice: the text through the name before the last, or
    /// through the root; `None` at a root.
    pub(super) fn parent_end(&self) -> Option<usize> {
        let mut below = self.below_root().rev();
        below.next()?;
        Some(below.next().map_or(self.root_end, |&(_, _, end)| end))
    }

    /// `Path::file_name`'s answer: the last name, unless it's `..`.
    pub(super) fn file_name(&self) -> Option<&'a str> {
        match self.below_root().next_back()? {
            (Component::Normal(name), ..) => Some(name),
            _ => None,
        }
    }
}

/// The canonical app path naming the same place as `text`, or why none does.
///
/// # Errors
///
/// [`NotAnAppPath::Relative`] for text with no root, `DriveRelative` for `C:x`, `DevicePath`
/// for a device or bare verbatim prefix, `DotName` for a `..`, and anything
/// [`AppPathBuf::parse`] refuses in a name.
pub(super) fn canonical(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    canonical_as(text, PathFlavor::HOST)
}

/// [`canonical`] as `host` reads `text`.
pub(super) fn canonical_as(text: &str, host: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
    let read = read_as(text, host);
    let mut spans = read.spans.iter().map(|&(component, ..)| component).peekable();
    let (mut built, flavor) = if read.scheme {
        let Some(Component::Normal(token)) = spans.next() else {
            return Err(NotAnAppPath::Relative);
        };
        (format!("{token}/"), PathFlavor::Posix)
    } else {
        match spans.next() {
            Some(Component::RootDir) => ("/".to_string(), PathFlavor::Posix),
            Some(Component::Prefix(Prefix::Disk(letter) | Prefix::VerbatimDisk(letter))) => {
                if spans.next() != Some(Component::RootDir) {
                    return Err(NotAnAppPath::DriveRelative);
                }
                (format!("{}:/", char::from(letter)), PathFlavor::Windows)
            }
            Some(Component::Prefix(Prefix::Unc(server, share) | Prefix::VerbatimUnc(server, share))) => {
                if spans.peek() == Some(&Component::RootDir) {
                    spans.next();
                }
                (format!("//{server}/{share}/"), PathFlavor::Windows)
            }
            Some(Component::Prefix(Prefix::Verbatim(_) | Prefix::DeviceNs(_))) => return Err(NotAnAppPath::DevicePath),
            _ => return Err(NotAnAppPath::Relative),
        }
    };
    for component in spans {
        match component {
            Component::Normal(name) => {
                if !built.ends_with('/') {
                    built.push('/');
                }
                built.push_str(name);
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(NotAnAppPath::DotName),
            Component::RootDir | Component::Prefix(_) => return Err(NotAnAppPath::NotCanonical),
        }
    }
    AppPathBuf::parse(&built, flavor)
}

/// Whether `text` is in canonical form, the shape typed arithmetic reads directly.
pub(super) fn is_canonical(text: &str) -> bool {
    // ❗ Flavor-free, so on macOS and Linux a full `//server/share/x` text reads as UNC here
    // while `Eq` reads it as std does (`/server/share/x`): canonical UNC values exist there
    // only in tests (`parse(_, Windows)`, which Linux CI runs the Windows arm with), and the
    // text can't say which it is. No std join makes a bridge value of that shape. A
    // `format!("{dir}/{name}")` with `dir == "/"` makes `//name`: with one or two names
    // (`//Users/x`) it has no share and reads leniently, as std does; with three or more
    // (`//Users/x/Documents`) it reads as UNC. Every divergent answer is then a refusal or a
    // `None` (review 3's 1b, no producer found), never a different file.
    parse::validate_canonical_any(text).is_ok()
}
