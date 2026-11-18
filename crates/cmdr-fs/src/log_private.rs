//! Mark the user-derived values in a log line, so an error report hashes exactly them.
//!
//! A log site that knows a value is a path, a name, a volume label, a host, or an account name
//! prints it through [`LogPrivate`] (`path={:?}` with `LogPrivate::dir_path(&path)`, or `{}` in
//! prose). That wraps the value between its kind's opener and [`CLOSER`], so the error reporter
//! hashes exactly that span instead of guessing where a name ends. The markers are Unicode
//! noncharacters from [`RESERVED`]: invisible in most viewers, never typed by a person, and still
//! valid scalar values, so a real filename CAN hold one. The reader of a marked line (the app's
//! `redact` module) treats every char of the range as structure and fails closed on anything it
//! can't pair. The table here and [`markers`] are the single source of truth both sides read;
//! nothing else lists the code points.
//!
//! A marked string is log text, never data: ❌ never turn a [`LogPrivate`] into a `String`, a
//! path, a stored value, or UI copy.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt::{self, Write as _};
use std::ops::RangeInclusive;

/// A user-derived value in a log line, marked with what it is.
///
/// `Display` writes the kind's opener, the value as the body of its `{:?}` (what `<str as Debug>`
/// writes between its quotes), and [`CLOSER`]; `Debug` writes the same inside quotes, so a
/// `path={:?}` site keeps the keyed-field shape every reader knows. The body escaping keeps a
/// hostile name from closing its own span, splitting the line, or reordering a viewer's display:
/// quotes, backslashes, control, bidi-format, and reserved chars inside the value all become
/// escape text. Locally a value reads as itself again: the terminal and Help > View debug log
/// decode each span. A non-UTF-8 value prints lossily, the way `Path::display` does.
///
/// It borrows and writes straight into the `Formatter`, so it costs nothing on a record below the
/// active level and allocates nothing on a UTF-8 value.
#[must_use = "a LogPrivate means something only inside a log macro's arguments"]
#[derive(Clone, Copy)]
pub struct LogPrivate<'a> {
    kind: LogKind,
    value: &'a OsStr,
}

impl<'a> LogPrivate<'a> {
    /// `value` marked as `kind`.
    pub fn new<V: AsRef<OsStr> + ?Sized>(kind: LogKind, value: &'a V) -> Self {
        LogPrivate {
            kind,
            value: value.as_ref(),
        }
    }

    /// A path whose leaf kind the site doesn't know.
    pub fn path<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::Path, value)
    }

    /// A path to a file: a report keeps its extension.
    pub fn file_path<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::FilePath, value)
    }

    /// A path to a directory: a report never reads a dot in its leaf as an extension.
    pub fn dir_path<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::DirPath, value)
    }

    /// A file or folder name the site doesn't know the kind of.
    pub fn name<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::Name, value)
    }

    /// A filename.
    pub fn file_name<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::FileName, value)
    }

    /// A folder name.
    pub fn dir_name<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::DirName, value)
    }

    /// A volume label or a share name.
    pub fn volume<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::Volume, value)
    }

    /// A host: a server, a Bonjour instance, a device on the network.
    pub fn host<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::Host, value)
    }

    /// An account name.
    pub fn user<V: AsRef<OsStr> + ?Sized>(value: &'a V) -> Self {
        Self::new(LogKind::User, value)
    }

    /// What the value is.
    pub fn kind(&self) -> LogKind {
        self.kind
    }

    /// The value as text, lossy for a non-UTF-8 one.
    pub(crate) fn text(&self) -> Cow<'a, str> {
        self.value.to_string_lossy()
    }
}

impl fmt::Display for LogPrivate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char(self.kind.opener())?;
        write_debug_body(f, &self.text())?;
        f.write_char(CLOSER)
    }
}

impl fmt::Debug for LogPrivate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{self}\"")
    }
}

/// Write `value` as the body of its `{:?}`: what `<str as Debug>` writes between its quotes.
///
/// Through `<str as Debug>` itself, never `str::escape_debug`: that one leaves a grapheme-extend
/// char (every NFD accent) raw after the first char, so the two spellings would tokenize apart.
pub(crate) fn write_debug_body(out: &mut dyn fmt::Write, value: &str) -> fmt::Result {
    write!(
        DebugBody {
            out,
            opened: false,
            held: None
        },
        "{value:?}"
    )
}

/// Passes `<str as Debug>`'s output through without its outer quotes: it skips the first char
/// and holds back the latest last one, so the one still held when the write ends (the closing
/// quote) never goes out.
struct DebugBody<'w> {
    out: &'w mut dyn fmt::Write,
    opened: bool,
    held: Option<char>,
}

impl fmt::Write for DebugBody<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut rest = s;
        if !self.opened {
            let mut chars = rest.chars();
            if chars.next().is_none() {
                return Ok(());
            }
            self.opened = true;
            rest = chars.as_str();
        }
        let Some(last) = rest.chars().next_back() else {
            return Ok(());
        };
        if let Some(held) = self.held.replace(last) {
            self.out.write_char(held)?;
        }
        self.out.write_str(&rest[..rest.len() - last.len_utf8()])
    }
}

/// A writer that Debug-escapes everything written into it, as one `{:?}` body: a marked text
/// written through it lands one depth deeper, its spans' values escaped once more. Escaping is
/// per char, so the body of a concatenation is the concatenation of the bodies.
pub(crate) struct DebugEscaping<'w>(pub(crate) &'w mut dyn fmt::Write);

impl fmt::Write for DebugEscaping<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_debug_body(self.0, s)
    }
}

/// Every char a marked line may use as structure. A producer replaces any of them it finds in
/// text it didn't mark, so only its own markers are ever legitimate.
pub const RESERVED: RangeInclusive<char> = '\u{FDD0}'..='\u{FDEF}';

/// Ends a span of any kind.
pub const CLOSER: char = '\u{FDD1}';

/// The top of [`RESERVED`], never an opener or the closer: a reader may use these chars for its
/// own bookkeeping while it rewrites a line it has already consumed every marker from.
pub const READER_SCRATCH: RangeInclusive<char> = '\u{FDE0}'..='\u{FDEF}';

/// What a marked value is. The producer's kind decides how a report redacts it: a file path
/// keeps its extension, a dir path never gets one, and an identity gets its own token domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogKind {
    /// A path whose leaf kind the site doesn't know.
    Path,
    /// A path to a file.
    FilePath,
    /// A path to a directory.
    DirPath,
    /// A file or folder name the site doesn't know the kind of.
    Name,
    /// A filename.
    FileName,
    /// A folder name.
    DirName,
    /// A volume label or a share name.
    Volume,
    /// A host: a server, a Bonjour instance, a device on the network.
    Host,
    /// An account name.
    User,
}

impl LogKind {
    /// Every kind, in table order.
    pub const ALL: [LogKind; 9] = [
        LogKind::Path,
        LogKind::FilePath,
        LogKind::DirPath,
        LogKind::Name,
        LogKind::FileName,
        LogKind::DirName,
        LogKind::Volume,
        LogKind::Host,
        LogKind::User,
    ];

    /// The char that opens a span of this kind.
    pub const fn opener(self) -> char {
        match self {
            LogKind::Path => '\u{FDD0}',
            LogKind::FilePath => '\u{FDD2}',
            LogKind::DirPath => '\u{FDD3}',
            LogKind::Name => '\u{FDD4}',
            LogKind::FileName => '\u{FDD5}',
            LogKind::DirName => '\u{FDD6}',
            LogKind::Volume => '\u{FDD7}',
            LogKind::Host => '\u{FDD8}',
            LogKind::User => '\u{FDD9}',
        }
    }

    /// The kind `c` opens, or `None` when it opens nothing (the closer, an unassigned reserved
    /// char, or any other char).
    pub fn from_opener(c: char) -> Option<LogKind> {
        LogKind::ALL.into_iter().find(|kind| kind.opener() == c)
    }
}

/// Whether `c` is in [`RESERVED`].
pub fn is_reserved(c: char) -> bool {
    RESERVED.contains(&c)
}

/// What a marker does in a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerRole {
    /// Opens a span of this kind. A reserved char nothing assigned opens one of unknown kind
    /// ([`LogKind::Path`]): a broken span, almost always, which the reader fails closed on.
    Open(LogKind),
    /// Closes the open span at its depth.
    Close,
}

/// One marker as it sits in a line. An escaped one spans only its own backslashes; the run's
/// other backslashes belong to the text before it.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    /// Byte offset of its first byte.
    pub start: usize,
    /// Byte offset just past it.
    pub end: usize,
    /// How many `{:?}` layers it went through: 0 for a raw char.
    pub depth: u32,
    /// Opener or closer.
    pub role: MarkerRole,
}

/// Every marker in `line`, raw or escaped at any depth, left to right: the one lexer both the
/// producers (which cap marked text without cutting a span) and the app's span pass read.
///
/// An escaped marker is a maximal run of `r` backslashes, then an escape that decodes to a
/// reserved char (`u{fdd1}`, any case, one to six hex digits). Its depth is
/// `1 + r.trailing_zeros()`: at depth `d` the marker's own escape is `2^(d-1)` backslashes, and
/// every literal backslash in front of it arrives as a multiple of `2^d`, so `r = 2^(d-1) × odd`.
pub fn markers(line: &str) -> Vec<Marker> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let run_start = i;
            while i < bytes.len() && bytes[i] == b'\\' {
                i += 1;
            }
            if let Some((c, len)) = escaped_char(&line[i..])
                && is_reserved(c)
            {
                let run = i - run_start;
                let depth = 1 + run.trailing_zeros();
                let own = 1usize << (depth - 1);
                out.push(Marker {
                    start: i - own,
                    end: i + len,
                    depth,
                    role: role_of(c),
                });
                i += len;
            }
        } else if bytes[i] == 0xEF
            && let Some(c) = line[i..].chars().next()
            && is_reserved(c)
        {
            out.push(Marker {
                start: i,
                end: i + c.len_utf8(),
                depth: 0,
                role: role_of(c),
            });
            i += c.len_utf8();
        } else {
            i += 1;
        }
    }
    out
}

/// `u{…}` at the start of `s` with one to six hex digits: the char and the escape's length.
fn escaped_char(s: &str) -> Option<(char, usize)> {
    let hex_and_rest = s.strip_prefix("u{")?;
    let close = hex_and_rest.find('}')?;
    let hex = &hex_and_rest[..close];
    if !(1..=6).contains(&hex.len()) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let c = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)?;
    Some((c, 2 + close + 1))
}

fn role_of(c: char) -> MarkerRole {
    if c == CLOSER {
        MarkerRole::Close
    } else {
        MarkerRole::Open(LogKind::from_opener(c).unwrap_or(LogKind::Path))
    }
}

#[cfg(test)]
#[path = "log_private_test.rs"]
mod tests;
