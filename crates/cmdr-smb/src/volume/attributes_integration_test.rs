//! Hidden and Locked on a real share (requires Docker SMB containers): the DOS
//! attributes `HIDDEN` and `READONLY` a copy reads off a source and puts on what
//! it writes, and what a Read-only item does to its own removal.
//!
//! The contract: `DETAILS.md` § "Hidden and Locked on copies". Every test here
//! is `#[ignore]`d so default runs skip it. Start the containers with
//! `apps/desktop/test/smb-servers/start.sh`, then run
//! `cargo nextest run smb_integration --run-ignored all`.

use super::test_support::*;
use super::*;
use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::metadata::{CarriedMetadata, ClassOutcome, MetadataClasses, Unblock, UserFlags};
use cmdr_fs::volume::{InMemoryVolume, StreamLength, WriteMode};

/// The flags the share's read stream reports for `path`: the copy's own read.
async fn read_flags(vol: &SmbVolume, path: &str) -> UserFlags {
    vol.open_read_stream(&AppPath::legacy_unchecked(Path::new(path)))
        .await
        .unwrap_or_else(|e| panic!("open {path}: {e:?}"))
        .metadata(MetadataClasses::FLAGS)
        .flags
}

/// Streams `bytes` carrying `flags` from an in-memory source onto `dest`.
async fn write_carrying(vol: &SmbVolume, dest: &str, bytes: Vec<u8>, flags: UserFlags) {
    let source = InMemoryVolume::new("Source");
    let size = bytes.len() as u64;
    source.create_file(app_path!("/src.bin"), &bytes).await.unwrap();
    source.set_carried_metadata(
        Path::new("/src.bin"),
        CarriedMetadata {
            flags,
            ..CarriedMetadata::default()
        },
    );
    let stream = source.open_read_stream(app_path!("/src.bin")).await.unwrap();
    vol.write_from_stream(
        &AppPath::legacy_unchecked(Path::new(dest)),
        WriteMode::CreateNew,
        StreamLength::Known(size),
        stream,
        &|_| std::ops::ControlFlow::Continue(()),
    )
    .await
    .unwrap_or_else(|e| panic!("write {dest}: {e:?}"));
}

/// Hidden rides the write itself, on the one-frame path and the streaming
/// writer alike, so a Hidden file costs no frame after its landing. Locked
/// doesn't: it waits for the landing (`apply_metadata`), since the temp it'd
/// sit on still has a rename ahead of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_write_keeps_hidden_and_leaves_locked_for_the_landing() {
    let vol = make_docker_volume().await;
    let base = test_dir_name();
    ensure_clean(&vol, &base).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&base)))
        .await
        .unwrap();
    let max_write = vol.negotiated_max_write().await.expect("negotiated params");
    let big = (max_write + 64 * 1024) as usize;
    assert!(
        !vol.write_is_single_shot(StreamLength::Known(big as u64)).await,
        "fixture precondition: the big file must take the streaming writer"
    );

    for (name, size) in [("small.bin", 100), ("big.bin", big)] {
        let dest = format!("{base}/{name}");
        write_carrying(&vol, &dest, vec![0x42; size], UserFlags::HIDDEN | UserFlags::LOCKED).await;
        assert_eq!(
            read_flags(&vol, &dest).await,
            UserFlags::HIDDEN,
            "{name}: the write keeps Hidden and leaves Locked for after the landing"
        );
    }
    let plain = format!("{base}/plain.bin");
    write_carrying(&vol, &plain, vec![0x42; 100], UserFlags::NONE).await;
    assert_eq!(
        read_flags(&vol, &plain).await,
        UserFlags::NONE,
        "a plain file stays plain"
    );

    ensure_clean(&vol, &base).await;
}

/// After the landing, `apply_metadata` puts the whole set on a file and on a
/// folder, in the frame that carries the dates. A Read-only item then refuses
/// its own removal, which `clear_blockers` lifts for a reversal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_locked_lands_after_the_rename_and_a_reversal_can_still_remove_it() {
    let vol = make_docker_volume().await;
    let base = test_dir_name();
    ensure_clean(&vol, &base).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&base)))
        .await
        .unwrap();
    let file = format!("{base}/locked.txt");
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&file)), b"locked")
        .await
        .unwrap();
    let folder = format!("{base}/locked-folder");
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&folder)))
        .await
        .unwrap();
    let set = CarriedMetadata {
        flags: UserFlags::HIDDEN | UserFlags::LOCKED,
        ..CarriedMetadata::default()
    };

    for path in [&file, &folder] {
        let report = vol
            .apply_metadata(&AppPath::legacy_unchecked(Path::new(path)), &set, None)
            .await;
        assert_eq!(report.flags, ClassOutcome::Kept, "{path}: {report:?}");
    }
    assert_eq!(read_flags(&vol, &file).await, UserFlags::HIDDEN | UserFlags::LOCKED);
    // Samba renames a Read-only file, as Windows does: Read-only waits for the
    // landing because a staged temp wearing it couldn't be REMOVED (a cancel,
    // a failed landing), not renamed.
    let renamed = format!("{base}/locked-renamed.txt");
    vol.rename(
        &AppPath::legacy_unchecked(Path::new(&file)),
        &AppPath::legacy_unchecked(Path::new(&renamed)),
        false,
    )
    .await
    .expect("a Read-only file renames");
    vol.rename(
        &AppPath::legacy_unchecked(Path::new(&renamed)),
        &AppPath::legacy_unchecked(Path::new(&file)),
        false,
    )
    .await
    .expect("and back");
    assert_eq!(
        vol.folder_metadata(&AppPath::legacy_unchecked(Path::new(&folder)), MetadataClasses::FLAGS)
            .await
            .flags,
        UserFlags::HIDDEN | UserFlags::LOCKED,
        "a folder reports its flags too"
    );

    for path in [&file, &folder] {
        let refused = vol.delete(&AppPath::legacy_unchecked(Path::new(path))).await;
        assert!(
            matches!(refused, Err(VolumeError::PermissionDenied { .. })),
            "{path}: a Read-only item refuses its removal as a permission refusal; got {refused:?}"
        );
        assert!(
            matches!(
                vol.clear_blockers(&AppPath::legacy_unchecked(Path::new(path))).await,
                Unblock::Cleared(_)
            ),
            "{path}: Read-only clears"
        );
        vol.delete(&AppPath::legacy_unchecked(Path::new(path)))
            .await
            .unwrap_or_else(|e| panic!("{path}: removable once cleared: {e:?}"));
    }
    assert_eq!(
        vol.clear_blockers(&AppPath::legacy_unchecked(Path::new(&base))).await,
        Unblock::NothingToClear,
        "a plain folder has nothing to clear"
    );

    ensure_clean(&vol, &base).await;
}

/// Samba reports every dotfile as Hidden (`hide dot files`, on by default). A
/// copy doesn't carry that: the name already hides it, and a local copy that
/// also wore `UF_HIDDEN` would stay hidden after a rename to a plain name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_a_dotfile_is_hidden_by_its_name_not_a_flag() {
    let vol = make_docker_volume().await;
    let base = test_dir_name();
    ensure_clean(&vol, &base).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&base)))
        .await
        .unwrap();
    let dotfile = format!("{base}/.profile");
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&dotfile)), b"dot")
        .await
        .unwrap();
    assert!(
        vol.attributes_impl(Path::new(&dotfile)).await.unwrap().is_hidden(),
        "fixture precondition: the server marks a dotfile Hidden"
    );

    assert_eq!(read_flags(&vol, &dotfile).await, UserFlags::NONE);

    ensure_clean(&vol, &base).await;
}
