//! Whether the running `.app` bundle sits somewhere an update can actually be written into.
//!
//! Three arrangements make the bundle unwritable, and none is fixable from inside the app:
//!
//! - App Translocation (Cmdr opened straight from `~/Downloads`, so Gatekeeper runs it from a
//!   randomized read-only mount) and a still-mounted `.dmg`. Both surface as `EROFS`, and a
//!   read-only mount refuses root too.
//! - A bundle this user may not write into, in practice one a `.pkg` installed as `root:wheel`.
//!   Admin rights don't fix that either: on macOS 13+, App Management refuses a privileged helper
//!   (`osascript … with administrator privileges` runs through Apple's `authtrampoline`, which
//!   isn't Cmdr's team) the same way it refuses everyone else. `DETAILS.md` § A bundle that can't
//!   be written.
//!
//! Detecting this BEFORE the download matters. Without the gate, such an install downloads ~63 MB
//! and rewrites nothing, once per poll interval, for as long as it runs.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use cmdr_fs::log_private::LogPrivate;

pub use crate::updater::BundleWriteBlocker;

/// Classifies a bundle path. Translocation is reported in preference to the read-only volume it
/// implies: a translocated app is on a read-only mount by construction, and naming the outer cause
/// is what tells a reader which arrangement they're looking at. Permissions come last: a read-only
/// mount would refuse the write whoever owned the bundle.
pub fn classify(bundle: &Path) -> Option<BundleWriteBlocker> {
    classify_with(bundle, is_admin_user)
}

/// [`classify`] with the admin question injected, so tests can pin both answers.
fn classify_with(bundle: &Path, is_admin: impl Fn() -> Option<bool>) -> Option<BundleWriteBlocker> {
    if is_translocated(bundle) {
        return Some(BundleWriteBlocker::Translocated);
    }
    if is_on_read_only_volume(bundle) {
        return Some(BundleWriteBlocker::ReadOnlyVolume);
    }
    if let Some(folder) = first_unwritable_folder(bundle) {
        log::info!(
            "This user can't write into {}, which the update needs",
            LogPrivate::dir_path(&folder)
        );
        return Some(not_writable(is_admin()));
    }
    None
}

/// The permission blocker for this user, for a write the pre-flight probe didn't see coming.
pub fn not_writable_for_this_user() -> BundleWriteBlocker {
    not_writable(is_admin_user())
}

/// An unknown answer reads as a standard user: "ask whoever manages this Mac" is true for anyone,
/// while "Finder will ask for your password" would strand someone who has none to give.
fn not_writable(is_admin: Option<bool>) -> BundleWriteBlocker {
    if is_admin == Some(true) {
        BundleWriteBlocker::NotWritableAdminUser
    } else {
        BundleWriteBlocker::NotWritableStandardUser
    }
}

/// The first folder in the bundle's `Contents` tree this user can't create files in, if any.
///
/// The sync replaces each file with a temp file plus `rename()`, which needs the FOLDER writable,
/// never the file, so folders are all it asks about. Symlinks aren't followed: the sync replaces a
/// link, it doesn't write through one. The bundle folder itself is left out too, since the install
/// only touches its mtime and a failure there is a warn.
///
/// Every doubt answers "writable". A missed folder costs one download that ends in the same notice
/// (the installer's `PermissionDenied` arm), while a wrong "unwritable" would stop updates that
/// would have worked.
fn first_unwritable_folder(bundle: &Path) -> Option<PathBuf> {
    let mut pending = vec![bundle.join("Contents")];
    while let Some(folder) = pending.pop() {
        match std::fs::symlink_metadata(&folder) {
            Ok(meta) if meta.is_dir() => {}
            _ => continue,
        }
        if denies_writing(&folder) {
            return Some(folder);
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            // `DirEntry::file_type` doesn't follow symlinks, so a linked folder reads as a symlink.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(entry.path());
            }
        }
    }
    None
}

/// Whether `access(2)` refuses this user write access to `path` on permissions (`EACCES`). Any other
/// answer, including one we can't get, reads as "not refused".
fn denies_writing(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a live NUL-terminated C string for the duration of the call, and
    // `access` only reads it.
    let rc = unsafe { libc::access(c_path.as_ptr(), libc::W_OK) };
    rc != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EACCES)
}

/// The `admin` group's id on every macOS release.
const ADMIN_GID: libc::gid_t = 80;

/// Whether the current user is in the `admin` group, or `None` when Directory Services can't say.
///
/// Asks `mbr_check_membership` rather than comparing `getgroups()`: membership can come through a
/// nested or directory group, and a process's group list is capped and snapshotted at login.
fn is_admin_user() -> Option<bool> {
    // libSystem's membership API (`<membership.h>`); the `libc` crate doesn't declare it.
    unsafe extern "C" {
        fn mbr_uid_to_uuid(uid: libc::uid_t, uu: *mut u8) -> libc::c_int;
        fn mbr_gid_to_uuid(gid: libc::gid_t, uu: *mut u8) -> libc::c_int;
        fn mbr_check_membership(user: *const u8, group: *const u8, ismember: *mut libc::c_int) -> libc::c_int;
    }

    let mut user = [0u8; 16];
    let mut group = [0u8; 16];
    let mut is_member: libc::c_int = 0;
    // SAFETY: `getuid` takes nothing and can't fail. Each `mbr_*` call writes at most 16 bytes (a
    // `uuid_t`) into the stack buffer it's handed, which is exactly 16 bytes, or one `c_int` into
    // `is_member`; the UUIDs are only read after the call that fills them reports success (0).
    let checked = unsafe {
        mbr_uid_to_uuid(libc::getuid(), user.as_mut_ptr()) == 0
            && mbr_gid_to_uuid(ADMIN_GID, group.as_mut_ptr()) == 0
            && mbr_check_membership(user.as_ptr(), group.as_ptr(), &mut is_member) == 0
    };
    checked.then_some(is_member != 0)
}

/// Asks macOS whether `path` is being served through App Translocation.
///
/// `SecTranslocateIsTranslocatedURL` is the supported answer (Security.framework, macOS 10.12+).
/// The alternative, testing the path for `/AppTranslocation/`, is a private layout that Apple
/// never promised and that we'd have no way to notice breaking.
///
/// Any failure answers `false`: a translocated app that we fail to recognize still gets caught by
/// the read-only-volume check below, so a false negative here costs only the sharper log line.
fn is_translocated(path: &Path) -> bool {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::url::{CFURL, CFURLRef};
    use std::ffi::c_void;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecTranslocateIsTranslocatedURL(path: CFURLRef, is_translocated: *mut u8, error: *mut *const c_void) -> u8;
    }

    let Some(url) = CFURL::from_path(path, true) else {
        return false;
    };

    let mut translocated: u8 = 0;
    let mut error: *const c_void = std::ptr::null();

    // SAFETY: `url` is a live CFURL owned by this scope and outlives the call. `translocated` and
    // `error` are stack slots the callee writes through; both are initialized before the call, and
    // `translocated` is read only when the call reports success. The `error` out-param is a
    // Create-rule CFErrorRef when non-null, balanced by the `CFType` wrapper below.
    let ok = unsafe { SecTranslocateIsTranslocatedURL(url.as_concrete_TypeRef(), &mut translocated, &mut error) };

    if !error.is_null() {
        // SAFETY: non-null here means the callee handed back a +1 CFError; wrapping it under the
        // Create rule transfers that reference so the drop at end of scope releases it exactly once.
        drop(unsafe { CFType::wrap_under_create_rule(error as CFTypeRef) });
    }

    ok != 0 && translocated != 0
}

/// Whether the volume holding `path` is mounted read-only, via `statfs`'s `MNT_RDONLY`.
///
/// This is the catch-all: it covers a mounted `.dmg`, a read-only network share, and translocation
/// itself, all without knowing which one it's looking at.
fn is_on_read_only_volume(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };

    // SAFETY: `libc::statfs` is a C struct of plain integers, so an all-zero bit pattern is a
    // valid (if meaningless) value to hand the call as scratch space.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a live NUL-terminated C string, and `stat` is a writable `statfs` the
    // call fills in; its fields are read only after `rc == 0`.
    let rc = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) };

    // `f_flags` is `u32` while `MNT_RDONLY` is declared `i32`; the constant is a single low bit,
    // so the cast is exact.
    rc == 0 && (stat.f_flags & libc::MNT_RDONLY as u32) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo itself is on a writable APFS volume and isn't translocated, so the happy path has
    /// to answer "nothing in the way". A blocker here would gate every update on this machine.
    #[test]
    fn a_normal_writable_path_has_no_blocker() {
        let here = std::env::current_dir().expect("the test process has a working directory");
        assert_eq!(classify(&here), None);
    }

    /// `/` is read-only on macOS 11+ (the signed system volume), which gives the `statfs` arm a
    /// real read-only mount to answer about without staging a disk image.
    #[test]
    #[cfg(target_os = "macos")]
    fn the_sealed_system_volume_reads_as_read_only() {
        assert!(
            is_on_read_only_volume(Path::new("/")),
            "macOS 11+ mounts the system volume read-only; if this fails, MNT_RDONLY isn't being read right"
        );
    }

    /// A path nothing can `statfs` must not be reported as read-only: a blocker is a hard stop on
    /// updating, so the failure direction has to be "keep going".
    #[test]
    fn an_unstattable_path_is_not_a_blocker() {
        assert!(!is_on_read_only_volume(Path::new("/no/such/path/for/cmdr/tests")));
    }

    /// A path with an interior NUL can't become a C string. Same reasoning: fail open.
    #[test]
    fn a_path_with_an_interior_nul_is_not_a_blocker() {
        use std::ffi::OsStr;
        let path = Path::new(OsStr::from_bytes(b"/tmp/cm\0dr"));
        assert!(!is_on_read_only_volume(path));
    }

    /// A normal path isn't translocated, and the call must survive being asked (the out-params and
    /// the CFError release are the parts worth exercising for real).
    #[test]
    fn a_normal_path_is_not_translocated() {
        let here = std::env::current_dir().expect("the test process has a working directory");
        assert!(!is_translocated(&here));
    }

    /// A path that doesn't exist still has to answer, not trap: the CFURL is constructible, so the
    /// framework call runs and reports its own failure through the out-params.
    #[test]
    fn a_missing_path_answers_rather_than_trapping() {
        assert!(!is_translocated(Path::new("/no/such/path/for/cmdr/tests")));
    }

    use crate::test_support::TestDir;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// A temp `Cmdr.app` shaped like the real one: the folders the sync writes into, each with a file.
    fn fake_bundle() -> (TestDir, PathBuf) {
        let dir = TestDir::new("bundle-location");
        let bundle = dir.join("Cmdr.app");
        for folder in [
            "Contents/MacOS",
            "Contents/Resources/resources/ai",
            "Contents/_CodeSignature",
        ] {
            fs::create_dir_all(bundle.join(folder)).expect("create folder");
        }
        fs::write(bundle.join("Contents/Info.plist"), "plist").expect("write");
        fs::write(bundle.join("Contents/MacOS/Cmdr"), "binary").expect("write");
        fs::write(bundle.join("Contents/Resources/resources/ai/model"), "model").expect("write");
        (dir, bundle)
    }

    /// Takes write access away from a path for the test's duration, and gives it back on drop so
    /// the temp dir can clean itself up.
    struct ReadOnly(PathBuf);

    impl ReadOnly {
        fn new(path: PathBuf) -> Self {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).expect("chmod 555");
            Self(path)
        }
    }

    impl Drop for ReadOnly {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    /// The update path everyone takes: a bundle the user owns is never blocked. A wrong answer here
    /// stops updates that would have worked, which is the one outcome this gate must never cause.
    #[test]
    fn a_bundle_the_user_owns_has_no_blocker() {
        let (_dir, bundle) = fake_bundle();
        assert_eq!(classify_with(&bundle, || Some(true)), None);
        assert_eq!(first_unwritable_folder(&bundle), None);
    }

    /// The sync writes a temp file and renames it in every folder it touches, so ONE folder deep in
    /// the tree that refuses the user is enough to strand the update half-written.
    #[test]
    fn one_unwritable_folder_deep_in_the_tree_blocks() {
        let (_dir, bundle) = fake_bundle();
        let deep = bundle.join("Contents/Resources/resources/ai");
        let _guard = ReadOnly::new(deep.clone());
        assert_eq!(first_unwritable_folder(&bundle), Some(deep));
    }

    #[test]
    fn an_unwritable_contents_folder_blocks() {
        let (_dir, bundle) = fake_bundle();
        let contents = bundle.join("Contents");
        let _guard = ReadOnly::new(contents.clone());
        assert_eq!(first_unwritable_folder(&bundle), Some(contents));
    }

    #[test]
    fn an_administrator_is_told_they_can_replace_it() {
        let (_dir, bundle) = fake_bundle();
        let _guard = ReadOnly::new(bundle.join("Contents/MacOS"));
        assert_eq!(
            classify_with(&bundle, || Some(true)),
            Some(BundleWriteBlocker::NotWritableAdminUser)
        );
    }

    #[test]
    fn a_standard_user_is_told_to_ask_whoever_manages_the_mac() {
        let (_dir, bundle) = fake_bundle();
        let _guard = ReadOnly::new(bundle.join("Contents/MacOS"));
        assert_eq!(
            classify_with(&bundle, || Some(false)),
            Some(BundleWriteBlocker::NotWritableStandardUser)
        );
    }

    /// "Finder will ask for your password" would strand someone with no password to give, so an
    /// unknown answer gets the copy that's true for everyone.
    #[test]
    fn an_unknown_admin_answer_reads_as_a_standard_user() {
        assert_eq!(not_writable(None), BundleWriteBlocker::NotWritableStandardUser);
    }

    /// The bundle folder itself is only touched for its mtime (a failure there is a warn), so a
    /// read-only `Cmdr.app` over a writable `Contents` still updates.
    #[test]
    fn a_read_only_bundle_folder_over_a_writable_contents_is_fine() {
        let (_dir, bundle) = fake_bundle();
        let _guard = ReadOnly::new(bundle.clone());
        assert_eq!(first_unwritable_folder(&bundle), None);
    }

    /// Replacing a file is a rename in its folder, which needs the FOLDER writable, not the file.
    #[test]
    fn a_read_only_file_in_a_writable_folder_is_fine() {
        let (_dir, bundle) = fake_bundle();
        let _guard = ReadOnly::new(bundle.join("Contents/MacOS/Cmdr"));
        assert_eq!(first_unwritable_folder(&bundle), None);
    }

    /// A symlinked folder (frameworks carry `Versions/Current`) is a link the sync replaces, not a
    /// tree it writes into, so the walk mustn't follow it somewhere read-only.
    #[test]
    fn a_symlink_to_a_read_only_folder_isnt_followed() {
        let (_dir, bundle) = fake_bundle();
        std::os::unix::fs::symlink("/System", bundle.join("Contents/Resources/system-link")).expect("symlink");
        assert_eq!(first_unwritable_folder(&bundle), None);
    }

    /// Outside a real bundle (a path with no `Contents`), the answer is "nothing in the way".
    #[test]
    fn a_path_without_contents_has_no_unwritable_folder() {
        let dir = TestDir::new("bundle-location");
        assert_eq!(first_unwritable_folder(&dir), None);
    }

    /// The membership lookup has to answer on a real Mac: whichever way, never "couldn't tell".
    #[test]
    fn the_admin_question_gets_an_answer() {
        assert!(is_admin_user().is_some());
    }
}
