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
//!   `PathBuf` does, everything else with the app's own grammar, so a sorted or hashed
//!   collection keyed on app paths behaves as one keyed on `PathBuf` for every value the
//!   host's `std` meets (`shape::components` says where they differ, and why).
//!
//! Pure: builds for the browser too, and every conversion runs on every OS as data.
//! `crates/cmdr-fs/DETAILS.md` § "App paths" holds the design and its decisions.

mod bridge;
mod escape;
mod native;
mod parse;
mod platform;
mod serde_impls;
mod shape;

use std::borrow::{Borrow, Cow};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

pub use escape::{
    EscapeRefusal, Namer, canonical_name, escape_bytes, escape_units, is_escape, native_name_length, shown_name,
    unescape_bytes, unescape_units,
};
pub use native::{NativeRefusal, NotPlainSafe};
pub use parse::NotAnAppPath;
pub use shape::is_scheme_url;

use crate::volume::NotAChildName;

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

    /// Typed arithmetic is defined on canonical values only: a transition-bridge value keeps
    /// unnormalized text (`/a/`, `C:\x`), whose parent or name the type would read wrong. Such
    /// a body does its arithmetic on `as_std_legacy` until it converts.
    fn debug_check_canonical(&self) {
        debug_assert!(
            parse::validate_canonical_any(&self.0).is_ok(),
            "typed arithmetic on a non-canonical app path: {:?}",
            &self.0
        );
    }

    /// The shape of the root.
    pub fn kind(&self) -> PathKind {
        shape::root_of(&self.0).0
    }

    /// The root itself: `/`, `C:/`, `//server/share/`, or `scheme://authority/`.
    pub fn root(&self) -> &AppPath {
        AppPath::new_unchecked(&self.0[..shape::root_of(&self.0).1])
    }

    /// Whether this is its volume's root.
    pub fn is_root(&self) -> bool {
        shape::root_of(&self.0).1 >= self.0.len()
    }

    /// The folder holding this path (a slice, no allocation), or `None` at a root: there is
    /// no parent INSIDE the volume. A row under an MTP pick answers the pick, as `Path` did;
    /// `twin_path::folder_of` steps over it.
    pub fn parent(&self) -> Option<&AppPath> {
        self.debug_check_canonical();
        let root_len = shape::root_of(&self.0).1;
        if root_len >= self.0.len() {
            return None;
        }
        let cut = self.0.rfind('/')?;
        let end = if cut < root_len { root_len } else { cut };
        Some(AppPath::new_unchecked(&self.0[..end]))
    }

    /// The last name, or `None` at a root.
    pub fn file_name(&self) -> Option<&str> {
        self.debug_check_canonical();
        let root_len = shape::root_of(&self.0).1;
        if root_len >= self.0.len() {
            return None;
        }
        Some(&self.0[self.0.rfind('/').map_or(0, |i| i + 1).max(root_len)..])
    }

    /// The last name's extension, as `Path::extension` reads one: after the last `.`, `None`
    /// for a name with no `.` past its first character (`.bashrc`) and at a root.
    pub fn extension(&self) -> Option<&str> {
        split_at_dot(self.file_name()?).1
    }

    /// The last name without its extension, as `Path::file_stem` reads it.
    pub fn file_stem(&self) -> Option<&str> {
        split_at_dot(self.file_name()?).0
    }

    /// This path with its last name replaced by `name` (at a root, `name` under it, as
    /// `Path::with_file_name` does).
    ///
    /// # Errors
    ///
    /// As [`Self::join`].
    pub fn with_file_name(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath> {
        self.parent().unwrap_or(self).join(name)
    }

    /// The last name as a person reads it (every escape as U+FFFD), or `None` at a root.
    pub fn shown_name(&self) -> Option<Cow<'_, str>> {
        self.file_name().map(shown_name)
    }

    /// The names under the root, outermost first.
    pub fn names(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.debug_check_canonical();
        let root_len = shape::root_of(&self.0).1;
        self.0[root_len.min(self.0.len())..]
            .split('/')
            .filter(|name| !name.is_empty())
    }

    /// `name` inside this folder: THE join, one plain name, so it can neither climb nor
    /// replace the base. One allocation, sized exactly.
    ///
    /// # Errors
    ///
    /// [`NotAnAppPath::Backslash`] when this is a drive or UNC path and `name` holds a `\`
    /// (a name from another OS's listing): Windows would read it as a subfolder.
    pub fn join(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath> {
        self.join_text(&name.0)
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
        self.debug_check_canonical();
        if matches!(self.kind(), PathKind::Drive | PathKind::Unc) && tail.contains('\\') {
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

    /// Whether this is `ancestor` or sits under it, by whole names. Exact, case included:
    /// case is the volume's business (`same_root` folds a UNC root).
    pub fn starts_with(&self, ancestor: &AppPath) -> bool {
        self.strip_prefix(ancestor).is_some()
    }

    /// The names below `ancestor` (empty for `ancestor` itself), or `None` when this isn't
    /// `ancestor` or under it. A slice, no allocation.
    pub fn strip_prefix(&self, ancestor: &AppPath) -> Option<&RelativeAppPath> {
        self.debug_check_canonical();
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

    /// [`Self::strip_prefix`], named for readers asking "where is this, from there".
    pub fn relative_to(&self, ancestor: &AppPath) -> Option<&RelativeAppPath> {
        self.strip_prefix(ancestor)
    }

    /// This path moved from under `old_root` to the same place under `new_root` (a remounted
    /// share, a renamed drive), or `None` when it isn't under `old_root`, or when `new_root`
    /// can't hold its names.
    pub fn rebase(&self, old_root: &AppPath, new_root: &AppPath) -> Option<AppPathBuf> {
        new_root.join_relative(self.strip_prefix(old_root)?).ok()
    }

    /// Whether both paths sit on the same root. A UNC server compares without ASCII case (DNS
    /// and NetBIOS names) and its share with Unicode simple case folding (SMB share names are
    /// case-insensitive on the wire); every other root exactly, a scheme root included (every
    /// backend mints its authority lowercase). What a volume pick asks; ❌ never `Eq`, which
    /// stays exact.
    pub fn same_root(&self, other: &AppPath) -> bool {
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
    pub fn strip_volume_prefix(&self, volume_root: &AppPath) -> Option<&RelativeAppPath> {
        self.debug_check_canonical();
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

    /// `name` as a backend spelled it (a `FileEntry.name`): checked, never respelled, so its
    /// escapes stay live.
    ///
    /// # Errors
    ///
    /// [`NotAChildName`] for the empty name, `.`, `..`, a `/`, a NUL outside a pick, or
    /// escapes that aren't canonical.
    pub fn new(name: &str) -> Result<&AppName, NotAChildName> {
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
    /// As [`AppName::new`].
    pub fn from_typed(text: &str, namer: Namer) -> Result<AppNameBuf, NotAChildName> {
        let literal = escape::literal(text, namer);
        AppName::new(&literal).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The app spelling of a name THIS HOST's `OsStr` holds natively (from a `read_dir`, a
    /// `Path::file_name`): every unit the host's decoder rejects escaped, and an escape-range
    /// code point it holds as text escaped as literal. The one door from a `ChildName`, which
    /// lives in the `std::path` world: `AppNameBuf::from_native_os(child.as_os_str())`. ❌ Never
    /// feed it app text (a `FileEntry.name`): that's [`AppName::new`].
    ///
    /// # Errors
    ///
    /// As [`AppName::new`].
    pub fn from_native_os(name: &std::ffi::OsStr) -> Result<AppNameBuf, NotAChildName> {
        let text = platform::os_name_to_app(name);
        AppName::new(&text).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The app spelling of a native name in raw bytes (a Unix disk, SFTP, NFS).
    ///
    /// # Errors
    ///
    /// As [`AppName::new`].
    pub fn from_native_bytes(bytes: &[u8]) -> Result<AppNameBuf, NotAChildName> {
        let text = escape_bytes(bytes);
        AppName::new(&text).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The app spelling of a native name in UTF-16 units (a Windows disk, SMB).
    ///
    /// # Errors
    ///
    /// As [`AppName::new`].
    pub fn from_native_units(units: &[u16]) -> Result<AppNameBuf, NotAChildName> {
        let text = escape_units(units);
        AppName::new(&text).map(|name| AppNameBuf(name.0.to_owned()))
    }

    /// The borrowed name.
    pub fn as_app_name(&self) -> &AppName {
        AppName::new_unchecked(&self.0)
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

// ---- Borrowing and ownership ----

impl Deref for AppPathBuf {
    type Target = AppPath;
    fn deref(&self) -> &AppPath {
        self.as_app_path()
    }
}

impl Borrow<AppPath> for AppPathBuf {
    fn borrow(&self) -> &AppPath {
        self.as_app_path()
    }
}

impl ToOwned for AppPath {
    type Owned = AppPathBuf;
    fn to_owned(&self) -> AppPathBuf {
        AppPathBuf(self.0.to_owned())
    }
}

impl Deref for AppNameBuf {
    type Target = AppName;
    fn deref(&self) -> &AppName {
        self.as_app_name()
    }
}

impl Borrow<AppName> for AppNameBuf {
    fn borrow(&self) -> &AppName {
        self.as_app_name()
    }
}

impl ToOwned for AppName {
    type Owned = AppNameBuf;
    fn to_owned(&self) -> AppNameBuf {
        AppNameBuf(self.0.to_owned())
    }
}

// ---- Comparison: component-wise, as `PathBuf` on POSIX (`shape::components`) ----

impl PartialEq for AppPath {
    fn eq(&self, other: &AppPath) -> bool {
        self.0 == other.0 || shape::components(&self.0).eq(shape::components(&other.0))
    }
}

impl Eq for AppPath {}

impl Hash for AppPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for component in shape::components(&self.0) {
            component.hash(state);
        }
    }
}

impl PartialOrd for AppPath {
    fn partial_cmp(&self, other: &AppPath) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppPath {
    fn cmp(&self, other: &AppPath) -> Ordering {
        if self.0 == other.0 {
            return Ordering::Equal;
        }
        shape::components(&self.0).cmp(shape::components(&other.0))
    }
}

impl PartialEq for AppPathBuf {
    fn eq(&self, other: &AppPathBuf) -> bool {
        self.as_app_path() == other.as_app_path()
    }
}

impl Eq for AppPathBuf {}

impl Hash for AppPathBuf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_app_path().hash(state);
    }
}

impl PartialOrd for AppPathBuf {
    fn partial_cmp(&self, other: &AppPathBuf) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppPathBuf {
    fn cmp(&self, other: &AppPathBuf) -> Ordering {
        self.as_app_path().cmp(other.as_app_path())
    }
}

impl PartialEq<AppPath> for AppPathBuf {
    fn eq(&self, other: &AppPath) -> bool {
        self.as_app_path() == other
    }
}

impl PartialEq<AppPathBuf> for AppPath {
    fn eq(&self, other: &AppPathBuf) -> bool {
        self == other.as_app_path()
    }
}

impl PartialEq for RelativeAppPath {
    fn eq(&self, other: &RelativeAppPath) -> bool {
        self.0 == other.0
    }
}

impl Eq for RelativeAppPath {}

impl Hash for RelativeAppPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for AppName {
    fn eq(&self, other: &AppName) -> bool {
        self.0 == other.0
    }
}

impl Eq for AppName {}

// By the text, the same as `AppNameBuf`'s derived impls, so a `HashMap<AppNameBuf, _>` or a
// `BTreeMap` is looked up by `&AppName` through `Borrow`.
impl Hash for AppName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for AppName {
    fn partial_cmp(&self, other: &AppName) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppName {
    fn cmp(&self, other: &AppName) -> Ordering {
        self.0.cmp(&other.0)
    }
}

// ---- Text ----

/// The wire spelling, for logs (inside `LogPrivate`) and the wire. ❌ Never what a person
/// reads: that's [`AppPath::shown`].
impl fmt::Display for AppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for AppPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AppPath({:?})", &self.0)
    }
}

impl fmt::Debug for AppPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_app_path(), f)
    }
}

impl fmt::Display for RelativeAppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RelativeAppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelativeAppPath({:?})", &self.0)
    }
}

impl fmt::Display for AppName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AppName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AppName({:?})", &self.0)
    }
}

impl fmt::Debug for AppNameBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_app_name(), f)
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
mod tests;
