//! The app path: the path a pane, a `Volume` method, a `FileEntry`, and the wire hold, as a
//! type of its own, with its own arithmetic.
//!
//! An app path is a ROOT, then names joined by `/`: `/Users/x` (POSIX), `C:/Users/x` (a
//! drive), `//server/share/x` (a UNC share), `sftp://u@h:22/srv/x` (a scheme path).
//! `std::path` reads such text with the host's rules, so on Windows a `join`, `parent`, or
//! `display` would add `\` to a remote path, read `a:b` as a drive, and reparse a drive
//! path. This type does its own arithmetic, the same on every OS, and crosses to the OS
//! only at named boundaries ([`AppPath::to_native`], [`AppPathBuf::from_native`]).
//!
//! - [`AppPath`] is borrowed and unsized like `Path`; [`AppPathBuf`] owns one like
//!   `PathBuf`. Both hold UTF-8 text. A native name that isn't valid Unicode text is spelled
//!   losslessly by an escape (`escape.rs`).
//! - **Canonical form**: a root, then names, no empty, `.`, or `..` name, no trailing `/`
//!   except the root's own (every root ends in `/`: `/`, `C:/`, `//server/share/`,
//!   `scheme://authority/`). Case and Unicode form are never touched: case belongs to the
//!   volume. [`AppPathBuf::parse`] normalizes into it; [`AppPath::from_normalized`] only
//!   checks it.
//! - **❌ No `From`, `AsRef`, or `Deref` to or from `std::path`**, ever: a native path
//!   becomes an app path only through a named, searchable constructor. The one transition
//!   bridge ([`AppPath::legacy_unchecked`], [`AppPath::as_std_legacy`]) is counted by the
//!   `app-path-std-bridge` check and goes away once every crate speaks this type.
//! - `Eq`, `Hash`, and `Ord` are component-wise: text the host's OS opens reads as its
//!   `PathBuf` does, everything else with the app's own grammar, so a hashed collection keyed
//!   on app paths behaves as one keyed on `PathBuf` for every value the host's `std` meets
//!   (`shape::components` says where they differ, and why). A sorted one does too, except
//!   that an escaped name sorts by its escape code point, above every raw byte, where
//!   `PathBuf` sorted the raw byte (`crates/cmdr-fs/DETAILS.md` § "App paths").
//!
//! Pure: builds for the browser too, and every conversion runs on every OS as data.
//! `crates/cmdr-fs/DETAILS.md` § "App paths" holds the design and its decisions.

mod bridge;
mod escape;
mod lenient;
mod native;
mod parse;
mod platform;
mod serde_impls;
mod shape;
mod traits;

use std::borrow::Cow;

pub use bridge::{BridgedPath, ListedFolder};
pub use escape::{
    EscapeRefusal, Namer, canonical_name, escape_bytes, escape_units, is_escape, native_name_length, shown_name,
    unescape_bytes, unescape_units,
};
pub use native::{NativeRefusal, NotPlainSafe};
pub use parse::NotAnAppPath;
#[cfg(cmdr_host)]
pub use platform::expand_short_names;
pub use shape::is_scheme_url;

use crate::volume::NotAChildName;

/// Whether this build's escape producers are live, so a stored path text may hold a live
/// escape. `false` until milestone 10f of `docs/specs/app-path-plan.md`, whose producer commit
/// flips it. A durable store that keeps app paths (the operation log) stamps a mark at the
/// first open with this `true` and reads every older row through
/// [`AppPathBuf::from_literal_store`].
pub const ESCAPES_LIVE: bool = false;

/// Which roots the local backend has: `/` alone, or drives and UNC shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathFlavor {
    /// macOS and Linux: one root, `/`.
    Posix,
    /// Windows: a root per drive (`C:/`) and per UNC share (`//server/share/`).
    Windows,
}

impl PathFlavor {
    /// The flavor of the OS this build runs on.
    pub const HOST: PathFlavor = platform::HOST_FLAVOR;
}

/// The shape of an app path's root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathKind {
    /// `/…` on macOS and Linux.
    Posix,
    /// `C:/…` on Windows.
    Drive,
    /// `//server/share/…` on Windows.
    Unc,
    /// `<scheme>://…`: a volume with no local mount, whose root is its own business.
    Scheme,
}

/// A borrowed app path, unsized like `Path`. Canonical unless it came through the
/// transition bridge ([`AppPath::legacy_unchecked`]), whose values keep their text as is.
#[repr(transparent)]
pub struct AppPath(str);

/// An owned app path, like `PathBuf`.
#[derive(Clone)]
pub struct AppPathBuf(String);

/// The names under an ancestor: `a/b`, or nothing at all. No leading or trailing `/`, and no
/// empty, `.`, or `..` name, so joining it can neither replace a base nor climb off it.
#[repr(transparent)]
pub struct RelativeAppPath(str);

/// An owned [`RelativeAppPath`]: what [`AppPath::strip_prefix`] answers for a bridge value.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RelativeAppPathBuf(String);

/// One plain name of an app path, as a backend spells it: never empty, `.`, or `..`, never
/// holding a `/`, a NUL only as an MTP pick, and its escapes canonical. The only thing
/// [`AppPath::join`] takes.
#[repr(transparent)]
pub struct AppName(str);

/// An owned [`AppName`].
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AppNameBuf(String);

impl AppPath {
    /// `text` as an app path, unchecked: callers have validated it, or it's a bridge value.
    fn new_unchecked(text: &str) -> &AppPath {
        // SAFETY: `AppPath` is `#[repr(transparent)]` over `str`, so a `&str` and a `&AppPath`
        // have the same layout and the same pointer metadata (the length); the lifetime is
        // carried over unchanged.
        unsafe { &*(std::ptr::from_ref::<str>(text) as *const AppPath) }
    }

    /// The text: the one wire spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is in canonical form. Every value but a transition-bridge one is; on a
    /// bridge value (`/a/`, `C:\x`) the arithmetic below answers through `lenient.rs`, right
    /// in release builds too, ❌ never by reading the text as if it were canonical.
    pub fn is_canonical(&self) -> bool {
        lenient::is_canonical(&self.0)
    }

    /// The canonical value naming the same place, borrowed when this already is one.
    ///
    /// # Errors
    ///
    /// For a bridge value with no canonical twin: no root (`Relative`), `C:x`
    /// (`DriveRelative`), a device path, or a `..` (`DotName`: by text it can name a
    /// different file across a symlink).
    pub fn to_canonical(&self) -> Result<Cow<'_, AppPath>, NotAnAppPath> {
        if self.is_canonical() {
            return Ok(Cow::Borrowed(self));
        }
        lenient::canonical(&self.0).map(Cow::Owned)
    }

    /// The shape of the root.
    pub fn kind(&self) -> PathKind {
        if self.is_canonical() {
            shape::root_of(&self.0).0
        } else {
            lenient::read(&self.0).kind()
        }
    }

    /// The byte length of the root, the root's own trailing `/` included.
    fn root_len(&self) -> usize {
        if self.is_canonical() {
            shape::root_of(&self.0).1
        } else {
            lenient::read(&self.0).root_end()
        }
    }

    /// The root itself: `/`, `C:/`, `//server/share/`, or `scheme://authority/` (a bridge
    /// value's root as its text spells it: `C:\`).
    pub fn root(&self) -> &AppPath {
        AppPath::new_unchecked(&self.0[..self.root_len()])
    }

    /// Whether this is its volume's root.
    pub fn is_root(&self) -> bool {
        if self.is_canonical() {
            shape::root_of(&self.0).1 >= self.0.len()
        } else {
            lenient::read(&self.0).is_root()
        }
    }

    /// The last name as a person reads it (every escape as U+FFFD), or `None` at a root.
    pub fn shown_name(&self) -> Option<Cow<'_, str>> {
        self.file_name().map(shown_name)
    }

    /// The names under the root, outermost first (a bridge value's as `Path` lists them, a
    /// `..` included).
    pub fn names(&self) -> impl DoubleEndedIterator<Item = &str> {
        let (canonical, lenient) = if self.is_canonical() {
            let root_len = shape::root_of(&self.0).1;
            (&self.0[root_len.min(self.0.len())..], Vec::new())
        } else {
            ("", lenient::read(&self.0).names(&self.0))
        };
        canonical.split('/').filter(|name| !name.is_empty()).chain(lenient)
    }

    /// `rel`'s names inside this folder: the multi-name join, for a validated tail (a remote
    /// root's offset, a [`Self::rebase`]).
    ///
    /// # Errors
    ///
    /// As [`Self::join`].
    pub fn join_relative(&self, rel: &RelativeAppPath) -> Result<AppPathBuf, NotAnAppPath> {
        if rel.0.is_empty() {
            return Ok(self.to_owned());
        }
        self.join_text(&rel.0)
    }

    fn join_text(&self, tail: &str) -> Result<AppPathBuf, NotAnAppPath> {
        if !self.is_canonical() {
            // A bridge base joins as the canonical place it names, so the result is canonical.
            return lenient::canonical(&self.0)?.join_text(tail);
        }
        self.join_canonical_text(tail)
    }

    /// [`Self::join_text`] for a path already known canonical: no re-check, so a caller
    /// joining many names under one folder ([`ListedFolder`]) checks the folder once.
    fn join_canonical_text(&self, tail: &str) -> Result<AppPathBuf, NotAnAppPath> {
        debug_assert!(self.is_canonical(), "{:?} isn't canonical", &self.0);
        if matches!(shape::root_of(&self.0).0, PathKind::Drive | PathKind::Unc) && tail.contains('\\') {
            return Err(NotAnAppPath::Backslash);
        }
        let slash = !self.0.ends_with('/');
        let mut out = String::with_capacity(self.0.len() + usize::from(slash) + tail.len());
        out.push_str(&self.0);
        if slash {
            out.push('/');
        }
        out.push_str(tail);
        Ok(AppPathBuf(out))
    }

    fn strip_prefix_canonical(&self, ancestor: &AppPath) -> Option<&RelativeAppPath> {
        // Never across roots: `//s/h/x` is not under `/`, though its text starts with it.
        if self.root().0 != ancestor.root().0 {
            return None;
        }
        let rest = self.0.strip_prefix(&ancestor.0)?;
        if rest.is_empty() {
            return Some(RelativeAppPath::new_unchecked(""));
        }
        if ancestor.0.ends_with('/') {
            return Some(RelativeAppPath::new_unchecked(rest));
        }
        rest.strip_prefix('/').map(RelativeAppPath::new_unchecked)
    }

    /// Whether both paths sit on the same root. A UNC server compares without ASCII case (DNS
    /// and NetBIOS names) and its share with Unicode simple case folding (SMB share names are
    /// case-insensitive on the wire); every other root exactly, a scheme root included (every
    /// backend mints its authority lowercase). What a volume pick asks; ❌ never `Eq`, which
    /// stays exact.
    pub fn same_root(&self, other: &AppPath) -> bool {
        if !(self.is_canonical() && other.is_canonical()) {
            return match (self.to_canonical(), other.to_canonical()) {
                (Ok(mine), Ok(theirs)) => mine.same_root(&theirs),
                _ => false,
            };
        }
        let (mine, theirs) = (self.root(), other.root());
        match (mine.kind(), theirs.kind()) {
            (PathKind::Unc, PathKind::Unc) => {
                let split = |root: &AppPath| -> (String, String) {
                    let mut parts = root.0[2..].splitn(2, '/');
                    let server = parts.next().unwrap_or("").to_string();
                    let share = parts.next().unwrap_or("").trim_end_matches('/').to_string();
                    (server, share)
                };
                let ((server_a, share_a), (server_b, share_b)) = (split(mine), split(theirs));
                server_a.eq_ignore_ascii_case(&server_b)
                    && share_a.chars().map(simple_fold).eq(share_b.chars().map(simple_fold))
            }
            _ => mine.0 == theirs.0,
        }
    }

    /// The names below the volume root `volume_root` (empty for the root itself), or `None`
    /// when this path isn't on that volume: the root matched as [`Self::same_root`] matches it
    /// (a UNC server and share without case), the names below it exactly. What a volume pick
    /// and a "does this path belong to that volume" check ask; [`Self::strip_prefix`] is the
    /// exact twin.
    pub fn strip_volume_prefix(&self, volume_root: &AppPath) -> Option<Cow<'_, RelativeAppPath>> {
        if self.is_canonical() && volume_root.is_canonical() {
            return self.strip_volume_prefix_canonical(volume_root).map(Cow::Borrowed);
        }
        let (mine, theirs) = (self.to_canonical().ok()?, volume_root.to_canonical().ok()?);
        Some(Cow::Owned(mine.strip_volume_prefix_canonical(&theirs)?.to_owned()))
    }

    fn strip_volume_prefix_canonical(&self, volume_root: &AppPath) -> Option<&RelativeAppPath> {
        if !self.same_root(volume_root) {
            return None;
        }
        let mine = &self.0[shape::root_of(&self.0).1..];
        let theirs = &volume_root.0[shape::root_of(&volume_root.0).1..];
        if theirs.is_empty() {
            return Some(RelativeAppPath::new_unchecked(mine));
        }
        let rest = mine.strip_prefix(theirs)?;
        if rest.is_empty() {
            return Some(RelativeAppPath::new_unchecked(""));
        }
        rest.strip_prefix('/').map(RelativeAppPath::new_unchecked)
    }

    /// Whether this path is on the volume rooted at `volume_root` ([`Self::strip_volume_prefix`]).
    pub fn is_on_volume(&self, volume_root: &AppPath) -> bool {
        self.strip_volume_prefix(volume_root).is_some()
    }

    /// What a person reads, the ONE formatter, twin of the frontend's `displayPath`: every
    /// escape as U+FFFD, every MTP pick and S3 access key id taken out, and a drive or UNC
    /// path spelled with `\` (`C:\Users\x`, `\\server\share`).
    pub fn shown(&self) -> Cow<'_, str> {
        let text = shown_name(&self.0);
        let text = match crate::twin_path::shown(&text) {
            Cow::Borrowed(_) => text,
            Cow::Owned(owned) => Cow::Owned(owned),
        };
        let text = match crate::volume::without_s3_key_ids(&text) {
            Cow::Borrowed(_) => text,
            Cow::Owned(owned) => Cow::Owned(owned),
        };
        match self.kind() {
            PathKind::Drive => Cow::Owned(text.replace('/', "\\")),
            PathKind::Unc => {
                let trimmed = if self.is_root() {
                    text.trim_end_matches('/')
                } else {
                    &text
                };
                Cow::Owned(trimmed.replace('/', "\\"))
            }
            PathKind::Posix | PathKind::Scheme => text,
        }
    }
}

impl AppPathBuf {
    /// The borrowed path.
    pub fn as_app_path(&self) -> &AppPath {
        AppPath::new_unchecked(&self.0)
    }

    /// The text, owned.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl RelativeAppPath {
    fn new_unchecked(text: &str) -> &RelativeAppPath {
        // SAFETY: `RelativeAppPath` is `#[repr(transparent)]` over `str`: same layout and
        // pointer metadata, lifetime carried over.
        unsafe { &*(std::ptr::from_ref::<str>(text) as *const RelativeAppPath) }
    }

    /// `text` as a relative tail: names joined by `/`, each one plain and canonical, or the
    /// empty text.
    ///
    /// # Errors
    ///
    /// [`NotAnAppPath::NotCanonical`] for an empty or `.` name or a leading or trailing `/`,
    /// `DotName` for `..`, and each name's own refusal.
    pub fn new(text: &str) -> Result<&RelativeAppPath, NotAnAppPath> {
        if !text.is_empty() {
            for name in text.split('/') {
                match name {
                    "" | "." => return Err(NotAnAppPath::NotCanonical),
                    ".." => return Err(NotAnAppPath::DotName),
                    _ => {}
                }
                if parse::checked_name(name, PathKind::Posix)? != name {
                    return Err(NotAnAppPath::NotCanonical);
                }
            }
        }
        Ok(RelativeAppPath::new_unchecked(text))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it names no name at all (an ancestor's own place).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The names, outermost first.
    pub fn names(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.0.split('/').filter(|name| !name.is_empty())
    }
}

impl AppName {
    fn new_unchecked(text: &str) -> &AppName {
        // SAFETY: `AppName` is `#[repr(transparent)]` over `str`: same layout and pointer
        // metadata, lifetime carried over.
        unsafe { &*(std::ptr::from_ref::<str>(text) as *const AppName) }
    }

    /// `name` in APP spelling, as a backend spelled it (a `FileEntry.name`, a wire string):
    /// checked, never respelled, so its escapes stay live.
    ///
    /// ❗ The name says what the text must be: an `OsStr`'s `to_str()` or `to_string_lossy()`
    /// is NOT app spelling (a native name holding U+10FEE9 as text would cross `to_native` as
    /// the byte `E9`, a different file). A native name goes through
    /// [`AppNameBuf::from_native_os`]; `app-path-std-bridge` refuses an OS-text hop into this
    /// door on one line.
    ///
    /// # Errors
    ///
    /// [`NotAChildName`] for the empty name, `.`, `..`, a `/`, a NUL outside a pick, or
    /// escapes that aren't canonical.
    pub fn from_app_spelling(name: &str) -> Result<&AppName, NotAChildName> {
        let refused = || NotAChildName(shown_name(name).into_owned());
        if matches!(name, "" | "." | "..") || name.contains('/') {
            return Err(refused());
        }
        match parse::checked_name(name, PathKind::Posix) {
            Ok(Cow::Borrowed(_)) => Ok(AppName::new_unchecked(name)),
            _ => Err(refused()),
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name as a person reads it: every escape as U+FFFD.
    pub fn shown(&self) -> Cow<'_, str> {
        shown_name(&self.0)
    }
}

impl AppNameBuf {
    /// A name a person typed (rename, new file, new folder, a transfer's destination name):
    /// every escape-range code point is LITERAL text, spelled through `namer`'s units, so
    /// typed text always means what it says.
    ///
    /// # Errors
    ///
    /// As [`AppName::from_app_spelling`].
    pub fn from_typed(text: &str, namer: Namer) -> Result<AppNameBuf, NotAChildName> {
        let literal = escape::literal(text, namer);
        AppName::from_app_spelling(&literal).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The app spelling of a name THIS HOST's `OsStr` holds natively (from a `read_dir`, a
    /// `Path::file_name`): every unit the host's decoder rejects escaped, and an escape-range
    /// code point it holds as text escaped as literal. The one door from a `ChildName`, which
    /// lives in the `std::path` world: `AppNameBuf::from_native_os(child.as_os_str())`. ❌ Never
    /// feed it app text (a `FileEntry.name`): that's [`AppName::from_app_spelling`].
    ///
    /// # Errors
    ///
    /// As [`AppName::from_app_spelling`].
    pub fn from_native_os(name: &std::ffi::OsStr) -> Result<AppNameBuf, NotAChildName> {
        let text = platform::os_name_to_app(name);
        AppName::from_app_spelling(&text).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The app spelling of a native name in raw bytes (a Unix disk, SFTP, NFS).
    ///
    /// # Errors
    ///
    /// As [`AppName::from_app_spelling`].
    pub fn from_native_bytes(bytes: &[u8]) -> Result<AppNameBuf, NotAChildName> {
        let text = escape_bytes(bytes);
        AppName::from_app_spelling(&text).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The app spelling of a native name in UTF-16 units (a Windows disk, SMB).
    ///
    /// # Errors
    ///
    /// As [`AppName::from_app_spelling`].
    pub fn from_native_units(units: &[u16]) -> Result<AppNameBuf, NotAChildName> {
        let text = escape_units(units);
        AppName::from_app_spelling(&text).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The borrowed name.
    pub fn as_app_name(&self) -> &AppName {
        AppName::new_unchecked(&self.0)
    }

    /// The text, owned: a listing's row name, with no copy beside the name it joined.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// The one simple case folding the UNC share match uses: a character's lowercase when it's a
/// single character (so `K`, the Kelvin sign, folds to `k`), else the character itself (so
/// `İ` doesn't become `i` plus a combining dot).
fn simple_fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// The app path's own arithmetic for every method `Path` also has (`parent`, `file_name`,
/// `join`, `starts_with`, `strip_prefix`, …), kept off the inherent `impl` so a value the trait
/// flip typed can't silently answer with it: while bodies still do `std::path` arithmetic
/// through the bridge, `x.file_name()` on a typed `x` must fail to compile (the flip's codemod
/// then drops the receiver to `as_std_legacy()`, `Path`'s answer), ❌ never pick these, which
/// answer differently on bridge values (`flip_shapes_test.rs`). Import it to adopt them on
/// purpose: this module and its pins do; each crate's own conversion milestone imports it
/// where it flips its pins deliberately (docs/specs/app-path-plan.md § "4. Flip the trait").
pub trait PathArithmetic {
    /// The folder holding this path (a slice, no allocation), or `None` at a root: there is
    /// no parent INSIDE the volume. A row under an MTP pick answers the pick, as `Path` did;
    /// `twin_path::folder_of` steps over it.
    fn parent(&self) -> Option<&AppPath>;

    /// The last name, or `None` at a root.
    fn file_name(&self) -> Option<&str>;

    /// The last name's extension, as `Path::extension` reads one: after the last `.`, `None`
    /// for a name with no `.` past its first character (`.bashrc`) and at a root.
    fn extension(&self) -> Option<&str>;

    /// The last name without its extension, as `Path::file_stem` reads it.
    fn file_stem(&self) -> Option<&str>;

    /// This path with its last name replaced by `name` (at a root, `name` under it, as
    /// `Path::with_file_name` does).
    ///
    /// # Errors
    ///
    /// As [`Self::join`], plus [`NotAnAppPath::DotName`] for a bridge value ending in `..`.
    fn with_file_name(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath>;

    /// `name` inside this folder: THE join, one plain name, so it can neither climb nor
    /// replace the base. One allocation, sized exactly.
    ///
    /// # Errors
    ///
    /// [`NotAnAppPath::Backslash`] when this is a drive or UNC path and `name` holds a `\`
    /// (a name from another OS's listing): Windows would read it as a subfolder.
    fn join(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath>;

    /// Whether this is `ancestor` or sits under it, by whole names. Exact, case included:
    /// case is the volume's business (`same_root` folds a UNC root).
    fn starts_with(&self, ancestor: &AppPath) -> bool;

    /// The names below `ancestor` (empty for `ancestor` itself), or `None` when this isn't
    /// `ancestor` or under it. A slice, no allocation, for canonical values. A bridge value
    /// (either side) is read as the canonical place it names, its tail owned; one with no
    /// canonical twin (a `..`, no root) sits under nothing, since by text a `..` can name a
    /// different file across a symlink.
    fn strip_prefix(&self, ancestor: &AppPath) -> Option<Cow<'_, RelativeAppPath>>;

    /// [`Self::strip_prefix`], named for readers asking "where is this, from there".
    fn relative_to(&self, ancestor: &AppPath) -> Option<Cow<'_, RelativeAppPath>>;

    /// This path moved from under `old_root` to the same place under `new_root` (a remounted
    /// share, a renamed drive), or `None` when it isn't under `old_root`, or when `new_root`
    /// can't hold its names.
    fn rebase(&self, old_root: &AppPath, new_root: &AppPath) -> Option<AppPathBuf>;
}

impl PathArithmetic for AppPath {
    fn parent(&self) -> Option<&AppPath> {
        if !self.is_canonical() {
            let end = lenient::read(&self.0).parent_end()?;
            return Some(AppPath::new_unchecked(&self.0[..end]));
        }
        let root_len = shape::root_of(&self.0).1;
        if root_len >= self.0.len() {
            return None;
        }
        let cut = self.0.rfind('/')?;
        let end = if cut < root_len { root_len } else { cut };
        Some(AppPath::new_unchecked(&self.0[..end]))
    }

    fn file_name(&self) -> Option<&str> {
        if !self.is_canonical() {
            return lenient::read(&self.0).file_name();
        }
        let root_len = shape::root_of(&self.0).1;
        if root_len >= self.0.len() {
            return None;
        }
        Some(&self.0[self.0.rfind('/').map_or(0, |i| i + 1).max(root_len)..])
    }

    fn extension(&self) -> Option<&str> {
        split_at_dot(self.file_name()?).1
    }

    fn file_stem(&self) -> Option<&str> {
        split_at_dot(self.file_name()?).0
    }

    fn with_file_name(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath> {
        // A bridge value ending in `..` has no last name to replace: `Path` would build
        // `/a/b/../z` (which opens `/a/z`), the lenient parent `/a/b/z`, another folder.
        if !self.is_root() && self.file_name().is_none() {
            return Err(NotAnAppPath::DotName);
        }
        self.parent().unwrap_or(self).join(name)
    }

    fn join(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath> {
        self.join_text(&name.0)
    }

    fn starts_with(&self, ancestor: &AppPath) -> bool {
        self.strip_prefix(ancestor).is_some()
    }

    fn strip_prefix(&self, ancestor: &AppPath) -> Option<Cow<'_, RelativeAppPath>> {
        if self.is_canonical() && ancestor.is_canonical() {
            return self.strip_prefix_canonical(ancestor).map(Cow::Borrowed);
        }
        let (mine, theirs) = (self.to_canonical().ok()?, ancestor.to_canonical().ok()?);
        Some(Cow::Owned(mine.strip_prefix_canonical(&theirs)?.to_owned()))
    }

    fn relative_to(&self, ancestor: &AppPath) -> Option<Cow<'_, RelativeAppPath>> {
        self.strip_prefix(ancestor)
    }

    fn rebase(&self, old_root: &AppPath, new_root: &AppPath) -> Option<AppPathBuf> {
        new_root.join_relative(&self.strip_prefix(old_root)?).ok()
    }
}

/// `Path`'s split of a name at its extension's dot: `(stem, extension)`.
fn split_at_dot(name: &str) -> (Option<&str>, Option<&str>) {
    if name == ".." {
        return (Some(name), None);
    }
    match name.rsplit_once('.') {
        Some(("", _)) | None => (Some(name), None),
        Some((stem, extension)) => (Some(stem), Some(extension)),
    }
}

/// A canonical app path literal, for tests: `app_path!("/a/b")` is a `&'static AppPath`.
/// Panics on a literal that isn't canonical (`"/a/"`, `"/a/../b"`, `""`), so a converted test
/// can't silently test a different path.
#[cfg(any(test, feature = "testing"))]
#[macro_export]
macro_rules! app_path {
    ($text:literal) => {
        $crate::app_path::AppPath::from_normalized($text)
            .unwrap_or_else(|why| panic!("app_path!({:?}) is no canonical app path: {why}", $text))
    };
}

#[cfg(test)]
mod arithmetic_is_opt_in_test;
#[cfg(test)]
mod bridge_test;
#[cfg(test)]
mod flip_shapes_test;
#[cfg(test)]
mod tests;
