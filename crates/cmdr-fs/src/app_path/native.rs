//! The native boundary: an app path to the path an OS API opens, and back.
//!
//! The conversions themselves are pure and work on bytes (POSIX) and UTF-16 units
//! (Windows), so they compile everywhere and Linux CI tests the Windows arm as data. The
//! host wrappers ([`AppPath::to_native`], [`AppPathBuf::from_native`]) pick the arm this OS
//! needs and refuse the other flavor typed.
//!
//! Every refusal is typed ([`NativeRefusal`], [`NotAnAppPath`]), ❌ never an `Option` a
//! caller turns into "not found": a `None` would hide an alternate-data-stream-shaped name
//! or a POSIX path headed for Windows' current drive.

use std::borrow::Cow;
use std::fmt;

use super::escape::{self, EscapeRefusal};
use super::{AppPath, AppPathBuf, NotAnAppPath, PathFlavor, PathKind};

/// Why an app path can't go to an OS API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRefusal {
    /// A scheme path: its volume talks to a server or a device, never this OS's files.
    NotLocal,
    /// A path shaped for the other OS: a POSIX `/x` on Windows (Win32 would read it as the
    /// CURRENT drive's root) or a drive or UNC path on macOS or Linux.
    WrongFlavor,
    /// A name holding a character Win32 can't store: `:` (❗ it would write an alternate
    /// data stream INSIDE another file), `*`, `?`, `"`, `<`, `>`, `|`, a control character,
    /// or NUL (on POSIX too: an MTP pick never reaches the OS).
    ForbiddenCharacter,
    /// A UNC server with a port (`//host:445/share`, `//host@445/share`): the Windows SMB
    /// client takes none in a path, so that text belongs to the app's own SMB backend.
    NoPortOnUnc,
    /// Past Win32's 32,767-unit ceiling, `\\?\` included. NTFS stops a little earlier and
    /// says so itself (`ERROR_FILENAME_EXCED_RANGE`).
    TooLong,
    /// A name escaped by the other alphabet: a raw byte can't become a UTF-16 unit, nor an
    /// unpaired surrogate a byte.
    ForeignEscape,
    /// The path isn't canonical (a transition-bridge value): it can't be converted
    /// faithfully, so it isn't converted at all.
    NotCanonical,
    /// [`AppPath::to_native_plain`]'s refusal: the plain form would name a different file.
    NotPlainSafe(NotPlainSafe),
}

/// Why a Windows path has no plain (non-`\\?\`) form that names the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotPlainSafe {
    /// A name ending in `.` or a space: Win32 strips it, so a plain `dup.` reads `dup`.
    TrailingDotOrSpace,
    /// A reserved device name, with or without an extension (`CON`, `con.txt`, `lpt1 .x`,
    /// `CONIN$`): the plain form opens the DEVICE (`nul.txt` writes nothing anywhere).
    ReservedName,
    /// Past 259 units: a shell API or a child process's working directory doesn't take the
    /// long-path opt-in.
    TooLong,
}

impl fmt::Display for NativeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NativeRefusal::NotLocal => "not a path on this computer",
            NativeRefusal::WrongFlavor => "a path shaped for another OS",
            NativeRefusal::ForbiddenCharacter => "a name holding a character the OS can't store",
            NativeRefusal::NoPortOnUnc => "a UNC server with a port",
            NativeRefusal::TooLong => "a path past the OS's length limit",
            NativeRefusal::ForeignEscape => "a name the OS can't spell",
            NativeRefusal::NotCanonical => "a path not in canonical form",
            NativeRefusal::NotPlainSafe(NotPlainSafe::TrailingDotOrSpace) => {
                "a name ending in a dot or space, outside a long-path form"
            }
            NativeRefusal::NotPlainSafe(NotPlainSafe::ReservedName) => {
                "a reserved device name, outside a long-path form"
            }
            NativeRefusal::NotPlainSafe(NotPlainSafe::TooLong) => "a path past 259 units, outside a long-path form",
        })
    }
}

impl std::error::Error for NativeRefusal {}

impl From<EscapeRefusal> for NativeRefusal {
    fn from(_: EscapeRefusal) -> Self {
        NativeRefusal::ForeignEscape
    }
}

/// Win32's long-path prefix, and its UNC form.
const VERBATIM: &str = r"\\?\";
const VERBATIM_UNC: &str = r"\\?\UNC\";
/// Win32's whole-path ceiling, in UTF-16 units.
const WIN32_MAX_UNITS: usize = 32_767;
/// `MAX_PATH` less its NUL: the longest plain path every Win32 API takes.
const PLAIN_MAX_UNITS: usize = 259;

/// A character a Win32 name can't hold.
fn forbidden_on_windows(c: char) -> bool {
    matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 0x20
}

/// Whether `name` is a reserved device name, with or without an extension, as Win32 reads it:
/// the part before the first `.`, trailing spaces dropped, compared without case.
fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let Some(tail) = upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT")) else {
        return false;
    };
    let mut chars = tail.chars();
    matches!((chars.next(), chars.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
}

/// Whether a UNC server names a port: a `:` outside an IPv6 literal's brackets, or the WebDAV
/// redirector's `@` spellings.
fn server_has_port(server: &str) -> bool {
    let outside = match (server.starts_with('['), server.find(']')) {
        (true, Some(end)) => &server[end + 1..],
        _ => server,
    };
    outside.contains(':') || server.contains('@')
}

/// Whether `name` means itself in a plain Win32 path: no trailing dot or space (Win32 strips
/// one), and no device name (Win32 opens the device).
fn plain_safe(name: &str) -> Result<(), NativeRefusal> {
    if name.ends_with(['.', ' ']) {
        return Err(NativeRefusal::NotPlainSafe(NotPlainSafe::TrailingDotOrSpace));
    }
    if is_reserved_device_name(name) {
        return Err(NativeRefusal::NotPlainSafe(NotPlainSafe::ReservedName));
    }
    Ok(())
}

impl AppPath {
    /// The bytes a POSIX OS call takes for this path: the text with each byte escape
    /// decoded. Borrowed when there's none.
    ///
    /// # Errors
    ///
    /// [`NativeRefusal::NotLocal`] for a scheme path, `WrongFlavor` for a drive or UNC path,
    /// `ForbiddenCharacter` for a NUL (an MTP pick), `ForeignEscape` for a unit escape, and
    /// `NotCanonical` for a bridge value.
    pub fn to_native_posix(&self) -> Result<Cow<'_, [u8]>, NativeRefusal> {
        self.check_canonical()?;
        match self.kind() {
            PathKind::Posix => {}
            PathKind::Scheme => return Err(NativeRefusal::NotLocal),
            PathKind::Drive | PathKind::Unc => return Err(NativeRefusal::WrongFlavor),
        }
        if self.as_str().contains('\0') {
            return Err(NativeRefusal::ForbiddenCharacter);
        }
        Ok(escape::unescape_bytes(self.as_str())?)
    }

    /// The UTF-16 units of the verbatim path Win32 opens: `\\?\C:\x\y`, `\\?\UNC\server\share\x`.
    /// Verbatim on purpose: Win32 then neither strips a trailing dot or space nor reads a
    /// device name, so the path names exactly this file.
    ///
    /// # Errors
    ///
    /// `NotLocal`, `WrongFlavor` (a POSIX path), `ForbiddenCharacter`, `NoPortOnUnc`,
    /// `TooLong`, `ForeignEscape` (a byte escape), and `NotCanonical`.
    pub fn to_native_windows(&self) -> Result<Vec<u16>, NativeRefusal> {
        self.windows_units(true)
    }

    /// The UTF-16 units of the plain path (no `\\?\`), for an API that can't take a verbatim
    /// one: the shell, a child process's working directory or arguments, a third-party crate.
    ///
    /// # Errors
    ///
    /// Everything [`Self::to_native_windows`] refuses, plus [`NativeRefusal::NotPlainSafe`]
    /// for every path whose plain form wouldn't name the same file. ❌ Never fall back to
    /// stripping `\\?\` by hand: that's exactly the wrong-file hazard.
    pub fn to_native_windows_plain(&self) -> Result<Vec<u16>, NativeRefusal> {
        self.windows_units(false)
    }

    fn check_canonical(&self) -> Result<(), NativeRefusal> {
        super::parse::validate_canonical_any(self.as_str()).map_err(|_| NativeRefusal::NotCanonical)
    }

    fn windows_units(&self, verbatim: bool) -> Result<Vec<u16>, NativeRefusal> {
        self.check_canonical()?;
        let text = self.as_str();
        let (prefix, after_prefix) = match self.kind() {
            PathKind::Drive => (if verbatim { VERBATIM } else { "" }, text),
            PathKind::Unc => {
                let after = &text[2..];
                let server = after.split('/').next().unwrap_or(after);
                if server_has_port(server) {
                    return Err(NativeRefusal::NoPortOnUnc);
                }
                (if verbatim { VERBATIM_UNC } else { r"\\" }, after)
            }
            PathKind::Posix => return Err(NativeRefusal::WrongFlavor),
            PathKind::Scheme => return Err(NativeRefusal::NotLocal),
        };
        let mut out: Vec<u16> = prefix.encode_utf16().collect();
        let (root_part, names) = match self.kind() {
            // `C:` then the names.
            PathKind::Drive => (&after_prefix[..2], &after_prefix[3..]),
            // `server/share` then the names; the server keeps its characters (`[::1]`).
            _ => {
                let mut split = after_prefix.splitn(3, '/');
                let server = split.next().unwrap_or("");
                let share = split.next().unwrap_or("");
                if share.chars().any(forbidden_on_windows) {
                    return Err(NativeRefusal::ForbiddenCharacter);
                }
                if !verbatim {
                    plain_safe(share)?;
                }
                escape::push_unescaped_units(server, &mut out)?;
                out.push(u16::from(b'\\'));
                (share, split.next().unwrap_or(""))
            }
        };
        escape::push_unescaped_units(root_part, &mut out)?;
        out.push(u16::from(b'\\'));
        for (i, name) in names.split('/').filter(|name| !name.is_empty()).enumerate() {
            if name.chars().any(forbidden_on_windows) {
                return Err(NativeRefusal::ForbiddenCharacter);
            }
            if !verbatim {
                plain_safe(name)?;
            }
            if i > 0 {
                out.push(u16::from(b'\\'));
            }
            escape::push_unescaped_units(name, &mut out)?;
        }
        if out.len() > WIN32_MAX_UNITS {
            return Err(NativeRefusal::TooLong);
        }
        if !verbatim && out.len() > PLAIN_MAX_UNITS {
            return Err(NativeRefusal::NotPlainSafe(NotPlainSafe::TooLong));
        }
        Ok(out)
    }
}

impl AppPathBuf {
    /// The app path for native POSIX `bytes` the OS reported, read with `flavor`: each name
    /// spelled losslessly (byte escapes for what isn't UTF-8).
    ///
    /// # Errors
    ///
    /// As [`Self::parse`]: a relative path, a `..` name (only the OS resolves one), and
    /// under the Windows flavor everything [`Self::from_native_windows`] refuses.
    pub fn from_native_posix(bytes: &[u8], flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
        from_escaped(&escape::escape_bytes(bytes), flavor)
    }

    /// The app path for a native Windows path's UTF-16 `units`, read with `flavor`: each
    /// unpaired surrogate as a unit escape.
    ///
    /// Takes the verbatim forms (`\\?\C:\x`, `\\?\UNC\server\share\x`) and the plain ones
    /// (`C:\x`, `\\server\share\x`, either separator). A plain path's `.` and `..` resolve
    /// lexically, as Win32 itself resolves them before the filesystem sees the path.
    ///
    /// # Errors
    ///
    /// Typed [`NotAnAppPath`]: a relative path, a drive-relative `C:foo` and a bare `C:`
    /// (both that drive's CURRENT directory to Win32), the device namespaces (`\\.\…`,
    /// `\\?\Volume{…}`, `\\?\GLOBALROOT\…`, a server of `?` or `.`), and an empty, `.`, or
    /// `..` name inside a verbatim path, which NTFS refuses.
    pub fn from_native_windows(units: &[u16], flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
        from_escaped(&escape::escape_units(units), flavor)
    }
}

/// The grammar half of `from_native`, over text whose names are already escaped. An escape
/// is a plane-16 code point, never a separator, so reading the escaped text is safe.
fn from_escaped(text: &str, flavor: PathFlavor) -> Result<AppPathBuf, NotAnAppPath> {
    match flavor {
        PathFlavor::Posix => AppPathBuf::parse(text, PathFlavor::Posix),
        PathFlavor::Windows => windows_from_native(text),
    }
}

fn windows_from_native(text: &str) -> Result<AppPathBuf, NotAnAppPath> {
    if text.is_empty() {
        return Err(NotAnAppPath::Empty);
    }
    // NT reads the `UNC` of `\\?\UNC\` without case.
    let verbatim_unc = text
        .get(..VERBATIM_UNC.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(VERBATIM_UNC));
    if verbatim_unc {
        let unc = &text[VERBATIM_UNC.len()..];
        let mut parts = unc.splitn(3, '\\');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        if share.is_empty() && !server.is_empty() && server != "?" && server != "." {
            return Err(NotAnAppPath::MissingShare);
        }
        return verbatim_names(&format!("//{server}/{share}/"), parts.next().unwrap_or(""));
    }
    if let Some(local) = text.strip_prefix(VERBATIM) {
        let upper = local.get(..10).map(str::to_ascii_uppercase);
        if local.get(..7).is_some_and(|head| head.eq_ignore_ascii_case("Volume{"))
            || upper.as_deref() == Some("GLOBALROOT")
        {
            return Err(NotAnAppPath::DevicePath);
        }
        let bytes = local.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            if bytes.len() == 2 {
                return Err(NotAnAppPath::BareDrive);
            }
            if bytes[2] != b'\\' {
                return Err(NotAnAppPath::DriveRelative);
            }
            let letter = char::from(bytes[0].to_ascii_uppercase());
            return verbatim_names(&format!("{letter}:/"), &local[3..]);
        }
        return Err(NotAnAppPath::DevicePath);
    }
    // A plain path the OS reported holds each name as stored: a `read_dir` under a plain root
    // joins `dup.` verbatim, so every trailing dot and space stays (❌ never Win32's strip of a
    // path PASSED IN, which would name `dup` beside `dup.`). Only `.` and `..` resolve by text,
    // which no stored name is.
    let slashed = text.replace('\\', "/");
    if slashed.starts_with("//./") || slashed.starts_with("//?/") {
        return Err(NotAnAppPath::DevicePath);
    }
    let bytes = slashed.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(NotAnAppPath::BareDrive);
    }
    let resolved = crate::volume::path_grammar::normalize_local(&slashed, PathFlavor::Windows).ok_or_else(|| {
        AppPathBuf::parse(&slashed, PathFlavor::Windows)
            .err()
            .unwrap_or(NotAnAppPath::Relative)
    })?;
    AppPathBuf::parse(&resolved, PathFlavor::Windows)
}

/// `root` plus the `\`-separated `names` of a verbatim path, where Win32 does no
/// normalization: an empty, `.`, or `..` name is refused (one trailing `\` is fine), and `/`
/// is no separator.
fn verbatim_names(root: &str, names: &str) -> Result<AppPathBuf, NotAnAppPath> {
    let names = names.strip_suffix('\\').unwrap_or(names);
    let mut text = String::with_capacity(root.len() + names.len());
    text.push_str(root);
    if !names.is_empty() {
        for (i, name) in names.split('\\').enumerate() {
            if matches!(name, "" | "." | "..") || name.contains('/') {
                return Err(NotAnAppPath::VerbatimName);
            }
            if i > 0 {
                text.push('/');
            }
            text.push_str(name);
        }
    }
    AppPathBuf::parse(&text, PathFlavor::Windows)
}
