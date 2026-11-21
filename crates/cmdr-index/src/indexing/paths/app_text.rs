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
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor};
use cmdr_fs::log_private::LogPrivate;

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
///
/// On Windows the PLAIN form (`D:\src`, `\\server\share`) when the root has one, the
/// verbatim one (`\\?\D:\…`) only for a root past what a plain path can spell. The host's
/// mount answers compare `Path`s, and a verbatim prefix never equals a plain one
/// (`\\?\UNC\nas\s` is no child of `\\nas\s`), so a share classified as a local drive and
/// a drive's mount identity went unanswered; every walked child's text inherits the
/// root's form, and only the plain one takes `native_text`'s cheap respelling. `std::fs`
/// adds the verbatim prefix itself for a long path under a plain root. macOS and Linux
/// have no second form, so nothing changes there.
pub(crate) fn native_root(root: &AppPath) -> Option<PathBuf> {
    let canonical = root.to_canonical().ok()?;
    canonical.to_native_plain().or_else(|_| canonical.to_native()).ok()
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
        Err(_) => AppPathBuf::from_wire(text).ok().map(Cow::Owned),
    }
}

/// Stored `text` on a LOCAL volume (the boot disk, a local drive) as the native path the
/// local walker opens: the one crossing for text the local pipeline put in the index, which
/// it spelled from the OS's own names (lossy where a name isn't Unicode text, as the index
/// always has), so on macOS and Linux it opens what it names.
pub(crate) fn local_path(text: &str) -> &Path {
    Path::new(text)
}

/// The text the index stores and compares for a NATIVE path its local pipeline holds (a
/// walked child, a watcher event, a scan or rescan root): the one crossing up from the OS,
/// through [`AppPathBuf::from_native_unicode`], so on Windows `D:\x` is stored as the app
/// spelling `D:/x` the rest of the index (roots, strips, `resolve_path`) splits on `/`.
///
/// On macOS and Linux it IS `to_string_lossy`, by construction: the native text of a
/// Unicode path is its app text (a non-canonical shape such as a trailing `/` kept as
/// spelled, as the index always stored it), and a path the app path can't hold keeps the
/// lossy text, so the hot walk loop pays exactly what it always did. On Windows a path
/// already spelled with `/` is borrowed, and a plain drive path takes a separator swap.
///
/// A name the app path can't hold yet (not Unicode text, or a literal escape-range code
/// point; `cmdr_fs::app_path::ESCAPES_LIVE` is still false) keeps today's lossy spelling,
/// ❌ never dropped: the row stays, as it always has. It's counted where its ROW's name
/// crosses ([`native_name`]), not here: a lossy folder makes every path under it lossy, and
/// the walker crosses each child's path and name both, so counting paths would report one
/// odd name as thousands. At milestone 10f this becomes `from_native`, which escapes it. A
/// path `from_native` refuses (relative, a `..`) is kept as spelled too: the pipeline only
/// meets those from a caller's own literal, and the text is what it always stored.
pub(crate) fn native_text(native: &Path) -> Cow<'_, str> {
    // POSIX: every answer IS the lossy spelling (a Unicode path's native text is its app
    // text; one the app path can't hold keeps the lossy text), so it's spelled that way by
    // construction, with not even the escape scan on top.
    if PathFlavor::HOST != PathFlavor::Windows {
        return native.to_string_lossy();
    }
    if let Some(text) = native.to_str()
        && !holds_escape(text)
        && let Some(spelled) = plain_drive_text(text)
    {
        return spelled;
    }
    match AppPathBuf::from_native_unicode(native) {
        Ok(app) => Cow::Owned(app.into_string()),
        Err(_) => native.to_string_lossy(),
    }
}

/// The text the index stores for a NATIVE name (a walked child's `file_name()`): the name's
/// own text, borrowed, when it is Unicode text holding no escape-range code point (what
/// [`cmdr_fs::app_path::AppNameBuf::from_native_os_unicode`] accepts, and with no
/// allocation); else today's lossy spelling, counted ([`names_kept_lossy`]).
pub(crate) fn native_name(name: &OsStr) -> Cow<'_, str> {
    if let Some(text) = name.to_str()
        && !holds_escape(text)
    {
        return Cow::Borrowed(text);
    }
    note_kept_lossy(name);
    name.to_string_lossy()
}

/// How many times the local pipeline crossed a native name the app path can't hold yet and
/// kept today's lossy spelling, in this process: once per crossing, so a name every rescan
/// meets again counts again. A diagnostic, read by the scan's end log.
pub(crate) fn names_kept_lossy() -> u64 {
    KEPT_LOSSY.load(Ordering::Relaxed)
}

static KEPT_LOSSY: AtomicU64 = AtomicU64::new(0);

fn note_kept_lossy(native: &OsStr) {
    if KEPT_LOSSY.fetch_add(1, Ordering::Relaxed) == 0 {
        log::info!(
            "Index: {} isn't plain Unicode text, so its row keeps the lossy spelling (counted, not dropped)",
            LogPrivate::path(Path::new(native))
        );
    }
}

/// A plain Windows drive path (`D:\src\x`, the walker's every child; `D:/src/x` borrowed) in
/// the app spelling, by swapping its separators: exactly what `from_native_unicode` answers
/// for it (a Windows name holds no `/` or `\`), at the cost of one byte scan and, for a `\`
/// path, one copy, where the full crossing (and `from_normalized`'s check) measured about
/// 1 µs and 0.8 µs a path (macOS 27 proxy over 1,000,000 real paths spelled `D:\…`,
/// 2026-11-21); a verbatim `\\?\` drive path is read the same. `None` for any other shape (a UNC lead, a lowercase
/// drive, an empty, `.`, or `..` name), which takes the full crossing.
fn plain_drive_text(text: &str) -> Option<Cow<'_, str>> {
    // A verbatim drive path (`\\?\D:\x`, what `canonicalize` and a long root hand back)
    // names the same place; NT reads its `/` as a name character, so one holding any takes
    // the full crossing.
    let text = match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.contains('/') => return None,
        Some(rest) => rest,
        None => text,
    };
    let bytes = text.as_bytes();
    let [letter, b':', b'\\' | b'/', ..] = bytes else {
        return None;
    };
    if !letter.is_ascii_uppercase() {
        return None;
    }
    let names = &text[3..];
    if !names.is_empty() && names.split(['\\', '/']).any(|name| matches!(name, "" | "." | "..")) {
        return None;
    }
    Some(if bytes.contains(&b'\\') {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        Cow::Borrowed(text)
    })
}

/// Whether `text` holds an escape-range code point. Every one is in plane 16, whose UTF-8
/// starts with `0xF4`, so nearly every path answers from one byte scan.
fn holds_escape(text: &str) -> bool {
    text.as_bytes().contains(&0xF4) && text.chars().any(cmdr_fs::app_path::is_escape)
}

#[cfg(test)]
#[path = "app_text_tests.rs"]
mod tests;
