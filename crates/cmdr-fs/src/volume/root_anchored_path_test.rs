//! Tests for [`super::root_anchored`], the one rule that turns a caller's path
//! into the form every backend accepts.

use super::root_anchored;
use std::path::{Path, PathBuf};

fn anchor(root: &str, path: &str) -> PathBuf {
    root_anchored(Path::new(root), Path::new(path))
}

#[test]
fn a_volume_relative_path_gets_the_root_in_front_of_it() {
    // The transfer dialog's destination box is volume-relative with a leading
    // slash. Handing `/test_photos` to a share mounted at `/Volumes/testshare`
    // without anchoring it is what made a 360 GB move die instantly:
    // `SmbVolume::to_smb_path` reads it as absolute, finds it outside the
    // mount, and answers `NotFound` before any I/O.
    assert_eq!(
        anchor("/Volumes/testshare", "/test_photos"),
        Path::new("/Volumes/testshare/test_photos")
    );
    assert_eq!(
        anchor("/Volumes/testshare", "/test_photos/test footage"),
        Path::new("/Volumes/testshare/test_photos/test footage")
    );
}

#[test]
fn a_path_with_no_leading_slash_gets_the_root_too() {
    assert_eq!(
        anchor("/Volumes/testshare", "photos"),
        Path::new("/Volumes/testshare/photos")
    );
    assert_eq!(
        anchor("/Volumes/testshare", "photos/2026"),
        Path::new("/Volumes/testshare/photos/2026")
    );
}

#[test]
fn a_path_already_under_the_root_is_left_alone() {
    // Idempotence is what lets every call site anchor without checking first:
    // the panes send absolute paths, the dialog sends volume-relative ones, and
    // anchoring twice must not double the prefix.
    let once = anchor("/Volumes/testshare", "/Volumes/testshare/photos");
    assert_eq!(once, Path::new("/Volumes/testshare/photos"));
    assert_eq!(root_anchored(Path::new("/Volumes/testshare"), &once), once);
}

#[test]
fn a_sibling_mount_sharing_a_name_prefix_is_not_under_the_root() {
    // macOS mounts a second copy of a share at `/Volumes/testshare-1`, and `-1` is a
    // legal filename. A raw string compare would call this path "already under
    // the root" and address the wrong file; whole-component matching anchors it
    // instead. Same rule `SmbVolume::to_smb_path` follows.
    assert_eq!(
        anchor("/Volumes/testshare", "/Volumes/testshare-1/x"),
        Path::new("/Volumes/testshare/Volumes/testshare-1/x")
    );
}

#[test]
fn the_root_volume_keeps_every_absolute_path_as_it_is() {
    // Root `/` contains every absolute path, so the "already under the root"
    // arm covers the whole local filesystem: no re-rooting, no `//` doubling.
    assert_eq!(
        anchor("/", "/Users/test_user_3/notes.txt"),
        Path::new("/Users/test_user_3/notes.txt")
    );
    assert_eq!(anchor("/", "/"), Path::new("/"));
    assert_eq!(anchor("/", "Users/test_user_3"), Path::new("/Users/test_user_3"));
}

#[test]
fn the_volume_root_itself_is_the_answer_for_every_spelling_of_it() {
    for root_ish in ["", "/", "."] {
        assert_eq!(
            anchor("/Volumes/testshare", root_ish),
            Path::new("/Volumes/testshare"),
            "{root_ish:?} means the volume root"
        );
    }
}

#[test]
fn a_scheme_shaped_root_anchors_the_same_way() {
    // An MTP volume's whole path vocabulary is `mtp://device/storage/…`, which
    // `Path::is_absolute` calls relative. The dialog still sends `/DCIM`, and
    // the device only ever sees the inner path, so anchoring has to produce the
    // URL form rather than a bare `/DCIM`.
    assert_eq!(
        anchor("mtp://mtp-0-1/65537", "/DCIM/Camera"),
        Path::new("mtp://mtp-0-1/65537/DCIM/Camera")
    );
    assert_eq!(
        anchor("mtp://mtp-0-1/65537", "mtp://mtp-0-1/65537/DCIM"),
        Path::new("mtp://mtp-0-1/65537/DCIM")
    );
}

// ── `anchor_at_root`, the typed twin ─────────────────────────────────

mod typed {
    use crate::app_path::{AppPath, NotAnAppPath, PathFlavor};
    use crate::volume::{AnchorRefusal, anchor_at_root};

    fn root(text: &str, flavor: PathFlavor) -> &'static AppPath {
        let text: &'static str = Box::leak(text.to_string().into_boxed_str());
        AppPath::from_normalized_as(text, flavor).unwrap_or_else(|why| panic!("{text:?}: {why}"))
    }

    /// The anchored text, or the app path's refusal of the wire text.
    fn anchored(root: &AppPath, wire: &str) -> Result<String, NotAnAppPath> {
        anchor_at_root(root, wire)
            .map(crate::app_path::AppPathBuf::into_string)
            .map_err(|refusal| match refusal {
                AnchorRefusal::NotAppPath(why) => why,
                AnchorRefusal::OffVolume => panic!("{wire:?} read as off the volume"),
            })
    }

    /// ❗ A path on another volume (its own scheme, drive, or UNC root, or a folder beside
    /// this volume's root on the same server) is refused as off the volume, ❌ never hung
    /// off the root as a chain of names (`…/srv/data/sftp:/…/srv/data-1/x`).
    #[test]
    fn a_path_on_another_volume_is_refused_as_off_the_volume() {
        let sftp = root("sftp://test_user_1@testnas.local:22/srv/data", PathFlavor::Posix);
        let mtp = root("mtp://mtp-0-1/65537", PathFlavor::Posix);
        let share = root("/Volumes/testshare", PathFlavor::Posix);
        for (root, wire) in [
            (sftp, "sftp://test_user_1@testnas.local:22/srv/data-1/x"),
            (sftp, "sftp://test_user_1@testnas.local:2222/srv/data/x"),
            (sftp, "mtp://mtp-0-1/65537/DCIM"),
            (mtp, "mtp://mtp-0-1/65538/DCIM"),
            (share, "smb://testnas.local/share/x"),
        ] {
            assert!(
                matches!(anchor_at_root(root, wire), Err(AnchorRefusal::OffVolume)),
                "{wire:?} under {root}: {:?}",
                anchor_at_root(root, wire)
            );
        }
        // A volume-relative `/x` still hangs off the root, as the transfer dialog types it.
        assert_eq!(anchored(share, "/photos").as_deref(), Ok("/Volumes/testshare/photos"));
        if PathFlavor::HOST == PathFlavor::Windows {
            let drive = root("C:/Users/test_user_1", PathFlavor::Windows);
            assert!(matches!(
                anchor_at_root(drive, "D:/elsewhere"),
                Err(AnchorRefusal::OffVolume)
            ));
        }
    }

    /// The `std::path` form's answers wherever it was right, on every host: the dialog's
    /// volume-relative text hangs off the root, a pane's own path stays, and every
    /// spelling of the root is the root.
    #[test]
    fn a_scheme_root_anchors_as_the_path_form_does() {
        let mtp = root("mtp://mtp-0-1/65537", PathFlavor::Posix);
        assert_eq!(
            anchored(mtp, "/DCIM/Camera").as_deref(),
            Ok("mtp://mtp-0-1/65537/DCIM/Camera")
        );
        assert_eq!(anchored(mtp, "DCIM").as_deref(), Ok("mtp://mtp-0-1/65537/DCIM"));
        assert_eq!(
            anchored(mtp, "mtp://mtp-0-1/65537/DCIM").as_deref(),
            Ok("mtp://mtp-0-1/65537/DCIM")
        );
        for root_ish in ["", ".", "/", "//", "/./"] {
            assert_eq!(
                anchored(mtp, root_ish).as_deref(),
                Ok("mtp://mtp-0-1/65537"),
                "{root_ish:?}"
            );
        }
    }

    /// ❗ What the path form got wrong: a trailing `/` is normalized away, and a `..` is
    /// refused rather than passed on to climb off the volume.
    #[test]
    fn a_trailing_slash_goes_and_a_dot_dot_is_refused() {
        let share = root("/Volumes/testshare", PathFlavor::Posix);
        assert_eq!(anchored(share, "photos/").as_deref(), Ok("/Volumes/testshare/photos"));
        assert_eq!(anchored(share, "./x").as_deref(), Ok("/Volumes/testshare/x"));
        assert_eq!(anchored(share, "../etc"), Err(NotAnAppPath::DotName));
        assert_eq!(anchored(share, "/Volumes/testshare/../etc"), Err(NotAnAppPath::DotName));
    }

    /// Windows shapes, pure, so every OS runs them: a drive root takes the dialog's `/x`
    /// as relative, a UNC root matches its server and share without case, and a name
    /// holding `\` can't hang off a drive (Windows would read a subfolder) while a remote
    /// root keeps it as a name character.
    #[test]
    fn drive_and_unc_roots_anchor_by_their_own_rules() {
        let drive = root("C:/Users/test_user_1", PathFlavor::Windows);
        assert_eq!(
            anchored(drive, "/Documents").as_deref(),
            Ok("C:/Users/test_user_1/Documents")
        );
        assert_eq!(anchored(drive, "a\\b"), Err(NotAnAppPath::Backslash));
        assert_eq!(anchored(drive, "CON").as_deref(), Ok("C:/Users/test_user_1/CON"));
        assert_eq!(anchored(drive, "dup. ").as_deref(), Ok("C:/Users/test_user_1/dup. "));
        let sftp = root("sftp://test_user_1@testnas.local:22/srv", PathFlavor::Windows);
        assert_eq!(
            anchored(sftp, "a\\b").as_deref(),
            Ok("sftp://test_user_1@testnas.local:22/srv/a\\b")
        );
        if PathFlavor::HOST == PathFlavor::Windows {
            assert_eq!(
                anchored(drive, "c:/Users/test_user_1/x").as_deref(),
                Ok("C:/Users/test_user_1/x")
            );
            let unc = root("//testnas.local/Share", PathFlavor::Windows);
            assert_eq!(
                anchored(unc, "//TESTNAS.local/share/x").as_deref(),
                Ok("//TESTNAS.local/share/x"),
                "the pane's own spelling of the share stays"
            );
        }
    }
}
