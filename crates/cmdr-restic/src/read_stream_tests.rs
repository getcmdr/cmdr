//! A snapshot file's stream reads ahead of the chunk it hands out, in order,
//! and holds a damaged file to its recorded size.

use crate::ResticVolume;
use crate::hub::{ResticRepos, Tuning};
use crate::path::VIRTUAL_ENTRY_NAME;
use crate::test_fixtures::{self, FixtureRepo};
use cmdr_fs::app_path::AppPath;
use cmdr_fs::testing::{TestDir, wait_until_async};
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{InMemoryVolume, Volume};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const PASSWORD: &str = "read ahead";
const ROOT: &str = "/share/repo";
/// The read-ahead this file pins, spelled out so a change to `READ_AHEAD`
/// shows up here as a decision rather than passing quietly.
const AHEAD_PINNED: usize = 4;
/// More chunks than the read-ahead window plus the one handed out, so the
/// stream has somewhere to read ahead to. ❗ No bigger than that: the fixture
/// backs this file up in a debug build, and at 12 MiB the suite under load
/// pushed these cells past their deadline.
const SIZE: usize = (AHEAD_PINNED + 2) * super::CHUNK_SIZE;

/// Deterministic, incompressible bytes, so the backup chunks the same way on
/// every run. Eight bytes per xorshift step keeps it cheap in a debug build.
fn noise() -> Vec<u8> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut bytes = Vec::with_capacity(SIZE);
    while bytes.len() < SIZE {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        bytes.extend_from_slice(&state.to_le_bytes());
    }
    bytes.truncate(SIZE);
    bytes
}

/// A repo holding `big.bin` on a remote-style parent, unlocked, and the file's
/// path in `latest`.
async fn big_file() -> (TestDir, Arc<InMemoryVolume>, ResticVolume, PathBuf) {
    let source = TestDir::new("restic_read_ahead_source");
    std::fs::write(source.join("big.bin"), noise()).expect("source file");
    let fixture = FixtureRepo::init(PASSWORD);
    fixture.backup(&source, SystemTime::now());
    let parent = Arc::new(InMemoryVolume::new("share"));
    fixture.write_into(parent.as_ref(), Path::new(ROOT)).await;
    let hub = Arc::new(ResticRepos::with_tuning(VolumeHost::detached(), Tuning::default()));
    let volume = ResticVolume::new(
        hub,
        "smb-share",
        Arc::clone(&parent) as Arc<dyn Volume>,
        PathBuf::from(ROOT),
    );
    assert!(test_fixtures::unlock(&volume, PASSWORD).await);
    let canonical = std::fs::canonicalize(&*source).expect("canonical source");
    let path = Path::new(ROOT)
        .join(VIRTUAL_ENTRY_NAME)
        .join("latest")
        .join(canonical.strip_prefix("/").expect("absolute"))
        .join("big.bin");
    (source, parent, volume, path)
}

/// One chunk handed out, and the pack reads behind the next ones already
/// asked for: a copy over a slow link never waits on one blob at a time.
#[tokio::test(flavor = "multi_thread")]
async fn the_stream_reads_the_next_chunks_before_they_are_asked_for() {
    let (_source, parent, volume, path) = big_file().await;
    let mut stream = volume
        .open_read_stream(&AppPath::legacy_unchecked(&path))
        .await
        .expect("opens");
    let before = parent.read_range_log().len();

    let first = stream.next_chunk().await.expect("a chunk").expect("reads");
    assert_eq!(first.len(), super::CHUNK_SIZE);

    // Blob bytes read for the file so far, with nobody asking past chunk one.
    // The bytes don't compress, so every chunk read costs at least a chunk.
    let read_so_far = || {
        parent.read_range_log()[before..]
            .iter()
            .map(|(_, len)| *len)
            .sum::<usize>()
    };
    wait_until_async(Duration::from_secs(5), "the chunks after the first are read", || {
        read_so_far() >= AHEAD_PINNED * super::CHUNK_SIZE
    })
    .await;
}

/// Read ahead or not, the bytes come out whole and in order.
#[tokio::test(flavor = "multi_thread")]
async fn the_chunks_come_out_in_order() {
    let (_source, _parent, volume, path) = big_file().await;
    let mut stream = volume
        .open_read_stream(&AppPath::legacy_unchecked(&path))
        .await
        .expect("opens");
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.expect("a chunk"));
    }
    assert_eq!(out, noise());
}
