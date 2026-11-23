//! OS junk files: files an operating system drops beside a person's own, which a
//! transfer can leave out on request (#314, #1305). The set is the RUNNING OS's
//! own, so the copy dialog's one toggle means "skip this system's junk" on each:
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
//! always goes. The matching logic takes the OS as a value, so every OS's set is
//! tested on every OS; only [`JunkOs::current`] reads the target, without `cfg`.
//!
//! Not the same rule as [`crate::apple_double`]'s, which leaves out only a `._`
//! sidecar the KERNEL serves as its sibling's xattrs (so a copy that carries the
//! xattrs would land it twice). That one always applies; this one is the
//! person's choice per operation.

use std::ffi::OsStr;

use serde::{Deserialize, Serialize};

/// Whether a transfer writes the running OS's junk files to its destination.
// DEFAULT-OK: `Include` claims nothing about the disk; it's what every transfer did
// before the person could ask otherwise, and what a caller that never asks gets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum OsJunkFiles {
    /// Copy them like any other file. The default: leaving a file out is
    /// something a person asks for.
    #[default]
    Include,
    /// Leave every listed junk file of the running OS out of the destination.
    Skip,
}

impl OsJunkFiles {
    /// Whether a walk under this choice leaves out the entry called `name`, by
    /// the running OS's set. `is_directory` is the listing's own word: a folder
    /// never matches.
    pub fn leaves_out(self, name: &OsStr, is_directory: bool) -> bool {
        self.leaves_out_on(JunkOs::current(), name, is_directory)
    }

    /// [`Self::leaves_out`] for a name a listing hands over as a `str`.
    pub fn leaves_out_str(self, name: &str, is_directory: bool) -> bool {
        self.leaves_out(OsStr::new(name), is_directory)
    }

    /// [`Self::leaves_out`] by `os`'s set: the seam that lets every set be tested
    /// on every OS.
    pub fn leaves_out_on(self, os: JunkOs, name: &OsStr, is_directory: bool) -> bool {
        self == Self::Skip && !is_directory && is_os_junk_file_name(os, name)
    }

    /// Whether a zip leaves out the `__MACOSX` `._` sidecars it would build from
    /// an item's xattrs. Those are the same AppleDouble clutter as a `._name`
    /// file, so only skipping macOS's set drops them; skipping Windows' or
    /// Linux's keeps the xattrs.
    pub fn leaves_out_apple_double_sidecars(self) -> bool {
        self.leaves_out_apple_double_sidecars_on(JunkOs::current())
    }

    /// [`Self::leaves_out_apple_double_sidecars`] on `os`.
    pub fn leaves_out_apple_double_sidecars_on(self, os: JunkOs) -> bool {
        self == Self::Skip && os == JunkOs::MacOs
    }
}

/// The OS whose junk set a transfer skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JunkOs {
    /// `.DS_Store` and `._*`.
    MacOs,
    /// `Thumbs.db`, `ehthumbs.db`, and `ehthumbs_vista.db`, in any case.
    Windows,
    /// `.nfs…` and `.fuse_hidden…` leftovers of deleted open files.
    Linux,
}

impl JunkOs {
    /// Every OS, for tests that walk them all.
    pub const ALL: [Self; 3] = [Self::MacOs, Self::Windows, Self::Linux];

    /// The OS this binary runs on, from the target's own OS name, so the matcher
    /// stays free of `cfg`. Anything that isn't macOS or Windows (Linux, and the
    /// browser demo's wasm build) answers Linux.
    pub const fn current() -> Self {
        match std::env::consts::OS.as_bytes() {
            b"macos" => Self::MacOs,
            b"windows" => Self::Windows,
            _ => Self::Linux,
        }
    }

    /// Two names this OS's set matches, for a test that writes junk files on
    /// whichever OS it runs on and checks a transfer leaves them out.
    #[cfg(any(test, feature = "testing"))]
    pub const fn sample_junk_names(self) -> [&'static str; 2] {
        match self {
            Self::MacOs => [".DS_Store", "._photo.jpg"],
            Self::Windows => ["Thumbs.db", "ehthumbs.db"],
            Self::Linux => [".nfs000000000012abcd00000001", ".fuse_hidden0000000a00000001"],
        }
    }
}

/// Whether `name` is in `os`'s junk set. See the module docs for each set.
pub fn is_os_junk_file_name(os: JunkOs, name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    match os {
        JunkOs::MacOs => is_mac_junk(bytes),
        JunkOs::Windows => is_windows_junk(bytes),
        JunkOs::Linux => is_linux_junk(bytes),
    }
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
