//! The index's crossings from an app path (a volume's root, as its `Volume` names it) to
//! what the index holds: the TEXT its stores, path spaces, and exclusion rules compare, and
//! the NATIVE path its local walker and watcher open.
//!
//! The index keeps text in its stores (`volume_path` meta, mount roots, read-side strips),
//! and that text is the lossy spelling it has always stored. Until escapes go live
//! (`cmdr_fs::app_path::ESCAPES_LIVE`, the app path plan's milestone 10f) only the transition
//! bridge spells one, for a mount point that isn't Unicode text, so [`stored_text`] decodes
//! it back to today's bytes: byte for byte what `as_std_legacy().to_string_lossy()` answered.
//! At 10f it becomes the root's own text.

use std::borrow::Cow;
use std::path::PathBuf;

use cmdr_fs::app_path::AppPath;

/// The text the index stores and compares for `path`: its own text, or, for one holding an
/// escape, the lossy spelling of the native name it stands for, as the index stored it
/// before app paths: U+FFFD per invalid byte (a Unix name's byte escapes, the only ones a
/// Unix host spells) or per unpaired surrogate (a Windows name's unit escapes). Pure, so
/// one body on every host: each escape decodes through its own alphabet, and text mixing
/// both, which no namer makes, stays as it is.
pub(crate) fn stored_text(path: &AppPath) -> Cow<'_, str> {
    let text = path.as_str();
    match cmdr_fs::app_path::unescape_bytes(text) {
        Ok(Cow::Borrowed(_)) => Cow::Borrowed(text),
        Ok(Cow::Owned(bytes)) => Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => match cmdr_fs::app_path::unescape_units(text) {
            Ok(units) => Cow::Owned(String::from_utf16_lossy(&units)),
            Err(_) => Cow::Borrowed(text),
        },
    }
}

/// Where a volume rooted at `root` sits on this machine's disk, for the local walker, the
/// watcher, and the host's space and mount answers: `root`'s one native crossing.
///
/// `None` for a root no OS opens: a phone's `mtp://` or `adb://` root, which the index walks
/// over the `Volume` trait alone, or one holding a `..`.
///
/// A root is the bridge's unnormalized text of what its registration got
/// (`LocalPosixVolume::on_mount`), which `to_native` refuses as not canonical, so it
/// crosses as its canonical twin: a trailing `/`, `//`, or `.` folds to the place the OS
/// opens for that text, where the local walker ran before the root was typed. ❌ Never a
/// `None` for those: a local drive would fall through to the SMB gate and never index.
pub(crate) fn native_root(root: &AppPath) -> Option<PathBuf> {
    root.to_canonical().ok()?.to_native().ok()
}

/// Stored `text` (an absolute path the index holds, such as a frontier root on a share) as
/// the app path a `Volume` method takes: the text itself when it's canonical, which every
/// row a backend lists is, else the canonical place it names. `None` for text that names no
/// place (no root, a `..`), and for non-canonical text holding an escape-range code point.
///
/// On a trait-scanned volume (a share, a phone) the stored text IS the listed rows' own text
/// (`FileEntry.path` and `.name`, as the backend spelled them), so reading it back as app
/// spelling names the very row it came from.
///
/// ❗ The respelling may fold separators (`//`, `/./`, a trailing `/`), which names the same
/// place, but never escapes: canonicalizing the two byte escapes of `é`'s UTF-8 spells
/// `xé`, a DIFFERENT folder, whose children a walk would file under the literal folder's
/// row (#1310). Such text names no place a walk may list.
pub(crate) fn app_path_of(text: &str) -> Option<Cow<'_, AppPath>> {
    match AppPath::from_normalized(text) {
        Ok(path) => Some(Cow::Borrowed(path)),
        Err(_) if text.chars().any(cmdr_fs::app_path::is_escape) => None,
        Err(_) => cmdr_fs::app_path::AppPathBuf::from_wire(text).ok().map(Cow::Owned),
    }
}

/// Stored `text` on a LOCAL volume (the boot disk, a local drive) as the native path the
/// local walker opens: the one crossing for text the local pipeline put in the index, which
/// it spelled from the OS's own names (lossy where a name isn't Unicode text, as the index
/// always has), so on macOS and Linux it opens what it names.
pub(crate) fn local_path(text: &str) -> &std::path::Path {
    std::path::Path::new(text)
}

#[cfg(test)]
#[path = "app_text_tests.rs"]
mod tests;
