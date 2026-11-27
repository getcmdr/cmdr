//! macOS: `NSFileManager.trashItemAtURL`, one Trash per volume.

use std::fs;
use std::path::{Path, PathBuf};

use super::super::super::super::mutation_error::MutationError;
use super::super::super::super::types::TrashRefusalKind;
use super::super::{StickyFacts, sticky_rule_refuses};

/// Synchronous trash implementation using macOS NSFileManager.trashItem.
///
/// Returns the item's **in-trash location** (`Some` on macOS, where the OS
/// reports it): the journal records it as the trash row's dest so a later restore
/// knows where the OS put the item (the trash rollback depends on it). `None`
/// means "trashed, but no restore location recorded" (Linux, or the rare case the
/// OS omitted the URL).
///
/// Uses `symlink_metadata()` for existence checks so dangling symlinks
/// are handled correctly (the link itself exists even if its target doesn't).
///
/// The macOS in-trash URL comes from `trashItemAtURL:resultingItemURL:error:`,
/// which populates the out-param with the final URL inside the user's Trash
/// (verified on macOS 15, live trash of a temp file returns a `~/.Trash/…` path,
/// 2026-07-10). NSFileManager may de-duplicate the name (`file 2.txt`) if the
/// Trash already holds one, so the returned location is the authoritative one.
pub fn move_to_trash_sync(path: &Path) -> Result<Option<PathBuf>, MutationError> {
    use objc2::rc::{Retained, autoreleasepool};
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    if fs::symlink_metadata(path).is_err() {
        return Err(MutationError::NotFound {
            path: path.display().to_string(),
        });
    }

    // Drain autoreleased ObjC objects (NSURL, NSString, NSFileManager internals).
    // Called from spawn_blocking threads that lack AppKit's autorelease pool.
    autoreleasepool(|_| {
        let path_str = path.to_string_lossy();
        let ns_path = NSString::from_str(&path_str);
        let url = NSURL::fileURLWithPath(&ns_path);
        let file_manager = NSFileManager::defaultManager();

        // Capture resultingItemURL (the final location inside Trash) so the
        // journal can record where to restore from (the trash rollback).
        let mut resulting: Option<Retained<NSURL>> = None;
        file_manager
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
            .map_err(|e| MutationError::TrashRefused {
                reason: refine_permission_refusal(path, classify_trash_refusal(&e)),
                detail: e.to_string(),
                // The words are the user's, but the codes (the underlying one most of
                // all) are what a report needs to tell two refusals with the same
                // sentence apart (cmdr-reports#104).
                codes: Some(error_codes(&e)),
            })?;
        let in_trash = resulting.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()));
        #[cfg(any(test, feature = "testing"))]
        if let Some(landed) = &in_trash {
            crate::testing::note_trashed(landed);
        }
        Ok(in_trash)
    })
}

/// Read an `NSError` from `trashItemAtURL` as a [`TrashRefusalKind`].
///
/// ❗ Domain plus code, ❌ never the message. `localizedDescription` is written for
/// the user, ships in the user's language, and Apple rewords it between releases;
/// classifying on it means the dialog silently stops offering the right next step
/// for everyone who isn't running English. `error-string-match` enforces this.
///
/// The codes are Foundation's `NSCocoaErrorDomain` constants (`NSFileReadNoPermissionError`
/// 257, `NSFileWriteNoPermissionError` 513, `NSFeatureUnsupportedError` 3328), which
/// are ABI-stable and documented; objc2 doesn't re-export them, so they're spelled
/// here with their names. (Verified against Foundation's `FoundationErrors.h` on
/// macOS 27.0, 2026-09-16.)
fn classify_trash_refusal(error: &objc2_foundation::NSError) -> TrashRefusalKind {
    const NS_FILE_READ_NO_PERMISSION: isize = 257;
    const NS_FILE_WRITE_NO_PERMISSION: isize = 513;
    const NS_FEATURE_UNSUPPORTED: isize = 3328;
    const EPERM: isize = 1;
    const EACCES: isize = 13;

    let domain = error.domain().to_string();
    let code = error.code();
    match domain.as_str() {
        "NSCocoaErrorDomain" => match code {
            NS_FILE_READ_NO_PERMISSION | NS_FILE_WRITE_NO_PERMISSION => TrashRefusalKind::NotPermitted,
            NS_FEATURE_UNSUPPORTED => TrashRefusalKind::NoTrashForVolume,
            _ => TrashRefusalKind::Other,
        },
        "NSPOSIXErrorDomain" => match code {
            EPERM | EACCES => TrashRefusalKind::NotPermitted,
            _ => TrashRefusalKind::Other,
        },
        _ => TrashRefusalKind::Other,
    }
}

/// An `NSError`'s domain and code, then each `NSUnderlyingErrorKey` below it, as
/// `NSCocoaErrorDomain 513 < NSOSStatusErrorDomain -5000`. Capped at four levels
/// so a cyclic or very deep chain can't run away.
fn error_codes(error: &objc2_foundation::NSError) -> String {
    use objc2::Message;
    use objc2_foundation::{NSError, NSUnderlyingErrorKey};

    // SAFETY: `NSUnderlyingErrorKey` is an immutable `NSString` constant Foundation
    // exports and initializes before any code of ours runs.
    let key = unsafe { NSUnderlyingErrorKey };
    let mut codes = Vec::new();
    let mut current = Some(error.retain());
    while let Some(level) = current.take() {
        codes.push(format!("{} {}", level.domain(), level.code()));
        if codes.len() == 4 {
            break;
        }
        current = level
            .userInfo()
            .objectForKey(key)
            .and_then(|underlying| underlying.downcast::<NSError>().ok());
    }
    codes.join(" < ")
}

/// Narrow a [`TrashRefusalKind::NotPermitted`] down to what the item, its folder,
/// and its Trash say about themselves, since the `NSError` can't: each of these
/// refuses as 513, the same code a spot behind macOS's privacy protection gives.
/// None is a grant away, so each gets its own advice. Asked right after the
/// refusal: `trashItemAtURL` is atomic, so the item is still where it was.
///
/// (verified on macOS 27.0 with `trashItemAtURL`: a `chflags uchg` file and a file
/// in a `0555` folder, 2026-10-14; a root-owned file in `/private/tmp` and a file
/// on a disk image whose `.Trashes/<uid>` is `0500`, both 513 over
/// `NSOSStatusErrorDomain -5000`, 2026-11-10. `NSWorkspace.recycleURLs` refuses
/// both of the latter with the same 513, so it's no fallback.)
fn refine_permission_refusal(path: &Path, reason: TrashRefusalKind) -> TrashRefusalKind {
    use crate::error_classification::is_locked_item;
    use crate::validation::folder_write_refusal;

    if reason != TrashRefusalKind::NotPermitted {
        return reason;
    }
    if is_locked_item(path) {
        return TrashRefusalKind::Locked;
    }
    if let Some(folder) = path.parent() {
        if folder_write_refusal(folder).is_some() {
            return TrashRefusalKind::FolderNotWritable;
        }
        if sticky_folder_keeps_it(path, folder) {
            return TrashRefusalKind::OwnedByAnotherUser;
        }
    }
    if trash_dir_for_path(path).is_some_and(|trash| folder_write_refusal(&trash).is_some()) {
        return TrashRefusalKind::TrashNotWritable;
    }
    reason
}

/// Whether `folder` is sticky and keeps `item` from us: a sticky folder lets only
/// the item's owner, the folder's owner, or root move an item out, however
/// writable the folder is.
pub(in super::super) fn trash_folder_facts(trash: &Path) -> String {
    use std::os::macos::fs::MetadataExt as _;
    use std::os::unix::fs::MetadataExt;

    let meta = match fs::symlink_metadata(trash) {
        Ok(meta) => meta,
        Err(error) => {
            return match error.raw_os_error() {
                Some(errno) => format!("unreadable (errno {errno})"),
                None => "unreadable".to_string(),
            };
        }
    };
    // The boot volume's is `~/.Trash`; any other volume's is `.Trashes/<uid>`.
    let kind = if trash.file_name().is_some_and(|name| name == ".Trash") {
        "home"
    } else {
        "volume"
    };
    // SAFETY: `geteuid` takes no arguments, touches no memory of ours, and can't fail.
    let euid = unsafe { libc::geteuid() };
    format!(
        "kind={kind} owner_uid={} euid={euid} mode={:o} symlink={} flags={:#x}",
        meta.uid(),
        meta.mode() & 0o7777,
        meta.file_type().is_symlink(),
        meta.st_flags(),
    )
}

fn sticky_folder_keeps_it(item: &Path, folder: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(item_meta), Ok(folder_meta)) = (fs::symlink_metadata(item), fs::metadata(folder)) else {
        return false;
    };
    // SAFETY: `geteuid` takes no arguments, touches no memory of ours, and can't fail.
    let euid = unsafe { libc::geteuid() };
    sticky_rule_refuses(StickyFacts {
        folder_mode: folder_meta.mode(),
        folder_owner: folder_meta.uid(),
        item_owner: item_meta.uid(),
        us: euid,
    })
}
/// The trash directory that holds items trashed from `path`'s volume.
///
/// macOS keeps ONE trash per volume, so there is no single answer: the boot
/// volume's is `~/.Trash`, and every other mounted volume gets its own
/// `<mount point>/.Trashes/<uid>`. Rather than reconstruct that from `statfs` +
/// `getuid`, this asks Cocoa the same question the trash move itself asks, via
/// `URLForDirectory:inDomain:appropriateForURL:create:`, so an unusual mount
/// (a disk image, a synthetic firmlink, a volume with trash turned off) answers
/// for itself instead of against our guess. `create: false` keeps this a pure
/// lookup: a volume nobody has trashed anything to yet has no trash directory,
/// and that answers `None` rather than quietly creating one.
///
/// `None` also covers a volume with no trash at all (FAT32, SMB), which is why
/// callers treat it as "nowhere to go", never as a failure.
///
/// **Gotcha**: `appropriateForURL:` answers only for a path that EXISTS — it
/// resolves the URL's volume, and a missing path has none. The question is asked
/// about paths that are routinely gone by then (the item the user just trashed is
/// no longer where it was), so this walks up to the nearest existing ancestor,
/// which sits on the same volume and gives the same trash. Without the walk, "Go
/// to trash" would answer `None` in exactly the case it exists for.
///
/// (verified on macOS 15: the boot volume answers `~/.Trash` and a mounted
/// USB volume answers `/Volumes/<name>/.Trashes/501`; a nonexistent path answers
/// an error until it's resolved against a live ancestor, 2026-08-27.)
pub fn trash_dir_for_path(path: &Path) -> Option<PathBuf> {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask, NSString, NSURL};

    let anchor = path.ancestors().find(|p| fs::symlink_metadata(p).is_ok())?;

    autoreleasepool(|_| {
        let path_str = anchor.to_string_lossy();
        let ns_path = NSString::from_str(&path_str);
        let url = NSURL::fileURLWithPath(&ns_path);
        let file_manager = NSFileManager::defaultManager();

        let trash_url = file_manager
            .URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::TrashDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                Some(&url),
                false,
            )
            .ok()?;
        trash_url.path().map(|p| PathBuf::from(p.to_string()))
    })
}

/// Nothing to tidy: the Trash keeps no record beside an item.
pub(crate) fn forget_restored(_in_trash: &Path) {}
