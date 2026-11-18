//! A copy that never leaves the server (`FSCTL_SRV_COPYCHUNK`), and the
//! refusal a server without it answers.
//!
//! The `smb_integration_*` cells need the Docker SMB containers
//! (`apps/desktop/test/smb-servers/start.sh`) and run under
//! `cargo nextest run -p cmdr-smb --run-ignored only -E 'test(smb_integration_copy)'`.
//! Every cell works in a unique directory of its own, so parallel cells never
//! delete each other's files.

use super::super::SmbVolume;
use super::super::test_support::*;
use super::COPY_SLICE_BYTES;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::conformance::{BIRTH_SECS, SOURCE_DATE_SECS};
use cmdr_fs::volume::{ServerCopyProgress, WriteMode};
use std::ops::ControlFlow;
use std::path::Path;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

/// Two whole slices and a tail that isn't slice-aligned, so the loop's middle,
/// its boundary, and its last short request all run.
const PAYLOAD: usize = 2 * COPY_SLICE_BYTES as usize + 12_345;

/// A connected volume and an empty directory of its own on the share.
async fn volume_with_scratch() -> (SmbVolume, String) {
    let vol = make_docker_volume().await;
    let dir = test_dir_name();
    ensure_clean(&vol, &dir).await;
    vol.create_directory(&AppPath::legacy_unchecked(Path::new(&dir)))
        .await
        .unwrap();
    (vol, dir)
}

/// Bytes that differ from slice to slice, so a slice copied to the wrong offset
/// changes the hash.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8 ^ (i / 65_536) as u8).collect()
}

/// `copy_within` without a progress hook.
async fn copy(vol: &SmbVolume, from: &str, to: &str) -> Result<u64, VolumeError> {
    vol.copy_within(
        &AppPath::legacy_unchecked(Path::new(from)),
        &AppPath::legacy_unchecked(Path::new(to)),
        &|_, _| ControlFlow::Continue(()),
    )
    .await
}

/// A `ServerCopyProgress` that counts its checkpoints and stops at the `stop_at`th.
struct CountingProgress {
    checkpoints: AtomicUsize,
    stop_at: usize,
}

impl ServerCopyProgress for CountingProgress {
    fn advanced(&self, _done: u64, _total: u64) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn checkpoint(&self) -> Pin<Box<dyn Future<Output = ControlFlow<()>> + Send + '_>> {
        Box::pin(async move {
            let seen = self.checkpoints.fetch_add(1, Ordering::SeqCst) + 1;
            if seen >= self.stop_at {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
    }
}

// ── Without a server ─────────────────────────────────────────────────

/// A server without copychunk answers the resume-key request with
/// `STATUS_NOT_SUPPORTED` (or `INVALID_DEVICE_REQUEST`), which must reach the
/// engine as `NotSupported`, its "stream it instead" signal, ❌ never as an I/O
/// failure.
#[test]
fn a_server_without_copychunk_answers_not_supported() {
    use smb2::types::Command;
    use smb2::types::status::NtStatus;

    let vol = make_test_volume();
    for status in [NtStatus::NOT_SUPPORTED, NtStatus::INVALID_DEVICE_REQUEST] {
        let refused: Result<(), smb2::Error> = Err(smb2::Error::Protocol {
            status,
            command: Command::Ioctl,
        });
        assert!(
            matches!(
                vol.handle_copy_result("copy_within(resume key)", "a.bin", SessionGeneration::FIRST, refused),
                Err(VolumeError::NotSupported)
            ),
            "{status:?} must read as NotSupported"
        );
    }
}

/// Any other refusal keeps its ordinary mapping, so a missing source still
/// says which file.
#[test]
fn other_refusals_keep_their_ordinary_mapping() {
    use smb2::types::Command;
    use smb2::types::status::NtStatus;

    let vol = make_test_volume();
    let missing: Result<(), smb2::Error> = Err(smb2::Error::Protocol {
        status: NtStatus::OBJECT_NAME_NOT_FOUND,
        command: Command::Create,
    });
    assert!(matches!(
        vol.handle_copy_result("copy_within(open source)", "a.bin", SessionGeneration::FIRST, missing),
        Err(VolumeError::NotFound(_))
    ));
}

// ── Against the fixture ──────────────────────────────────────────────

/// Samba copies for itself, byte for byte, across several slices.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_lands_byte_for_byte() {
    let (vol, dir) = volume_with_scratch().await;
    let bytes = payload(PAYLOAD);
    let (from, to) = (format!("{dir}/source.bin"), format!("{dir}/copy.bin"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), &bytes)
        .await
        .unwrap();

    let copied = copy(&vol, &from, &to).await.expect("Samba does server-side copy");

    assert_eq!(copied, PAYLOAD as u64);
    assert_eq!(
        hash_volume_file(&vol, Path::new(&to)).await,
        hash_bytes(&bytes),
        "the server-side copy must hold exactly the source's bytes"
    );
    ensure_clean(&vol, &dir).await;
}

/// Progress climbs once per slice to the whole file, and never past it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_reports_progress_up_to_the_whole_file() {
    let (vol, dir) = volume_with_scratch().await;
    let (from, to) = (format!("{dir}/source.bin"), format!("{dir}/copy.bin"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), &payload(PAYLOAD))
        .await
        .unwrap();

    let seen: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    vol.copy_within(
        &AppPath::legacy_unchecked(Path::new(&from)),
        &AppPath::legacy_unchecked(Path::new(&to)),
        &|done, total| {
            seen.lock_ignore_poison().push((done, total));
            ControlFlow::Continue(())
        },
    )
    .await
    .unwrap();

    let seen = seen.lock_ignore_poison().clone();
    assert_eq!(seen.len(), 3, "one report per slice: {seen:?}");
    assert!(
        seen.windows(2).all(|w| w[0].0 < w[1].0),
        "progress only climbs: {seen:?}"
    );
    assert!(seen.iter().all(|&(_, total)| total == PAYLOAD as u64), "{seen:?}");
    assert_eq!(seen.last().map(|&(done, _)| done), Some(PAYLOAD as u64));
    ensure_clean(&vol, &dir).await;
}

/// A Cancel between slices stops the copy, takes the partial with it, and
/// leaves the source alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_cancel_leaves_nothing_behind() {
    let (vol, dir) = volume_with_scratch().await;
    let bytes = payload(PAYLOAD);
    let (from, to) = (format!("{dir}/source.bin"), format!("{dir}/copy.bin"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), &bytes)
        .await
        .unwrap();

    let result = vol
        .copy_within(
            &AppPath::legacy_unchecked(Path::new(&from)),
            &AppPath::legacy_unchecked(Path::new(&to)),
            &|_, _| ControlFlow::Break(()),
        )
        .await;

    assert!(matches!(result, Err(VolumeError::Cancelled(_))), "{result:?}");
    assert!(
        !vol.exists(&AppPath::legacy_unchecked(Path::new(&to))).await,
        "a canceled copy must not leave its partial"
    );
    assert_eq!(hash_volume_file(&vol, Path::new(&from)).await, hash_bytes(&bytes));
    ensure_clean(&vol, &dir).await;
}

/// The engine's entry point parks at a checkpoint before each slice, so a
/// pause lands mid-file, and a Cancel there leaves nothing behind either.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_on_server_checks_in_before_every_slice() {
    let (vol, dir) = volume_with_scratch().await;
    let (from, to) = (format!("{dir}/source.bin"), format!("{dir}/copy.bin"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), &payload(PAYLOAD))
        .await
        .unwrap();

    let all_the_way = CountingProgress {
        checkpoints: AtomicUsize::new(0),
        stop_at: usize::MAX,
    };
    vol.copy_on_server(
        &vol,
        &AppPath::legacy_unchecked(Path::new(&from)),
        &AppPath::legacy_unchecked(Path::new(&to)),
        WriteMode::CreateOrReplace,
        &all_the_way,
    )
    .await
    .unwrap();
    assert_eq!(
        all_the_way.checkpoints.load(Ordering::SeqCst),
        3,
        "one checkpoint per slice"
    );

    let stopped = CountingProgress {
        checkpoints: AtomicUsize::new(0),
        stop_at: 2,
    };
    let second = format!("{dir}/second.bin");
    let result = vol
        .copy_on_server(
            &vol,
            &AppPath::legacy_unchecked(Path::new(&from)),
            &AppPath::legacy_unchecked(Path::new(&second)),
            WriteMode::CreateOrReplace,
            &stopped,
        )
        .await;
    assert!(matches!(result, Err(VolumeError::Cancelled(_))), "{result:?}");
    assert!(
        !vol.exists(&AppPath::legacy_unchecked(Path::new(&second))).await,
        "a stop at a checkpoint must not leave its partial"
    );
    ensure_clean(&vol, &dir).await;
}

/// The trait's contract: `to` is TRUNCATED when it exists, so a longer old
/// file leaves no tail behind the copy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_replaces_a_longer_destination_whole() {
    let (vol, dir) = volume_with_scratch().await;
    let (from, to) = (format!("{dir}/short.txt"), format!("{dir}/long.txt"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), b"short")
        .await
        .unwrap();
    vol.create_file(
        &AppPath::legacy_unchecked(Path::new(&to)),
        b"a much longer file that was here first",
    )
    .await
    .unwrap();

    copy(&vol, &from, &to).await.unwrap();

    assert_eq!(
        vol.read_range(&AppPath::legacy_unchecked(Path::new(&to)), 0, 64)
            .await
            .unwrap(),
        b"short"
    );
    ensure_clean(&vol, &dir).await;
}

/// An empty source copies to an empty file and still reports once, so a
/// caller's bar reaches 100%.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_copies_an_empty_file() {
    let (vol, dir) = volume_with_scratch().await;
    let (from, to) = (format!("{dir}/empty.txt"), format!("{dir}/copy.txt"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), b"")
        .await
        .unwrap();

    let seen: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    let copied = vol
        .copy_within(
            &AppPath::legacy_unchecked(Path::new(&from)),
            &AppPath::legacy_unchecked(Path::new(&to)),
            &|done, total| {
                seen.lock_ignore_poison().push((done, total));
                ControlFlow::Continue(())
            },
        )
        .await
        .unwrap();

    assert_eq!(copied, 0);
    assert_eq!(*seen.lock_ignore_poison(), vec![(0, 0)]);
    assert_eq!(
        vol.get_metadata(&AppPath::legacy_unchecked(Path::new(&to)))
            .await
            .unwrap()
            .size,
        Some(0)
    );
    ensure_clean(&vol, &dir).await;
}

/// Copychunk moves bytes only, so the source's dates are ours to carry, as a
/// streamed copy carries them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_keeps_the_source_dates() {
    let (vol, dir) = volume_with_scratch().await;
    let (from, to) = (format!("{dir}/aged.txt"), format!("{dir}/copy.txt"));
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), b"bytes from 2021")
        .await
        .unwrap();
    let at = |secs| std::time::UNIX_EPOCH + Duration::from_secs(secs);
    vol.set_basic_info_impl(
        Path::new(&from),
        smb2::FileTimes::new()
            .set_modified(at(SOURCE_DATE_SECS))
            .set_created(at(BIRTH_SECS)),
    )
    .await
    .unwrap();

    copy(&vol, &from, &to).await.unwrap();

    let landed = vol
        .get_metadata(&AppPath::legacy_unchecked(Path::new(&to)))
        .await
        .unwrap();
    assert_eq!(
        landed.modified_at,
        Some(SOURCE_DATE_SECS),
        "the copy keeps the source's date"
    );
    assert_eq!(
        landed.created_at,
        Some(BIRTH_SECS),
        "the copy keeps the source's creation date"
    );
    ensure_clean(&vol, &dir).await;
}

/// The shared handshake: a backend that copies must also say
/// `supports_copy_within`, or the engine never asks it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn smb_integration_copy_within_matches_what_is_offered() {
    let (vol, dir) = volume_with_scratch().await;
    let from = format!("{dir}/original.txt");
    vol.create_file(&AppPath::legacy_unchecked(Path::new(&from)), b"copied on the server")
        .await
        .unwrap();

    cmdr_fs::volume::conformance::assert_copy_within_matches_what_is_offered(
        &vol,
        Path::new(&from),
        Path::new(&format!("{dir}/duplicate.txt")),
    )
    .await;
    ensure_clean(&vol, &dir).await;
}
