//! OS junk files: files an operating system drops beside a person's own, which a
//! transfer can leave out on request (#314, #1305, #1416). The set is ONE merged
//! set, the same on every OS, since a USB stick or a NAS share carries what every
//! system that touched it wrote:
//!
//! - **macOS**: Finder's `.DS_Store` in every folder it shows, and the `._name`
//!   AppleDouble files macOS writes beside items on volumes that can't hold
//!   xattrs. Both mean something only to a Mac.
//! - **Windows**: the thumbnail caches File Explorer (`Thumbs.db`) and Windows
//!   Media Center (`ehthumbs.db`, `ehthumbs_vista.db`) write and rebuild on demand.
//! - **Linux**: the `.nfs…` and `.fuse_hidden…` names NFS and FUSE give a file
//!   that was deleted while still open; they vanish once the last handle closes.
//!
//! What's NOT here, and why (each a folder setting, the person's data, or a
//! folder; the rule is "regenerated cache or OS dropping, never data or
//! settings"): Windows' `desktop.ini` (a folder's custom icon and display name),
//! `$RECYCLE.BIN`, and `System Volume Information`; KDE's `.directory` (Dolphin's
//! per-folder view and a custom icon), the XDG `.Trash-<uid>` folder (trashed
//! files), and `lost+found`. Sources and dates: `DETAILS.md` § "OS junk files".
//!
//! The match is on the NAME, typed here once, so every walk (the local scan, each
//! backend's copy scan, the cross-volume merge, the compress plan) asks the same
//! question. Only a FILE matches: a folder named `._x` is the person's own and
//! always goes. The one per-OS part is the zip's `__MACOSX` sidecars, which only
//! a Mac builds ([`OsJunkFiles::leaves_out_apple_double_sidecars`]).
//!
//! Not the same rule as [`crate::apple_double`]'s, which leaves out only a `._`
//! sidecar the KERNEL serves as its sibling's xattrs (so a copy that carries the
//! xattrs would land it twice). That one always applies; this one is the
//! person's choice per operation.

use std::ffi::OsStr;

use serde::{Deserialize, Serialize};

/// Whether a transfer writes OS junk files to its destination.
// DEFAULT-OK: `Include` claims nothing about the disk; it's what every transfer did
// before the person could ask otherwise, and what a caller that never asks gets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum OsJunkFiles {
    /// Copy them like any other file. The default: leaving a file out is
    /// something a person asks for.
    #[default]
    Include,
    /// Leave every listed junk file, whichever OS's, out of the destination.
    Skip,
}

impl OsJunkFiles {
    /// Whether a walk under this choice leaves out the entry called `name`.
    /// `is_directory` is the listing's own word: a folder never matches.
    pub fn leaves_out(self, name: &OsStr, is_directory: bool) -> bool {
        self == Self::Skip && !is_directory && is_os_junk_file_name(name)
    }

    /// [`Self::leaves_out`] for a name a listing hands over as a `str`.
    pub fn leaves_out_str(self, name: &str, is_directory: bool) -> bool {
        self.leaves_out(OsStr::new(name), is_directory)
    }

    /// Whether a zip leaves out the `__MACOSX` `._` sidecars it would build from
    /// an item's xattrs. Those are the same AppleDouble clutter as a `._name`
    /// file, so skipping on a Mac drops them; elsewhere skipping keeps the xattrs.
    pub fn leaves_out_apple_double_sidecars(self) -> bool {
        self.leaves_out_apple_double_sidecars_on(RUNNING_ON_MACOS)
    }

    /// [`Self::leaves_out_apple_double_sidecars`] as if on a Mac or not: the seam
    /// that tests both answers on every OS.
    pub fn leaves_out_apple_double_sidecars_on(self, on_macos: bool) -> bool {
        self == Self::Skip && on_macos
    }
}

/// Whether this binary runs on macOS, from the target's own OS name, so the
/// module stays free of `cfg`.
pub const RUNNING_ON_MACOS: bool = matches!(std::env::consts::OS.as_bytes(), b"macos");

/// Junk names from all three OSes' sets, for a test that writes junk files and
/// checks a transfer leaves them out on whichever OS it runs on. The first is a
/// `._*` PREFIX match, the one rule that can meet a real person's name and runs
/// beside the AppleDouble sidecar rule, so tests put it in every folder.
#[cfg(any(test, feature = "testing"))]
pub const SAMPLE_JUNK_NAMES: [&str; 4] = ["._photo.jpg", "Thumbs.db", ".DS_Store", ".nfs000000000012abcd00000001"];

/// Whether `name` is in the merged junk set. See the module docs for each part.
pub fn is_os_junk_file_name(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    is_mac_junk(bytes) || is_windows_junk(bytes) || is_linux_junk(bytes)
}

/// Exactly `.DS_Store`, or `._` followed by at least one more byte. Byte-exact:
/// Finder writes `.DS_Store` in this spelling everywhere, so a `.ds_store` is
/// somebody's own file.
fn is_mac_junk(bytes: &[u8]) -> bool {
    bytes == b".DS_Store" || bytes.strip_prefix(b"._").is_some_and(|rest| !rest.is_empty())
}

/// The three thumbnail-cache names, ASCII-case-insensitively: Windows file
/// systems ignore case, so `thumbs.db` is the same file to Explorer.
fn is_windows_junk(bytes: &[u8]) -> bool {
    [&b"Thumbs.db"[..], b"ehthumbs.db", b"ehthumbs_vista.db"]
        .iter()
        .any(|junk| bytes.eq_ignore_ascii_case(junk))
}

/// The exact shapes the kernel and libfuse write, so a person's `.nfsconfig`
/// stays theirs: `.nfs` + 24 lowercase hex digits (`fs/nfs/unlink.c`,
/// `SILLYNAME_PREFIX "%0*llx%0*x"`), and `.fuse_hidden` + 16 lowercase hex digits
/// (libfuse `lib/fuse.c`, `".fuse_hidden%08x%08x"`).
fn is_linux_junk(bytes: &[u8]) -> bool {
    let is_lower_hex = |rest: &[u8], len: usize| {
        rest.len() == len && rest.iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    };
    bytes.strip_prefix(b".nfs").is_some_and(|rest| is_lower_hex(rest, 24))
        || bytes
            .strip_prefix(b".fuse_hidden")
            .is_some_and(|rest| is_lower_hex(rest, 16))
}

#[cfg(test)]
#[path = "os_junk_files_test.rs"]
mod tests;
