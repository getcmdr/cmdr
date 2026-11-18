//! What the local volume does TODAY with a wire path shaped in ways a listing never
//! builds: a trailing `/` on a symlink to a folder, a `..` that crosses a symlink,
//! and a name addressed in another Unicode form than the disk holds.
//!
//! The volume hands the caller's bytes to the OS untouched, so the OS decides:
//! `lstat("link/")` follows the link, and `link/..` is the TARGET's parent. These
//! cells pin that, including the two answers the app-path type changes on purpose
//! (a trailing `/` gets dropped, so the path names the link itself; a `..` gets
//! resolved by the volume before it reaches a call), so that change is a visible
//! flip of a pinned answer, ❌ never a silent drift. #1225.

#![cfg(unix)] // POSIX symlinks and the OS's own `..` resolution through one.

use super::*;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::entry::FileEntry;
use cmdr_fs::testing::TestDir;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{Volume, VolumeError};
use std::path::Path;

/// `x/link -> ../real/inner`, with `real/inner/kept.txt`, plus two files named `y`:
/// `x/y` (one byte, where a lexical `..` lands) and `real/y` (four bytes, where
/// the OS's `..` through the link lands).
struct LinkTree {
    dir: TestDir,
    volume: LocalVolume,
}

impl LinkTree {
    fn new(name: &str) -> Self {
        let dir = TestDir::new(name);
        std::fs::create_dir_all(dir.join("real/inner")).unwrap();
        std::fs::write(dir.join("real/inner/kept.txt"), b"kept").unwrap();
        std::fs::write(dir.join("real/y"), b"real").unwrap();
        std::fs::create_dir_all(dir.join("x")).unwrap();
        std::fs::write(dir.join("x/y"), b"x").unwrap();
        cmdr_fs::testing::symlink("../real/inner", dir.join("x/link")).unwrap();
        let volume = LocalVolume::local_folder("Test", dir.to_path_buf(), VolumeHost::detached());
        Self { dir, volume }
    }

    /// The wire spelling of `rel` under the tree: the volume's root plus `rel`,
    /// byte for byte, so a trailing `/` or a `..` reaches the volume as typed.
    fn wire(&self, rel: &str) -> String {
        format!("{}/{rel}", self.dir.to_string_lossy())
    }

    async fn stat(&self, rel: &str) -> Result<FileEntry, VolumeError> {
        self.volume
            .get_metadata(&AppPath::legacy_unchecked(Path::new(&self.wire(rel))))
            .await
    }

    async fn listed_paths(&self, rel: &str) -> Vec<String> {
        let mut paths: Vec<String> = self
            .volume
            .list_directory(&AppPath::legacy_unchecked(Path::new(&self.wire(rel))), None)
            .await
            .expect("lists")
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        paths.sort();
        paths
    }
}

/// Without the slash the stat names the link; WITH it, the OS follows the link and
/// the stat describes the target folder. Today's answer: the app-path type drops
/// the trailing `/`, so the second stat flips to the first one's.
#[tokio::test]
async fn a_stat_of_a_folder_link_with_a_trailing_slash_describes_the_target_today() {
    let tree = LinkTree::new("wire_shape_stat_slash");

    let bare = tree.stat("x/link").await.expect("the link stats");
    assert!(bare.is_symlink, "the bare path names the link itself");

    let slashed = tree.stat("x/link/").await.expect("the slashed path stats");
    assert!(!slashed.is_symlink, "today: the slash follows the link");
    assert!(slashed.is_directory, "today: and describes the target folder");
    assert_eq!(slashed.name, "link", "the name is still the last component");
    assert_eq!(
        slashed.path,
        tree.wire("x/link/"),
        "and the path echoes the bytes it was given"
    );
}

/// Both spellings list the target's children, and the rows join their names onto
/// the path as given, so the slash never doubles: `link/kept.txt` either way.
#[tokio::test]
async fn a_listing_of_a_folder_link_lists_the_target_with_or_without_a_trailing_slash() {
    let tree = LinkTree::new("wire_shape_list_slash");
    let expected = vec![tree.wire("x/link/kept.txt")];
    assert_eq!(tree.listed_paths("x/link").await, expected);
    assert_eq!(tree.listed_paths("x/link/").await, expected);
}

/// The OS resolves `link/..` physically: the TARGET's parent, so `x/link/../y`
/// names `real/y`, ❌ not the `x/y` a lexical reading gives. Today's answer;
/// the volume-aware `..` resolver keeps it for a POSIX disk (it asks the volume),
/// while a bare `from_wire` refuses the `..` outright.
#[tokio::test]
async fn a_dot_dot_through_a_folder_link_lands_beside_the_target_today() {
    let tree = LinkTree::new("wire_shape_dot_dot");

    let through = tree.stat("x/link/../y").await.expect("stats");
    assert_eq!(
        through.size,
        Some(4),
        "today: the OS's `..` is the target's parent, so `real/y`"
    );
    assert_eq!(
        through.path,
        tree.wire("x/link/../y"),
        "the path keeps its `..` as given"
    );

    let listed = tree.listed_paths("x/link/..").await;
    assert_eq!(
        listed,
        vec![tree.wire("x/link/../inner"), tree.wire("x/link/../y")],
        "today: `link/..` lists `real`, with rows joined onto the unresolved text"
    );
}

/// A name created in NFD and addressed in NFC: APFS and HFS+ match names
/// normalization-insensitively, so the NFC spelling stats the file; ext4 and the
/// other Linux filesystems compare bytes, so it's not there. The listing gives
/// back the bytes the disk holds (NFD) on both. Equality stays exact in the
/// app-path type, so neither answer moves.
#[tokio::test]
async fn a_name_made_in_nfd_and_asked_in_nfc_follows_the_filesystem() {
    let dir = TestDir::new("wire_shape_unicode_form");
    let nfd = "cafe\u{0301}.txt";
    let nfc = "caf\u{00e9}.txt";
    std::fs::write(dir.join(nfd), b"nfd").unwrap();
    let volume = LocalVolume::local_folder("Test", dir.to_path_buf(), VolumeHost::detached());

    let listed = volume
        .list_directory(&AppPath::legacy_unchecked(&dir), None)
        .await
        .expect("lists");
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, [nfd], "the listing keeps the stored NFD bytes");

    let nfc_path = dir.join(nfc);
    let asked = volume.get_metadata(&AppPath::legacy_unchecked(&nfc_path)).await;
    #[cfg(target_os = "macos")]
    {
        let entry = asked.expect("APFS matches the NFC spelling");
        assert_eq!(entry.size, Some(3));
        assert_eq!(
            entry.name, nfc,
            "the stat echoes the spelling it was asked, not the stored one"
        );
        assert!(volume.exists(&AppPath::legacy_unchecked(&nfc_path)).await);
    }
    #[cfg(target_os = "linux")]
    {
        assert!(
            matches!(asked, Err(VolumeError::NotFound(_))),
            "a byte-comparing filesystem has no NFC name: {asked:?}"
        );
        assert!(!volume.exists(&AppPath::legacy_unchecked(&nfc_path)).await);
    }
}
