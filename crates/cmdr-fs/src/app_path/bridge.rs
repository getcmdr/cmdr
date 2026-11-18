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

use std::borrow::Cow;
use std::path::Path;

use super::{AppPath, AppPathBuf, platform};

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
