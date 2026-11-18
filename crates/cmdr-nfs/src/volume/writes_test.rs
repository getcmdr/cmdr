//! The byte path into the export against `fake_nfs.rs`: the window, the
//! rolling COMMIT and its verifier, short WRITEs, `wtmax`, cancel, a source
//! that lies about its length, and the order SETATTR rides in.

use cmdr_fs::app_path::AppPath;
use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cmdr_fs::metadata::{CarriedMetadata, MetadataClasses};
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{StreamLength, StreamWriteProgress, Volume, VolumeError, VolumeReadStream, WriteMode};
use tokio_util::sync::CancellationToken;

use super::super::fake_nfs::{FakeNfs, proc};
use super::super::{NfsVolume, connect_with};
use crate::dial::DialOptions;

/// A source handing out `bytes` in 100 KiB chunks, dated and moded like a real
/// file, declaring `declared` as its length.
struct Source {
    chunks: VecDeque<Vec<u8>>,
    declared: StreamLength,
    read: u64,
    modified: Option<SystemTime>,
    mode: Option<u32>,
}

impl Source {
    fn of(bytes: &[u8]) -> Self {
        Self {
            chunks: bytes.chunks(100 * 1024).map(<[u8]>::to_vec).collect(),
            declared: StreamLength::Known(bytes.len() as u64),
            read: 0,
            modified: Some(UNIX_EPOCH + Duration::new(1_611_909_015, 250_000_000)),
            mode: Some(0o640),
        }
    }

    fn declaring(mut self, length: u64) -> Self {
        self.declared = StreamLength::Known(length);
        self
    }
}

impl VolumeReadStream for Source {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        let next = self.chunks.pop_front();
        if let Some(chunk) = &next {
            self.read += chunk.len() as u64;
        }
        Box::pin(async move { next.map(Ok) })
    }

    fn total_size(&self) -> StreamLength {
        self.declared
    }

    fn bytes_read(&self) -> u64 {
        self.read
    }

    fn modified_at(&self) -> Option<SystemTime> {
        self.modified
    }

    fn metadata(&self, wanted: MetadataClasses) -> CarriedMetadata {
        CarriedMetadata {
            mode: self.mode,
            ..CarriedMetadata::default()
        }
        .restricted_to(wanted)
    }
}

/// Every byte its own position, so a misplaced chunk can't checksum right.
fn patterned(len: usize) -> Vec<u8> {
    (0..len).map(|at| (at % 251) as u8).collect()
}

async fn over_pipe(fake: &FakeNfs) -> NfsVolume {
    connect_with(
        "fake",
        fake.params(),
        VolumeHost::builder().runtime(tokio::runtime::Handle::current()).build(),
        fake.duplex_dialer(DialOptions::PRODUCTION),
        CancellationToken::new(),
    )
    .await
    .expect("the fake mounts")
}

fn at(volume: &NfsVolume, remote: &str) -> PathBuf {
    PathBuf::from(format!("{}{remote}", volume.root().as_std_legacy().display()))
}

fn keep_going(_: StreamWriteProgress) -> ControlFlow<()> {
    ControlFlow::Continue(())
}

async fn write(volume: &NfsVolume, remote: &str, mode: WriteMode, source: Source) -> Result<u64, VolumeError> {
    let length = source.declared;
    volume
        .write_from_stream(
            &AppPath::legacy_unchecked(&at(volume, remote)),
            mode,
            length,
            Box::new(source),
            &keep_going,
        )
        .await
        .map(|written| written.bytes)
}

#[tokio::test]
async fn a_write_across_two_commit_spans_lands_every_byte_and_commits_each() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(9 * 1024 * 1024 + 123);

    let written = write(&volume, "/big.bin", WriteMode::CreateNew, Source::of(&bytes)).await;

    assert_eq!(written.unwrap(), bytes.len() as u64);
    assert_eq!(fake.contents("/big.bin").unwrap(), bytes);
    assert_eq!(
        fake.calls(proc::COMMIT),
        2,
        "one COMMIT per 8 MiB span, the last one short"
    );
}

#[tokio::test]
async fn a_verifier_that_changes_before_the_commit_resends_the_span_and_lands() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(640 * 1024);
    // A reboot between the WRITEs and their COMMIT: the COMMIT's verifier
    // isn't the one the WRITEs answered, so they may be lost.
    fake.rotate_verifier_on(proc::COMMIT, 1);

    let written = write(&volume, "/rebooted.bin", WriteMode::CreateNew, Source::of(&bytes)).await;

    assert_eq!(written.unwrap(), bytes.len() as u64);
    assert_eq!(fake.contents("/rebooted.bin").unwrap(), bytes);
    assert_eq!(fake.calls(proc::WRITE), 20, "the ten 64 KiB WRITEs, then all ten again");
    assert_eq!(fake.calls(proc::COMMIT), 2);
}

#[tokio::test]
async fn a_verifier_that_changes_among_the_writes_resends_the_span() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(640 * 1024);
    fake.rotate_verifier_on(proc::WRITE, 4);

    let written = write(&volume, "/mixed.bin", WriteMode::CreateNew, Source::of(&bytes)).await;

    assert_eq!(written.unwrap(), bytes.len() as u64);
    assert_eq!(fake.contents("/mixed.bin").unwrap(), bytes);
    assert_eq!(fake.calls(proc::WRITE), 20);
}

#[tokio::test]
async fn a_verifier_that_changes_twice_fails_the_write_and_leaves_nothing() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(640 * 1024);
    fake.rotate_verifier_on(proc::COMMIT, 1);
    fake.rotate_verifier_on(proc::COMMIT, 2);

    let written = write(&volume, "/twice.bin", WriteMode::CreateNew, Source::of(&bytes)).await;

    assert!(
        matches!(written, Err(VolumeError::IoError { .. })),
        "two reboots in one file is a failure, ❌ never a third try: {written:?}"
    );
    assert!(!fake.exists("/twice.bin"), "the partial goes with the failure");
}

#[tokio::test]
async fn a_short_write_is_resent_from_where_the_server_stopped() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(300 * 1024 + 7);
    fake.answer_writes_short(10_000);

    let written = write(&volume, "/short.bin", WriteMode::CreateNew, Source::of(&bytes)).await;

    assert_eq!(written.unwrap(), bytes.len() as u64);
    assert_eq!(
        fake.contents("/short.bin").unwrap(),
        bytes,
        "❌ never a short WRITE taken as done"
    );
}

#[tokio::test]
async fn no_write_carries_more_than_the_servers_wtmax() {
    let fake = FakeNfs::new();
    fake.set_transfer_size(1024 * 1024);
    fake.set_write_max(16 * 1024);
    let volume = over_pipe(&fake).await;
    let bytes = patterned(200 * 1024);

    write(&volume, "/capped.bin", WriteMode::CreateNew, Source::of(&bytes))
        .await
        .unwrap();

    assert_eq!(fake.contents("/capped.bin").unwrap(), bytes);
    assert!(
        fake.largest_write() <= 16 * 1024,
        "largest WRITE: {}",
        fake.largest_write()
    );
}

#[tokio::test]
async fn the_create_carries_only_the_mode_and_setattr_follows_the_last_write() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(200 * 1024);

    write(&volume, "/dated.bin", WriteMode::CreateNew, Source::of(&bytes))
        .await
        .unwrap();

    let create = fake.last_create().expect("a CREATE");
    assert_eq!(create.how, 1, "GUARDED");
    assert!(create.mode.is_some() && !create.owner_set && create.size.is_none() && !create.times_set);
    let log = fake.call_log();
    let last_write = log.iter().rposition(|&call| call == proc::WRITE).unwrap();
    let setattr = log.iter().rposition(|&call| call == proc::SETATTR).expect("a SETATTR");
    assert!(
        setattr > last_write,
        "the date and mode go on after the last WRITE: {log:?}"
    );
    let (mode, atime, mtime) = fake.mode_and_times("/dated.bin").unwrap();
    assert_eq!(mode & 0o777, 0o640);
    assert_eq!((mtime.seconds, mtime.nseconds), (1_611_909_015, 250_000_000));
    assert_eq!(atime, mtime);
}

#[tokio::test]
async fn create_new_refuses_a_taken_name_and_leaves_it_alone() {
    let fake = FakeNfs::new();
    fake.add_file("/notes.txt", b"the user's notes");
    let volume = over_pipe(&fake).await;

    let written = write(&volume, "/notes.txt", WriteMode::CreateNew, Source::of(b"new")).await;

    assert!(matches!(written, Err(VolumeError::AlreadyExists(_))), "{written:?}");
    assert_eq!(fake.contents("/notes.txt").unwrap(), b"the user's notes");
}

#[tokio::test]
async fn create_or_replace_overwrites_a_longer_file_whole() {
    let fake = FakeNfs::new();
    fake.add_file("/.cmdr-tmp-1", &[b'x'; 5000]);
    let volume = over_pipe(&fake).await;

    write(
        &volume,
        "/.cmdr-tmp-1",
        WriteMode::CreateOrReplace,
        Source::of(b"short"),
    )
    .await
    .unwrap();

    assert_eq!(
        fake.contents("/.cmdr-tmp-1").unwrap(),
        b"short",
        "truncated, ❌ never a tail left"
    );
}

#[tokio::test]
async fn a_cancel_from_the_progress_callback_stops_and_removes_the_partial() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let bytes = patterned(2 * 1024 * 1024);
    let seen = AtomicU64::new(0);
    let stop_after_one = |progress: StreamWriteProgress| {
        seen.store(progress.bytes_written, Ordering::SeqCst);
        ControlFlow::Break(())
    };

    let written = volume
        .write_from_stream(
            &AppPath::legacy_unchecked(&at(&volume, "/cancelled.bin")),
            WriteMode::CreateNew,
            StreamLength::Known(bytes.len() as u64),
            Box::new(Source::of(&bytes)),
            &stop_after_one,
        )
        .await;

    assert!(matches!(written, Err(VolumeError::Cancelled(_))), "{written:?}");
    assert!(seen.load(Ordering::SeqCst) > 0, "progress reports acknowledged bytes");
    assert!(!fake.exists("/cancelled.bin"), "nothing left behind");
}

#[tokio::test]
async fn a_source_shorter_than_it_declared_fails_and_removes_the_partial() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;

    let written = write(
        &volume,
        "/liar.bin",
        WriteMode::CreateNew,
        Source::of(&patterned(1000)).declaring(2000),
    )
    .await;

    assert!(matches!(written, Err(VolumeError::IoError { .. })), "{written:?}");
    assert!(!fake.exists("/liar.bin"));
}

#[tokio::test]
async fn create_file_lands_its_content_and_refuses_to_clobber() {
    let fake = FakeNfs::new();
    let volume = over_pipe(&fake).await;
    let content = patterned(150 * 1024);

    volume
        .create_file(&AppPath::legacy_unchecked(&at(&volume, "/new.txt")), &content)
        .await
        .unwrap();
    let again = volume
        .create_file(&AppPath::legacy_unchecked(&at(&volume, "/new.txt")), b"other")
        .await;

    assert_eq!(fake.contents("/new.txt").unwrap(), content);
    assert!(matches!(again, Err(VolumeError::AlreadyExists(_))), "{again:?}");
    assert_eq!(fake.contents("/new.txt").unwrap(), content, "❌ never truncated");
}
