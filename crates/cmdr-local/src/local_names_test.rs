//! Names a local volume must answer for exactly: a source name that names no child, refused
//! alone on every OS, then each OS's own hard names. On macOS and Linux, names the app path
//! spells with an escape (`posix_tests`); on Windows, names NTFS holds that Win32's plain
//! paths misread (`win32_tests`, `docs/specs/app-path-plan.md` § "The native boundary").

use crate::test_support::volume_at;
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::{SourceItemInfo, Volume};

/// ❗ A source name that isn't one plain name is refused typed, ❌ never joined: `Path::join`
/// of `..` asked about the destination's PARENT (and reported it as a conflict), and of `/x`
/// about `/x` itself. The refusal is that ITEM's: it names no destination, so it can't
/// collide, and every other item in the batch is still checked (a conflict has no per-item
/// error, so the refusal is logged). No listing yields such a name, so only a broken or
/// hostile caller ever sends one. `a\b` is one plain name on macOS and Linux (no file holds it
/// here, so no conflict) and two on Windows (refused). On macOS and Linux, a real name holding
/// escape code points of both alphabets (which no typed name can spell) is checked as before;
/// Windows refuses an escape until escapes go live (plan milestone 10f).
#[tokio::test]
async fn a_conflict_scan_refuses_only_the_source_name_that_is_not_one_plain_name() {
    let dir = AppTestDir::new("names_conflicts");
    std::fs::write(dir.native().join("kept.txt"), b"x").unwrap();
    #[cfg(unix)]
    let odd_real_names = {
        let odd_real_name = "m\u{10FEE9}x\u{10E801}";
        std::fs::write(dir.native().join(odd_real_name), b"x").unwrap();
        vec![odd_real_name]
    };
    #[cfg(not(unix))]
    let odd_real_names: Vec<&str> = Vec::new();
    let names = [
        vec!["..", "/etc", "a/b", r"a\b", "", "kept.txt"],
        odd_real_names.clone(),
    ]
    .concat();
    let expected = [vec!["kept.txt"], odd_real_names].concat();
    let volume = volume_at(&dir);
    let source_items: Vec<SourceItemInfo> = names
        .into_iter()
        .map(|name| SourceItemInfo {
            name: name.to_string(),
            size: 1,
            modified: None,
            is_directory: false,
        })
        .collect();

    let conflicts = volume
        .scan_for_conflicts(&source_items, volume.root())
        .await
        .expect("an odd name doesn't fail the batch");

    let found: Vec<&str> = conflicts.iter().map(|c| c.source_path.as_str()).collect();
    assert_eq!(found, expected, "{conflicts:?}");
}

/// Names the app path spells with an escape: a Linux name that isn't UTF-8, and on macOS (whose
/// APFS refuses one) a name holding a literal escape-range code point, the same row shape there.
/// Until escapes go live (plan milestone 10f), every row the volume hands back for such a name is
/// today's lossy text, so a pane holds ONE row for the file whichever reader built it: the
/// listing, `get_metadata`, or a mutation's patch.
#[cfg(unix)] // POSIX byte names; Windows refuses an escaped path until milestone 10f
mod posix_tests {
    use crate::LocalPosixVolume;
    use cmdr_fs::app_path::{AppNameBuf, AppPathBuf};
    use cmdr_fs::entry::FileEntry;
    use cmdr_fs::testing::TestDir;
    use cmdr_fs::volume::DirectoryChange;
    use cmdr_fs::volume::host::VolumeHost;
    use cmdr_fs::volume::host::listings::RecordingListings;
    use cmdr_fs::volume::{MutationEvent, Volume};
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::Arc;

    /// The native name of a file whose app spelling holds an escape on this OS.
    fn escaped_native_name() -> &'static OsStr {
        if cfg!(target_os = "linux") {
            OsStr::from_bytes(b"caf\xe9.txt")
        } else {
            OsStr::new("caf\u{10FEE9}.txt")
        }
    }

    struct Fixture {
        dir: TestDir,
        listings: Arc<RecordingListings>,
        volume: LocalPosixVolume,
        /// The row the listing gives the file.
        listed: FileEntry,
    }

    async fn fixture(tag: &str) -> Fixture {
        let dir = TestDir::new(tag);
        std::fs::write(dir.join(escaped_native_name()), b"bytes").expect("the file is created");
        let listings = Arc::new(RecordingListings::new());
        let host = VolumeHost::builder().listings(listings.clone()).build();
        let volume = LocalPosixVolume::local_folder("Test", &*dir, host);
        let listed = volume
            .list_directory(volume.root(), None)
            .await
            .expect("the folder lists")
            .into_iter()
            .find(|entry| entry.name.starts_with("caf"))
            .expect("the file lists");
        Fixture {
            dir,
            listings,
            volume,
            listed,
        }
    }

    /// `get_metadata` of the file's app path (escaped, as a path made from the native one is)
    /// answers the listing's row text, ❌ never the escaped spelling.
    #[tokio::test]
    async fn get_metadata_of_an_escaped_path_answers_the_listed_row() {
        let f = fixture("names_get_metadata").await;
        let native = f.dir.join(escaped_native_name());
        let path = AppPathBuf::from_native(&native).expect("a native path crosses up");
        assert!(
            path.as_str().chars().any(cmdr_fs::app_path::is_escape),
            "the path the volume is asked about holds an escape: {path:?}"
        );

        let entry = f.volume.get_metadata(&path).await.expect("the file stats");

        assert_eq!((&entry.name, &entry.path), (&f.listed.name, &f.listed.path));
        assert_eq!(entry.size, Some(5), "the stat reached the file itself");
    }

    /// A mutation named by the file's ESCAPED spelling patches no second row beside the listed
    /// one: the patch names the file as the folder's text joins it, the way it always did.
    #[tokio::test]
    async fn a_mutation_named_by_the_escaped_spelling_patches_no_second_row() {
        let f = fixture("names_mutation_escaped").await;
        let escaped = AppNameBuf::from_native_os(escaped_native_name())
            .expect("one plain name")
            .into_string();

        f.volume
            .notify_mutation("vol", f.volume.root(), MutationEvent::Created(escaped))
            .await;

        for (_, _, change) in f.listings.changes() {
            if let DirectoryChange::Added(entry) = change {
                assert_eq!(
                    (&entry.name, &entry.path),
                    (&f.listed.name, &f.listed.path),
                    "a patch may only repeat the listed row"
                );
            }
        }
    }

    /// A mutation named by the listed row's own name (what an engine sends: a lossy
    /// `file_name()`) patches at most that same row.
    #[tokio::test]
    async fn a_mutation_named_by_the_listed_name_patches_the_listed_row() {
        let f = fixture("names_mutation_listed").await;

        f.volume
            .notify_mutation("vol", f.volume.root(), MutationEvent::Created(f.listed.name.clone()))
            .await;

        let added: Vec<FileEntry> = f
            .listings
            .changes()
            .into_iter()
            .filter_map(|(_, _, change)| match change {
                DirectoryChange::Added(entry) => Some(entry),
                _ => None,
            })
            .collect();
        if cfg!(target_os = "macos") {
            // The listed text IS the file's name there, so the re-stat finds it.
            assert_eq!(added.len(), 1, "{added:?}");
        }
        for entry in added {
            assert_eq!((&entry.name, &entry.path), (&f.listed.name, &f.listed.path));
        }
    }
}

/// Names NTFS holds that Win32's plain paths misread, run on real NTFS: every one reaches its
/// own file through the volume's `\\?\` crossing, never a neighbor (each reproduced by hand in
/// the app-path plan's review round 3).
#[cfg(target_os = "windows")] // Win32's trailing-dot, device-name, stream, and UTF-16 rules
mod win32_tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;
    use std::path::Path;

    use cmdr_fs::testing::AppTestDir;
    use cmdr_fs::volume::{Volume, VolumeError};

    use crate::test_support::{at, volume_at};

    /// The names in `dir` as the OS lists them, sorted, each with its byte count.
    fn on_disk(dir: &Path) -> Vec<(OsString, u64)> {
        let mut names: Vec<(OsString, u64)> = std::fs::read_dir(dir)
            .expect("the folder lists")
            .map(|entry| {
                let entry = entry.expect("an entry");
                (entry.file_name(), entry.metadata().expect("its stat").len())
            })
            .collect();
        names.sort();
        names
    }

    fn names(listed: &[(&str, u64)]) -> Vec<(OsString, u64)> {
        let mut names: Vec<(OsString, u64)> = listed.iter().map(|&(text, len)| (OsString::from(text), len)).collect();
        names.sort();
        names
    }

    /// ❗ `dup` and `dup.` are two files. A plain `D:\x\dup.` names `dup` (Win32 strips the dot),
    /// so a delete of the row `dup.` through a plain path would remove `dup`, the other file.
    #[tokio::test]
    async fn a_delete_of_the_row_dup_dot_removes_dup_dot_and_leaves_dup() {
        let test_dir = AppTestDir::new("win_dup_dot");
        let volume = volume_at(&test_dir);
        volume.create_file(&at(&volume, "dup"), b"1").await.expect("create dup");
        volume
            .create_file(&at(&volume, "dup."), b"22")
            .await
            .expect("create dup.");
        assert_eq!(
            on_disk(test_dir.native()),
            names(&[("dup", 1), ("dup.", 2)]),
            "two files side by side"
        );

        let listed = volume.list_directory(volume.root(), None).await.expect("lists");
        let row = listed
            .iter()
            .find(|entry| entry.name == "dup.")
            .expect("the row `dup.`");
        assert_eq!(row.size, Some(2), "the row `dup.` is `dup.`'s own stat");
        volume
            .delete(&cmdr_fs::app_path::AppPathBuf::from_wire(&row.path).expect("a row path"))
            .await
            .expect("delete the row");

        assert_eq!(on_disk(test_dir.native()), names(&[("dup", 1)]), "only `dup.` went");
    }

    /// A device name and a trailing space are ordinary files through `\\?\`: each lists, stats,
    /// renames, and deletes as itself. (Creating one is refused earlier, by the name rules,
    /// Windows plan task 14; this is the file a Linux tool or WSL made.)
    #[tokio::test]
    async fn a_device_name_and_a_trailing_space_list_stat_rename_and_delete_as_files() {
        for name in ["con.txt", "NUL", "a "] {
            let test_dir = AppTestDir::new("win_reserved");
            let volume = volume_at(&test_dir);
            volume.create_file(&at(&volume, name), b"bytes").await.expect(name);
            assert_eq!(
                on_disk(test_dir.native()),
                names(&[(name, 5)]),
                "{name:?} created as itself"
            );

            let listed = volume.list_directory(volume.root(), None).await.expect("lists");
            assert_eq!(listed.len(), 1, "{name:?}");
            assert_eq!(listed[0].name, name);
            assert_eq!(listed[0].size, Some(5), "{name:?} stats as the file");
            let stat = volume.get_metadata(&at(&volume, name)).await.expect(name);
            assert_eq!(stat.size, Some(5), "{name:?}");

            let renamed = format!("{name}-renamed");
            volume
                .rename(&at(&volume, name), &at(&volume, &renamed), false)
                .await
                .expect(name);
            assert_eq!(on_disk(test_dir.native()), names(&[(&renamed, 5)]), "{name:?} renamed");
            volume.delete(&at(&volume, &renamed)).await.expect(name);
            assert!(on_disk(test_dir.native()).is_empty(), "{name:?} deleted");
        }
    }

    /// ❗ A name holding `:` never reaches the OS: `C:/x/a:b` would write the alternate data
    /// stream `b` INSIDE a file `a` (creating an empty `a` to hold it), which no listing shows.
    #[tokio::test]
    async fn a_name_holding_a_colon_is_refused_and_no_stream_appears() {
        let test_dir = AppTestDir::new("win_colon");
        let volume = volume_at(&test_dir);

        let refused = volume.create_file(&at(&volume, "a:b"), b"hidden").await;
        assert!(matches!(refused, Err(VolumeError::InvalidName(_))), "{refused:?}");
        let refused = volume.create_directory(&at(&volume, "a:b")).await;
        assert!(matches!(refused, Err(VolumeError::InvalidName(_))), "{refused:?}");
        assert!(on_disk(test_dir.native()).is_empty(), "no base file `a` holds a stream");
    }

    /// Two names differing only in an unpaired surrogate list as two rows, each its own file's
    /// stat (the reader stats through the raw name). Their row names and paths are both
    /// `a\u{FFFD}b` until escapes go live (plan milestone 10f gives each its own lossless
    /// spelling, and this cell's names flip then).
    #[tokio::test]
    async fn names_differing_only_in_an_unpaired_surrogate_list_as_two_rows_with_their_own_stats() {
        let test_dir = AppTestDir::new("win_surrogates");
        std::fs::write(test_dir.native().join(OsString::from_wide(&[0x61, 0xD800, 0x62])), b"1").expect("a<D800>b");
        std::fs::write(
            test_dir.native().join(OsString::from_wide(&[0x61, 0xD801, 0x62])),
            b"22",
        )
        .expect("a<D801>b");
        let volume = volume_at(&test_dir);

        let listed = volume.list_directory(volume.root(), None).await.expect("lists");
        let mut sizes: Vec<Option<u64>> = listed.iter().map(|entry| entry.size).collect();
        sizes.sort();
        assert_eq!(sizes, [Some(1), Some(2)], "each row is its own file's stat");
        for entry in &listed {
            assert_eq!(entry.name, "a\u{FFFD}b", "today's lossy row");
            assert!(!entry.path.contains('\\'), "an app path: {}", entry.path);
        }
    }

    /// A native path (`C:\Users\…`, what a tempdir or the OS hands over) crosses up at the
    /// crate's `&Path` entry points, so the reader lists it with app paths and the no-replace
    /// rename lands.
    #[test]
    fn the_native_entry_points_take_a_native_windows_path() {
        let test_dir = AppTestDir::new("win_native_entry");
        let native = test_dir.native();
        std::fs::write(native.join("a.txt"), b"a").expect("a.txt");
        std::fs::write(native.join("taken.txt"), b"the user's").expect("taken.txt");

        let listed =
            crate::reading::list_directory_core(native, cmdr_fs::volume::host::VolumeHost::detached().listings())
                .expect("a native folder lists");
        for entry in &listed {
            assert!(!entry.path.contains('\\'), "an app path: {}", entry.path);
            assert!(entry.path.ends_with(&format!("/{}", entry.name)), "{}", entry.path);
        }
        let single = crate::reading::get_single_entry(&native.join("a.txt")).expect("stats");
        assert_eq!(single.size, Some(1));

        let taken = crate::rename_local_exclusive(&native.join("a.txt"), &native.join("taken.txt"));
        assert_eq!(
            taken.expect_err("a taken name").kind(),
            std::io::ErrorKind::AlreadyExists
        );
        crate::rename_local_exclusive(&native.join("a.txt"), &native.join("b.txt")).expect("renames");
        assert_eq!(on_disk(native), names(&[("b.txt", 1), ("taken.txt", 10)]));
    }
}
