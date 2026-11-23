//! The transition bridge: the one lossless, NON-normalizing door between `std::path` and an
//! app path, for the conversion while the `Volume` trait speaks app paths and the bodies
//! behind it still do `std::path` arithmetic.
//!
//! - [`AppPath::legacy_unchecked`] wraps a `Path`'s text AS IS: unvalidated, unnormalized
//!   (a trailing `/`, `//`, `.`, `..`, and a Windows `\` all survive). A name that isn't
//!   valid Unicode text is escaped, ❗ never spelled lossily: the local engine's walks hand
//!   raw `read_dir` paths to code that calls `Volume` methods, and a lossy bridge would make
//!   those calls miss the file, or hit a sibling literally named with U+FFFD.
//! - [`AppPath::as_std_legacy`] gives the exact native bytes back.
//!
//! Proven, not hoped: `legacy_unchecked(p).as_std_legacy()` has `p`'s exact `OsStr` bytes
//! (Unix bytes, Windows UTF-16 units) for arbitrary `p` (the bridge property tests). ❌ Never
//! assert that with `Path ==`, which ignores a trailing `/` and `.`.
//!
//! Every call is counted per file by the `app-path-std-bridge` check, and both functions go
//! once that count reaches zero.
//!
//! [`BridgedPath`] is the bridge for a path a `Volume` RETURNS by reference (`root`,
//! `stands_in_for_own_disk`, `local_backing_file`): `legacy_unchecked` answers a `Cow`, which
//! can't be returned as `&AppPath` (E0515), so the field holds both spellings, made once.

use std::borrow::Cow;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use super::{AppName, AppPath, AppPathBuf, NotAnAppPath, platform};

/// A native path held beside its app spelling, both made once at construction, so a `Volume`
/// can hand out `&AppPath` for a field (`fn root(&self) -> &AppPath { self.root.app() }`) while
/// every body use of the field keeps reading `std::path` through `Deref<Target = Path>`
/// unchanged. The milestone 4 codemod retypes such a field from `PathBuf`; each crate's own
/// milestone turns it into a plain `AppPathBuf`, and the type goes with the bridge
/// (milestone 11). Lossless: the app side is [`AppPath::legacy_unchecked`]'s, so a non-UTF-8
/// mount point survives. Counted by `app-path-std-bridge` like the bridge's two functions.
#[derive(Debug, Clone)]
pub struct BridgedPath {
    native: PathBuf,
    app: AppPathBuf,
}

impl BridgedPath {
    /// `native`, with its app spelling made now.
    pub fn new(native: PathBuf) -> BridgedPath {
        let app = AppPath::legacy_unchecked(&native).into_owned();
        BridgedPath { native, app }
    }

    /// The app spelling: what a `Volume` method returns.
    pub fn app(&self) -> &AppPath {
        &self.app
    }

    /// The native path, exactly as given.
    pub fn as_path(&self) -> &Path {
        &self.native
    }

    /// The native path, owned.
    pub fn into_path_buf(self) -> PathBuf {
        self.native
    }
}

/// A native-side value: it reads as the `Path` it holds, so a body's `self.root.join(x)`
/// compiles unchanged. ❗ Never the other way: the app side is only [`BridgedPath::app`].
impl Deref for BridgedPath {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.native
    }
}

impl AsRef<Path> for BridgedPath {
    fn as_ref(&self) -> &Path {
        &self.native
    }
}

impl AppPath {
    /// `path` as an app path, unchecked and unnormalized; borrowed when its text is valid
    /// UTF-8 holding no escape-range code point. The bridge's way in.
    pub fn legacy_unchecked(path: &Path) -> Cow<'_, AppPath> {
        match platform::os_name_to_app(path.as_os_str()) {
            Cow::Borrowed(text) => Cow::Borrowed(AppPath::new_unchecked(text)),
            Cow::Owned(text) => Cow::Owned(AppPathBuf(text)),
        }
    }

    /// This path as a `std::path::Path`, every escape decoded back to its native byte or
    /// unit; borrowed when there's none. The bridge's way out. An escape of the alphabet this
    /// host can't spell stays as its code point's text: only `to_native`'s typed refusal is
    /// right for that, and no bridge value holds one.
    pub fn as_std_legacy(&self) -> Cow<'_, Path> {
        match platform::app_to_os(self.as_str()) {
            Cow::Borrowed(os) => Cow::Borrowed(Path::new(os)),
            Cow::Owned(os) => Cow::Owned(os.into()),
        }
    }
}

/// A folder its listed names join under (a walk's level, a patched listing's parent), its
/// join decided ONCE for every name it takes:
///
/// - A folder with a canonical twin joins as that canonical place, by the app path's own
///   join, so a remote or in-memory child never gains a Windows `\` and `/dot/.` or `/top/`
///   lose their spelling.
/// - A bridge value with none (a relative `src/album` a local engine hands its backend, a
///   `..` spelling) joins as POSIX's `Path::join` joined it, by text with a `/`: the typed
///   join has no place to answer, and refusing would drop a walk's children or a pane's
///   patch. By text so a Windows host joins with the app's `/` too, where its `Path::join`
///   put a `\` into the app path (`rel/dir\x`).
///
/// Goes with the bridge, once no caller hands a walk or a patch a folder with no canonical
/// twin.
pub struct ListedFolder<'a>(Joins<'a>);

enum Joins<'a> {
    Typed(Cow<'a, AppPath>),
    Bridge(&'a AppPath),
}

impl<'a> ListedFolder<'a> {
    /// `dir`, its canonical twin looked up now.
    pub fn new(dir: &'a AppPath) -> Self {
        match dir.to_canonical() {
            Ok(canonical) => Self(Joins::Typed(canonical)),
            Err(_) => Self(Joins::Bridge(dir)),
        }
    }

    /// `name` in this folder.
    ///
    /// # Errors
    ///
    /// What [`PathArithmetic::join`](super::PathArithmetic::join) refuses under a canonical folder (a drive or UNC folder takes
    /// no name holding `\`). A bridge folder's join refuses nothing, as `Path::join` didn't.
    pub fn join(&self, name: &AppName) -> Result<AppPathBuf, NotAnAppPath> {
        match &self.0 {
            Joins::Typed(dir) => dir.join_canonical_text(name.as_str()),
            Joins::Bridge(dir) => Ok(AppPathBuf(bridge_join_text(dir.as_str(), name.as_str()))),
        }
    }
}

/// `name` under the bridge folder `dir`, as POSIX's `Path::join` spells it: no separator after
/// an empty folder or one that already ends in one. Escapes ride as text, so on a Unix host
/// this is exactly the bridge round trip of `Path::join`. On Windows a folder spelled with `/`
/// alone (`rel/dir`, a remote root) joins with `/` too, and one still spelled natively (it
/// holds a `\`) keeps `Path::join`'s `\`.
fn bridge_join_text(dir: &str, name: &str) -> String {
    let native = cfg!(windows) && dir.contains('\\');
    let ends_in_separator = dir.ends_with('/') || (native && dir.ends_with('\\'));
    if dir.is_empty() || ends_in_separator {
        format!("{dir}{name}")
    } else if native {
        format!("{dir}\\{name}")
    } else {
        format!("{dir}/{name}")
    }
}
