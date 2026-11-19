//! A LOCAL source streaming onto a real SMB share.
//!
//! `cmdr-smb`'s own `write_stream_integration_test` covers what a write does with
//! a source in general; this cell is here because its source is a second app
//! backend, `LocalVolume`, which is the pairing every local-to-share copy
//! actually runs.
//!
//! Same gating as every SMB integration cell: `#[ignore]`d by default, so start
//! the containers with `./apps/desktop/test/smb-servers/start.sh` and run
//! `cargo nextest run smb_integration --run-ignored all`.

use cmdr_fs::app_path::AppPath;
use std::path::Path;
use std::sync::atomic::Ordering;

use cmdr_fs::testing::TestDir;

use super::smb_test_support::*;
use cmdr_fs::volume::{StreamLength, WriteMode};

/// A multi-MB local file streams onto the share through `LocalVolume`'s
/// `open_read_stream` into `SmbVolume`'s `write_from_stream`: progress fires more
/// than once (so the chunked path really ran) and the destination hashes equal to
/// the source.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_write_from_stream_local_source_large_file() {
    use std::sync::atomic::{AtomicU64, AtomicUsize};

    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    ensure_clean(&vol, &dir).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&dir)))
        .await
        .unwrap();

    let size = 4 * 1024 * 1024; // 4 MB, spans multiple import chunks
    let data: Vec<u8> = (0..size).map(|i| ((i * 13) % 251) as u8).collect();

    let local_tmp = TestDir::new("smb-import");
    std::fs::write(local_tmp.join("import-large.bin"), &data).unwrap();

    let local_vol =
        cmdr_local::LocalVolume::local_folder("local-src", local_tmp.to_path_buf(), crate::volume_host::host());

    let smb_path = format!("{}/import-large.bin", dir);
    let progress_calls = AtomicUsize::new(0);
    let last_bytes = AtomicU64::new(0);

    let stream = local_vol
        .open_read_stream(&AppPath::legacy_unchecked(Path::new("import-large.bin")))
        .await
        .unwrap();
    assert_eq!(stream.total_size(), StreamLength::Known(size as u64));

    let bytes = vol
        .write_from_stream(
            &AppPath::legacy_unchecked(Path::new(&smb_path)),
            WriteMode::CreateOrReplace,
            StreamLength::Known(size as u64),
            stream,
            &|progress| {
                progress_calls.fetch_add(1, Ordering::Relaxed);
                last_bytes.store(progress.bytes_written, Ordering::Relaxed);
                assert_eq!(progress.expected_length, StreamLength::Known(size as u64));
                std::ops::ControlFlow::Continue(())
            },
        )
        .await
        .unwrap();

    assert_eq!(bytes.bytes, size as u64);
    assert!(
        progress_calls.load(Ordering::Relaxed) >= 2,
        "streaming write should call progress multiple times for a multi-chunk source"
    );
    assert_eq!(last_bytes.load(Ordering::Relaxed), size as u64);

    // Byte-level integrity: hash the source and the destination and
    // compare. Streaming hash avoids materializing a 4 MB `Vec<u8>`
    // just to `assert_eq!` it, and on mismatch we get a legible hex
    // dump instead of a multi-megabyte diff.
    let expected_hash = hash_bytes(&data);
    let actual_hash = hash_volume_file(&vol as &dyn Volume, Path::new(&smb_path)).await;
    assert_eq!(
        actual_hash, expected_hash,
        "SMB destination bytes must match source (expected blake3 {:x?}, got {:x?})",
        expected_hash, actual_hash
    );

    ensure_clean(&vol, &dir).await;
}

/// ERR-9JY7Q: a Synology share refused a staged write of a name already at the
/// 255-byte limit, because the temp's suffix pushed it over
/// (`STATUS_OBJECT_NAME_INVALID` on the temp's open). The fixture's Samba sits on
/// a Linux filesystem with the same limit, so this is the same refusal. Staged
/// exactly as the transfer engine stages it: a minted temp, `CreateNew`, then
/// the landing rename. ASCII, and Greek at two bytes a letter.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_staged_write_of_a_name_at_the_byte_limit_lands() {
    use cmdr_fs::staging::{StagingTemp, is_staging_temp_name};

    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    ensure_clean(&vol, &dir).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&dir)))
        .await
        .unwrap();

    let data = b"a book, at last".to_vec();
    let local_tmp = TestDir::new("smb-long-name");
    std::fs::write(local_tmp.join("book.epub"), &data).unwrap();
    let local_vol =
        cmdr_local::LocalVolume::local_folder("local-src", local_tmp.to_path_buf(), crate::volume_host::host());

    for name in [
        format!("{}.epub", "a".repeat(250)),
        format!("{}.epub", "αβγδε".repeat(25)),
    ] {
        assert_eq!(name.len(), 255);
        let final_path = format!("{dir}/{name}");
        let temp = StagingTemp::mint(Path::new(&final_path), None);

        let stream = local_vol
            .open_read_stream(&AppPath::legacy_unchecked(Path::new("book.epub")))
            .await
            .unwrap();
        vol.write_from_stream(
            &AppPath::legacy_unchecked(temp.path()),
            WriteMode::CreateNew,
            StreamLength::Known(data.len() as u64),
            stream,
            &|_| std::ops::ControlFlow::Continue(()),
        )
        .await
        .unwrap_or_else(|e| panic!("the share refused the temp {}: {e:?}", temp.path().display()));
        vol.rename(
            &AppPath::legacy_unchecked(temp.path()),
            &AppPath::legacy_unchecked(Path::new(&final_path)),
            false,
        )
        .await
        .unwrap_or_else(|e| panic!("the share refused the landing onto {final_path}: {e:?}"));

        assert_eq!(
            hash_volume_file(&vol as &dyn Volume, Path::new(&final_path)).await,
            hash_bytes(&data)
        );
    }

    let names: Vec<String> = vol
        .list_directory(&AppPath::legacy_unchecked(Path::new(&dir)), None)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(
        !names.iter().any(|n| is_staging_temp_name(n)),
        "a temp survived: {names:?}"
    );
    assert_eq!(names.len(), 2, "got {names:?}");

    ensure_clean(&vol, &dir).await;
}

/// The other half of the honest-error promise: a FINAL name the share really
/// can't hold (past 255 bytes) still comes back as `InvalidName`, the variant
/// that tells the user to pick a shorter name. The shortened temp takes the
/// bytes, so it's the landing rename that has to refuse.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_staged_write_of_a_name_past_the_byte_limit_is_an_invalid_name() {
    use cmdr_fs::staging::StagingTemp;

    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    ensure_clean(&vol, &dir).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&dir)))
        .await
        .unwrap();

    let data = b"too long a name".to_vec();
    let local_tmp = TestDir::new("smb-too-long-name");
    std::fs::write(local_tmp.join("book.epub"), &data).unwrap();
    let local_vol =
        cmdr_local::LocalVolume::local_folder("local-src", local_tmp.to_path_buf(), crate::volume_host::host());

    // 150 Greek letters: 300 bytes, which a Windows server would hold (150
    // UTF-16 units) but a Linux one can't.
    let final_path = format!("{dir}/{}.epub", "αβγδε".repeat(30));
    let temp = StagingTemp::mint(Path::new(&final_path), None);
    let stream = local_vol
        .open_read_stream(&AppPath::legacy_unchecked(Path::new("book.epub")))
        .await
        .unwrap();
    vol.write_from_stream(
        &AppPath::legacy_unchecked(temp.path()),
        WriteMode::CreateNew,
        StreamLength::Known(data.len() as u64),
        stream,
        &|_| std::ops::ControlFlow::Continue(()),
    )
    .await
    .expect("the shortened temp fits");

    let refused = vol
        .rename(
            &AppPath::legacy_unchecked(temp.path()),
            &AppPath::legacy_unchecked(Path::new(&final_path)),
            false,
        )
        .await
        .expect_err("a 300-byte name can't land on a Linux share");
    assert!(matches!(refused, VolumeError::InvalidName(_)), "got {refused:?}");

    vol.delete(&AppPath::legacy_unchecked(temp.path())).await.unwrap();
    ensure_clean(&vol, &dir).await;
}
