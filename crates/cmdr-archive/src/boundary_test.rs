//! The boundary detector on real files in a scratch dir, through the typed
//! forms the host moves to, plus the `std::path` forms it still calls (on Unix,
//! where callers hand them POSIX text).

use super::*;
use crate::testing::at;
use cmdr_fs::app_path::RelativeAppPath;
use cmdr_fs::testing::AppTestDir;

/// A scratch dir holding `name` with `bytes`, and that file's app path.
fn on_disk(name: &str, bytes: &[u8]) -> (AppTestDir, AppPathBuf) {
    let dir = AppTestDir::new("archive-boundary");
    std::fs::write(dir.native().join(name), bytes).expect("write the file");
    let path = at(dir.app_path(), name);
    (dir, path)
}

fn relative(text: &str) -> RelativeAppPathBuf {
    RelativeAppPath::new(text).expect("a relative path").to_owned()
}

#[test]
fn confirm_accepts_a_real_zip_file() {
    // A minimal but valid start-of-file zip signature is enough for the magic
    // check (which reads only the first four bytes).
    let (_dir, zip) = on_disk("real.zip", b"PK\x03\x04rest-of-archive");

    let (got_zip, got_inner) = confirm_archive_boundary_at(&at(&zip, "inner/a.txt")).expect("confirmed boundary");
    assert_eq!(got_zip, zip);
    assert_eq!(got_inner, relative("inner/a.txt"));
}

#[test]
fn confirm_rejects_a_real_directory_named_like_an_archive() {
    // A directory literally named `foo.zip` must lose to normal directory
    // navigation, never route into an archive volume.
    let dir = AppTestDir::new("archive-boundary");
    std::fs::create_dir_all(dir.native().join("foo.zip").join("sub")).expect("create dir named foo.zip");
    let sub = at(dir.app_path(), "foo.zip/sub");

    assert!(confirm_archive_boundary_at(&sub).is_none());
    assert!(!path_crosses_archive_boundary_at(&sub));
}

#[test]
fn confirm_rejects_a_mislabeled_non_zip_file() {
    let (_dir, mislabeled) = on_disk("notreally.zip", b"this is plain text, not a zip");

    assert!(confirm_archive_boundary_at(&at(&mislabeled, "inner")).is_none());
}

#[test]
fn inside_archive_distinguishes_the_zip_file_itself_from_its_contents() {
    let (dir, zip) = on_disk("real.zip", b"PK\x03\x04rest-of-archive");

    // A path INSIDE the archive is "inside"...
    assert!(path_is_inside_archive_at(&at(&zip, "inner/a.txt")));
    // ...but the `.zip` file ITSELF is NOT inside — it's a regular file, even
    // though it DOES "cross" a boundary (navigation enters it).
    assert!(!path_is_inside_archive_at(&zip));
    assert!(path_crosses_archive_boundary_at(&zip));
    // A plain non-archive path is neither.
    assert!(!path_is_inside_archive_at(dir.app_path()));
    assert!(!path_crosses_archive_boundary_at(dir.app_path()));
}

#[test]
fn targets_archive_file_true_for_a_real_file_named_like_an_archive() {
    // A mislabeled file (no zip magic) still counts: browsing it failed because
    // it isn't a real archive, which is the "unreadable archive" case.
    let (_dir, mislabeled) = on_disk("notreally.zip", b"plain text");
    assert!(path_targets_archive_file_at(&mislabeled));
    assert!(path_targets_archive_file_at(&at(&mislabeled, "inner")));
}

#[test]
fn targets_archive_file_false_for_a_directory_or_a_non_archive_path() {
    let dir = AppTestDir::new("archive-boundary");
    // A real directory named like an archive lists normally, so it's not the case.
    std::fs::create_dir(dir.native().join("foo.zip")).expect("create dir named foo.zip");
    assert!(!path_targets_archive_file_at(&at(dir.app_path(), "foo.zip")));
    // No archive-extension component at all.
    assert!(!path_targets_archive_file_at(&at(dir.app_path(), "plain/folder")));
    // Archive-extension name that doesn't exist on disk.
    assert!(!path_targets_archive_file_at(&at(dir.app_path(), "ghost.zip")));
}

#[test]
fn confirm_is_none_when_the_archive_component_does_not_exist() {
    let dir = AppTestDir::new("archive-boundary");
    assert!(confirm_archive_boundary_at(&at(dir.app_path(), "ghost.zip/inner")).is_none());
}

// ---- Multi-format extension + magic ------------------------------------

#[test]
fn extension_check_recognizes_the_tar_family_and_7z() {
    for name in [
        "a.tar",
        "a.tar.gz",
        "a.tgz",
        "a.tar.xz",
        "a.txz",
        "a.tar.bz2",
        "a.tbz2",
        "a.tar.zst",
        "a.tzst",
        "a.7z",
        "a.TAR.GZ",
    ] {
        assert!(has_supported_archive_extension(name), "{name} should be an archive");
    }
    // A bare compressed file (not a tar) is not a browsable archive.
    assert!(!has_supported_archive_extension("photo.gz"));
    assert!(!has_supported_archive_extension("data.zst"));
}

#[test]
fn confirm_matches_per_format_magic() {
    // A plain tar has no offset-0 magic; ustar sits at 257.
    let mut tar_bytes = vec![0u8; 512];
    tar_bytes[257..262].copy_from_slice(b"ustar");
    let (_tar_dir, tar) = on_disk("real.tar", &tar_bytes);
    assert!(confirm_archive_boundary_at(&at(&tar, "inner")).is_some());

    // gzip-wrapped tar.
    let (_tgz_dir, tgz) = on_disk("real.tar.gz", &[0x1f, 0x8b, 0x08, 0x00]);
    let (zip, inner) = confirm_archive_boundary_at(&at(&tgz, "d/f.txt")).expect("tgz boundary");
    assert_eq!(zip, tgz);
    assert_eq!(inner, relative("d/f.txt"));

    // 7z magic.
    let (_sevenz_dir, sevenz) = on_disk("real.7z", &[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00]);
    assert!(confirm_archive_boundary_at(&at(&sevenz, "x")).is_some());
}

#[test]
fn confirm_rejects_a_mislabeled_tar_gz() {
    // A `.tar.gz` name whose bytes aren't gzip must not route.
    let (_dir, fake) = on_disk("notreally.tar.gz", b"plain text, definitely not gzip");
    assert!(confirm_archive_boundary_at(&at(&fake, "inner")).is_none());
}

// The split itself (no I/O) is `cmdr_fs::archive_format`'s, tested there.

/// The `std::path` forms the host still calls, on the same fixtures.
#[cfg(unix)]
#[test]
fn the_std_path_forms_answer_as_the_typed_ones() {
    let (dir, _zip) = on_disk("real.zip", b"PK\x03\x04rest-of-archive");
    let zip = dir.native().join("real.zip");
    let (got_zip, got_inner) = confirm_archive_boundary(&zip.join("inner/a.txt")).expect("confirmed boundary");
    assert_eq!(got_zip, zip);
    assert_eq!(got_inner, PathBuf::from("inner/a.txt"));
    assert!(path_is_inside_archive(&zip.join("inner/a.txt")));
    assert!(!path_is_inside_archive(&zip));
    assert!(path_crosses_archive_boundary(&zip));
    assert!(path_targets_archive_file(&zip.join("inner")));
    assert!(!path_targets_archive_file(&dir.native().join("ghost.zip")));
}
