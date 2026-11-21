//! Archive-boundary detection: does a path cross into a supported archive?
//!
//! The transparent-path model presents a `.zip` as a folder: navigating to
//! `/path/to/foo.zip/inner` should route to the read-only `ArchiveVolume` for
//! `foo.zip` and list `inner` inside it. This module owns the ONE detector both
//! `VolumeManager::resolve` and `commands/volumes.rs` share, so the pane label
//! and the I/O target can never disagree (two drifting detectors is the failure
//! this centralizes away).
//!
//! Two tiers, by cost:
//!
//! - [`archive_boundary_candidate`] / [`has_supported_archive_extension`] are
//!   pure string checks (no I/O), both in `cmdr_fs::archive_format`. Extension-only is deliberate: it's what
//!   `FileEntry.is_archive` uses at listing time, where a magic-byte sniff per
//!   entry would be a round-trip-per-file on a remote backend (principles 3/5).
//! - [`confirm_archive_boundary`] adds the I/O the string check can't do: it
//!   confirms the candidate component is a real FILE (a directory literally
//!   named `foo.zip` must lose to normal directory navigation) and that its
//!   first bytes are a zip signature (a mislabeled file isn't routed). This runs
//!   once per navigation, only when a component carries an archive extension.
//!   It's `std::fs`-only; the parent-aware confirm a host routes with (local or
//!   through a remote parent's own I/O) is `crate::boundary_on_parent`'s.

use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppPath, AppPathBuf, NativeRefusal, PathArithmetic, RelativeAppPathBuf};

use super::read::{ArchiveFormat, TarCodec, format_for_name, format_for_path};

/// Longest header prefix any format's magic check needs. A plain `.tar`'s ustar
/// magic sits at offset 257, so one 512-byte tar block covers every format
/// (zip/gzip/bzip2/xz/zstd/7z magics all live at offset 0).
pub(crate) const ARCHIVE_MAGIC_PREFIX_LEN: usize = 512;

// The extension predicate and the extension-only split live in
// `cmdr_fs::archive_format` alongside `format_for_name`, the suffix table both
// delegate to: `FileEntry::new` sets `is_archive` from the predicate, and the
// listing engine asks the split without depending on this crate. The predicate is
// re-exported so this module stays its import site; the split is named at
// `cmdr_fs::archive_format` everywhere.
pub use cmdr_fs::archive_format::has_supported_archive_extension;
use cmdr_fs::archive_format::{archive_boundary_candidate, archive_boundary_candidate_at, archive_name_text};

/// Whether `header` (a file's first bytes, ideally [`ARCHIVE_MAGIC_PREFIX_LEN`]
/// long) carries the magic signature for `format`. Short slices simply fail to
/// match (never panic), so a truncated read declines the route rather than
/// mis-detecting. Shared by the local sniff and the REMOTE confirm
/// (`crate::boundary_on_parent`), so both agree on what "is a `<format>`" means.
pub(crate) fn bytes_match_archive_magic(format: ArchiveFormat, header: &[u8]) -> bool {
    match format {
        // A document container carries the same `PK\x03\x04` header, so the
        // magic confirm is the zip one; only writability differs.
        ArchiveFormat::Zip | ArchiveFormat::Ooxml => bytes_start_with_zip_signature(header),
        // A plain tar has no signature at offset 0; the ustar magic sits at 257.
        // (A pre-POSIX v7 tar has none at all — those are accepted by extension +
        // a successful parse rather than magic, but modern tars are ustar/GNU/pax.)
        ArchiveFormat::Tar(TarCodec::Plain) => matches!(header.get(257..262), Some(b"ustar")),
        ArchiveFormat::Tar(TarCodec::Gzip) => header.starts_with(&[0x1f, 0x8b]),
        ArchiveFormat::Tar(TarCodec::Bzip2) => header.starts_with(b"BZh"),
        ArchiveFormat::Tar(TarCodec::Xz) => header.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]),
        ArchiveFormat::Tar(TarCodec::Zstd) => header.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]),
        ArchiveFormat::SevenZ => header.starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]),
    }
}

/// The I/O-backed boundary check used at navigation / resolve time.
///
/// Runs [`archive_boundary_candidate`], then confirms the candidate is a real
/// archive: (a) the component is a FILE — a directory literally named `foo.zip`
/// must lose to normal directory navigation — and (b) its bytes carry the
/// format's magic ([`bytes_match_archive_magic`]), so a mislabeled non-archive
/// isn't routed. Returns the split only when both hold.
///
/// Blocking (a local stat + a few-byte read). Callers on the async executor
/// accept it because it runs once per navigation and only when a path component
/// carries an archive extension (the pure candidate check gates the I/O).
/// This is `std::fs`-only, so it answers for a LOCAL parent alone; a host asks
/// `confirm_archive_boundary_on_parent`, which forks to a remote parent's own
/// I/O (sharing this module's magic predicate).
pub(crate) fn confirm_archive_boundary(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let (archive_path, inner_path) = archive_boundary_candidate(path)?;
    let metadata = std::fs::metadata(&archive_path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let format = format_for_path(&archive_path)?;
    if !file_matches_archive_magic(&archive_path, format) {
        return None;
    }
    Some((archive_path, inner_path))
}

/// Whether `path` crosses into a confirmed supported archive — the `.zip` file
/// ITSELF (empty inner) OR a path inside it. This is the "enter the archive"
/// predicate: navigation and listing route the `.zip` path here to browse the
/// archive root.
///
/// For write/copy/viewer sites that operate ON a path, prefer
/// [`path_is_inside_archive`]: those must treat the `.zip` file itself as a
/// normal file (copy/move/rename/view it), and only refuse or route paths
/// genuinely INSIDE the archive.
pub fn path_crosses_archive_boundary(path: &Path) -> bool {
    confirm_archive_boundary(path).is_some()
}

/// Whether `path` points at something INSIDE a confirmed archive (a non-empty
/// inner path like `foo.zip/entry`), as opposed to the `.zip` file itself.
///
/// This is the predicate for sites that operate ON a path rather than navigate
/// INTO it: copy/move/delete/rename guards, source routing, drag locality, and
/// the viewer. The `.zip` file itself is a regular file — copying, moving,
/// renaming, or viewing it must behave exactly like any other file — so those
/// sites must NOT treat it as archive-internal. Only a genuinely-inner path is
/// read-only / extract-routed / non-materializable.
pub fn path_is_inside_archive(path: &Path) -> bool {
    confirm_archive_boundary(path).is_some_and(|(_zip, inner)| !inner.as_os_str().is_empty())
}

/// True when a path targets an archive FILE Cmdr tried to browse — the archive
/// component exists and is a regular file (not a directory that merely shares the
/// name). Path + a cheap stat, no magic check: the caller pairs it with the
/// listing error kind to decide whether a FAILED listing was a damaged/mislabeled
/// archive (`NotSupported`/`IoError`) versus a valid archive with a missing inner
/// path (`NotFound`). A real directory named `foo.zip` returns false — it lists
/// like any folder.
pub fn path_targets_archive_file(path: &Path) -> bool {
    let Some((archive_path, _inner)) = archive_boundary_candidate(path) else {
        return false;
    };
    std::fs::metadata(&archive_path).is_ok_and(|meta| meta.is_file())
}

/// The archive format `path`'s last name denotes, read as its native text, as
/// [`format_for_path`] reads a native one.
pub(crate) fn format_at(path: &AppPath) -> Option<ArchiveFormat> {
    format_for_name(&archive_name_text(path.file_name()?)?)
}

/// [`confirm_archive_boundary`] for an app path: the split when `path` crosses
/// into a confirmed archive on this machine's disk. The archive file crosses to
/// the OS at [`AppPath::to_native`]; a path the OS can't open (a scheme path) is
/// no archive here.
pub fn confirm_archive_boundary_at(path: &AppPath) -> Option<(AppPathBuf, RelativeAppPathBuf)> {
    let (archive, inner) = archive_boundary_candidate_at(path)?;
    confirms_on_disk(&archive).then_some((archive, inner))
}

/// Whether the archive at `archive` is a real file on this machine's disk
/// carrying its format's magic.
pub(crate) fn confirms_on_disk(archive: &AppPath) -> bool {
    let Ok(native) = native_of(archive) else {
        return false;
    };
    if !std::fs::metadata(&native).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    format_at(archive).is_some_and(|format| file_matches_archive_magic(&native, format))
}

/// Where the local backing file of an archive at `path` lives on this machine:
/// [`AppPath::to_native`], the one crossing. A browser build has no local files,
/// so every path there is `NotLocal`.
pub(crate) fn native_of(path: &AppPath) -> Result<PathBuf, NativeRefusal> {
    #[cfg(not(target_family = "wasm"))]
    return path.to_native();
    #[cfg(target_family = "wasm")]
    {
        let _ = path;
        Err(NativeRefusal::NotLocal)
    }
}

/// [`path_crosses_archive_boundary`] for an app path.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "a typed twin the host switches to when its routing speaks app paths"
    )
)]
pub(crate) fn path_crosses_archive_boundary_at(path: &AppPath) -> bool {
    confirm_archive_boundary_at(path).is_some()
}

/// [`path_is_inside_archive`] for an app path.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "a typed twin the host switches to when its routing speaks app paths"
    )
)]
pub(crate) fn path_is_inside_archive_at(path: &AppPath) -> bool {
    confirm_archive_boundary_at(path).is_some_and(|(_archive, inner)| !inner.is_empty())
}

/// [`path_targets_archive_file`] for an app path.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "a typed twin the host switches to when its routing speaks app paths"
    )
)]
pub(crate) fn path_targets_archive_file_at(path: &AppPath) -> bool {
    let Some((archive, _inner)) = archive_boundary_candidate_at(path) else {
        return false;
    };
    native_of(&archive).is_ok_and(|native| std::fs::metadata(native).is_ok_and(|meta| meta.is_file()))
}

/// Reads up to [`ARCHIVE_MAGIC_PREFIX_LEN`] bytes and checks them against
/// `format`'s magic ([`bytes_match_archive_magic`]). Best-effort read: a file
/// shorter than the prefix (a tiny zip) yields fewer bytes, and the magic check
/// tolerates that. A file that can't be opened isn't an archive.
fn file_matches_archive_magic(path: &Path, format: ArchiveFormat) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; ARCHIVE_MAGIC_PREFIX_LEN];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return false,
        }
    }
    bytes_match_archive_magic(format, &buf[..filled])
}

/// Whether `bytes` (a file's first four bytes) match one of the three zip
/// start-of-file signatures: a local file header (`PK\x03\x04`), an empty
/// archive's end-of-central-directory (`PK\x05\x06`), or the spanned-archive
/// marker (`PK\x07\x08`). Fewer than four bytes isn't a zip.
///
/// The zip arm of [`bytes_match_archive_magic`], which both the local sniff and
/// the remote confirm go through.
fn bytes_start_with_zip_signature(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some(b"PK\x03\x04") | Some(b"PK\x05\x06") | Some(b"PK\x07\x08")
    )
}

#[cfg(test)]
#[path = "boundary_test.rs"]
mod tests;
