//! The symlink seam, and how the copy scan counts a link.

#![cfg(unix)]

use super::*;
use cmdr_fs::app_path;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{Volume, VolumeError};
use std::path::Path;

/// A copy's totals: a link inside a folder is one entry worth no bytes, as the
/// shared scan and the preview count it, ❌ never skipped and ❌ never followed.
#[tokio::test]
async fn test_scan_for_copy_counts_a_nested_link_as_one_entry_without_following_it() {
    let test_dir = TestDir::new("scan_counts_link");
    std::fs::create_dir_all(test_dir.join("real")).unwrap();
    std::fs::write(test_dir.join("real/big.bin"), vec![0u8; 4096]).unwrap();
    std::fs::create_dir_all(test_dir.join("tree")).unwrap();
    std::fs::write(test_dir.join("tree/keep.txt"), b"kept").unwrap();
    cmdr_fs::testing::symlink("../real", test_dir.join("tree/link")).unwrap();
    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_path_buf(), VolumeHost::detached());

    let scan = volume.scan_for_copy(app_path!("/tree")).await.unwrap();

    assert_eq!(scan.file_count, 2, "the file and the link");
    assert_eq!(scan.total_bytes, 4, "the file's bytes alone");
}

/// A filesystem that can't hold a link (the error `exFAT` answers with) reads as
/// exactly that, and a missing folder as the plain failure it is.
#[tokio::test]
async fn test_read_link_target_of_something_missing_is_not_found() {
    let test_dir = TestDir::new("read_link_missing");
    let volume = LocalPosixVolume::local_folder("Test", test_dir.to_path_buf(), VolumeHost::detached());
    assert!(matches!(
        volume.read_link_target(app_path!("/nope")).await,
        Err(VolumeError::NotFound(_))
    ));
    assert!(matches!(
        volume.create_symlink(app_path!("/nope/link"), Path::new("t")).await,
        Err(VolumeError::NotFound(_))
    ));
}

/// What `<path>/..` names: the cells of `docs/specs/app-path-plan.md` § "Typed and pasted `..`"
/// on a real disk. `x/link -> ../real/dir`, `x/abs -> <dir>/real/dir`, and a plain folder `x/plain`.
mod dot_dot {
    use super::*;
    use cmdr_fs::app_path::AppPathBuf;
    use cmdr_fs::testing::AppTestDir;

    fn tree() -> AppTestDir {
        let dir = AppTestDir::new("dot_dot");
        let root = dir.native();
        std::fs::create_dir_all(root.join("real/dir")).unwrap();
        std::fs::create_dir_all(root.join("x/plain")).unwrap();
        cmdr_fs::testing::symlink("../real/dir", root.join("x/link")).unwrap();
        cmdr_fs::testing::symlink(root.join("real/dir"), root.join("x/abs")).unwrap();
        dir
    }

    fn at(dir: &AppTestDir, rel: &str) -> AppPathBuf {
        AppPathBuf::from_native(&dir.native().join(rel)).unwrap()
    }

    async fn up(dir: &AppTestDir, rel: &str) -> AppPathBuf {
        let volume = LocalPosixVolume::local_folder_at("Test", dir.app_path().to_owned(), VolumeHost::detached());
        volume.parent_for_dot_dot(&at(dir, rel)).await.expect("answers")
    }

    #[tokio::test]
    async fn a_link_answers_its_targets_parent_as_the_kernel_does() {
        let dir = tree();
        assert_eq!(up(&dir, "x/link").await, at(&dir, "real"));
        assert_eq!(up(&dir, "x/abs").await, at(&dir, "real"));
        // What the kernel says: `x/link/..` holds `real`'s children.
        assert!(dir.native().join("x/link/../dir").is_dir());
    }

    #[tokio::test]
    async fn a_plain_folder_and_a_missing_name_answer_the_lexical_parent() {
        let dir = tree();
        assert_eq!(up(&dir, "x/plain").await, at(&dir, "x"));
        assert_eq!(up(&dir, "x/nope").await, at(&dir, "x"));
        assert_eq!(up(&dir, "x/nope/deeper").await, at(&dir, "x/nope"));
    }

    #[tokio::test]
    async fn a_name_under_a_file_is_no_link() {
        let dir = tree();
        std::fs::write(dir.native().join("x/file.txt"), b"x").unwrap();
        assert_eq!(up(&dir, "x/file.txt/sub").await, at(&dir, "x/file.txt"));
    }
}
