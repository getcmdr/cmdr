use cmdr_fs::app_path::AppPath;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fs::deadline::Deadline;
use cmdr_fs::testing::WedgedVolume;
use cmdr_fs::volume::{InMemoryVolume, Volume, VolumeError};

use crate::{BlockReader, ByteSource, MAX_FETCHED_BYTES_PER_FILE, ReadError, VolumeByteSource};

const BLOCK: usize = 64 * 1024;

/// Bytes whose value says where they are, so a misplaced copy shows.
fn patterned(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

/// An in-memory file that records every `read_at`, answers at most `chunk`
/// bytes per call, and may claim a size its bytes don't reach.
struct Recording {
    data: Vec<u8>,
    claimed_size: u64,
    chunk: usize,
    calls: Arc<Mutex<Vec<(u64, usize)>>>,
}

impl Recording {
    fn new(data: Vec<u8>) -> Self {
        Self {
            claimed_size: data.len() as u64,
            data,
            chunk: usize::MAX,
            calls: Arc::default(),
        }
    }

    fn calls(&self) -> Arc<Mutex<Vec<(u64, usize)>>> {
        Arc::clone(&self.calls)
    }
}

impl ByteSource for Recording {
    fn size(&self) -> u64 {
        self.claimed_size
    }

    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        self.calls.lock().unwrap().push((offset, len));
        let start = (offset as usize).min(self.data.len());
        let end = start.saturating_add(len.min(self.chunk)).min(self.data.len());
        Ok(self.data[start..end].to_vec())
    }
}

fn fetched(calls: &Mutex<Vec<(u64, usize)>>) -> Vec<u64> {
    calls.lock().unwrap().iter().map(|(offset, _)| *offset).collect()
}

#[test]
fn reads_a_whole_file_across_blocks_one_fetch_per_block() {
    let data = patterned(3 * BLOCK + 100);
    let source = Recording::new(data.clone());
    let calls = source.calls();
    let mut reader = BlockReader::new(source);

    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();

    assert_eq!(out, data);
    assert_eq!(
        fetched(&calls),
        vec![0, BLOCK as u64, 2 * BLOCK as u64, 3 * BLOCK as u64]
    );
}

#[test]
fn seeks_from_start_end_and_current() {
    let data = patterned(2 * BLOCK);
    let mut reader = BlockReader::new(Recording::new(data.clone()));
    let mut two = [0u8; 2];

    assert_eq!(
        reader.seek(SeekFrom::Start(BLOCK as u64 - 1)).unwrap(),
        BLOCK as u64 - 1
    );
    reader.read_exact(&mut two).unwrap();
    assert_eq!(two, [data[BLOCK - 1], data[BLOCK]]);

    assert_eq!(reader.seek(SeekFrom::End(-2)).unwrap(), 2 * BLOCK as u64 - 2);
    reader.read_exact(&mut two).unwrap();
    assert_eq!(two, [data[2 * BLOCK - 2], data[2 * BLOCK - 1]]);

    assert_eq!(reader.seek(SeekFrom::Current(-4)).unwrap(), 2 * BLOCK as u64 - 4);
    assert!(reader.seek(SeekFrom::Current(-(4 * BLOCK as i64))).is_err());
}

#[test]
fn reading_at_or_past_the_end_answers_nothing_and_fetches_nothing() {
    let source = Recording::new(patterned(10));
    let calls = source.calls();
    let mut reader = BlockReader::new(source);
    let mut buf = [0u8; 4];

    reader.seek(SeekFrom::Start(10)).unwrap();
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
    reader.seek(SeekFrom::Start(1_000)).unwrap();
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
    assert!(fetched(&calls).is_empty());
}

#[test]
fn a_kept_block_is_not_fetched_again() {
    let source = Recording::new(patterned(2 * BLOCK));
    let calls = source.calls();
    let mut reader = BlockReader::new(source);
    let mut buf = [0u8; 16];

    reader.read_exact(&mut buf).unwrap();
    reader.seek(SeekFrom::Start(BLOCK as u64)).unwrap();
    reader.read_exact(&mut buf).unwrap();
    reader.seek(SeekFrom::Start(100)).unwrap();
    reader.read_exact(&mut buf).unwrap();

    assert_eq!(fetched(&calls), vec![0, BLOCK as u64]);
}

#[test]
fn the_least_recently_used_block_goes_first() {
    let source = Recording::new(patterned(10 * BLOCK));
    let calls = source.calls();
    let mut reader = BlockReader::new(source);
    let mut byte = [0u8; 1];
    let mut touch = |reader: &mut BlockReader<Recording>, block: usize| {
        reader.seek(SeekFrom::Start((block * BLOCK) as u64)).unwrap();
        reader.read_exact(&mut byte).unwrap();
    };

    // Eight blocks fill the cache; touching block 0 again makes block 1 the oldest.
    for block in 0..8 {
        touch(&mut reader, block);
    }
    touch(&mut reader, 0);
    touch(&mut reader, 8);
    touch(&mut reader, 0);
    touch(&mut reader, 1);

    let offsets: Vec<usize> = fetched(&calls).iter().map(|offset| *offset as usize / BLOCK).collect();
    assert_eq!(offsets, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 1]);
}

#[test]
fn fetching_past_the_budget_is_too_large() {
    let budget = MAX_FETCHED_BYTES_PER_FILE as usize;
    let source = Recording::new(patterned(budget + BLOCK));
    let calls = source.calls();
    let mut reader = BlockReader::new(source);

    let error = reader.read_to_end(&mut Vec::new()).unwrap_err();

    assert_eq!(ReadError::from_io(error), ReadError::TooLarge);
    assert_eq!(fetched(&calls).len(), budget / BLOCK);
}

#[test]
fn rereading_kept_blocks_costs_no_budget() {
    let source = Recording::new(patterned(BLOCK));
    let mut reader = BlockReader::new(source);
    let mut buf = [0u8; 8];

    for _ in 0..(2 * MAX_FETCHED_BYTES_PER_FILE as usize / BLOCK) {
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_exact(&mut buf).unwrap();
    }
}

#[test]
fn a_hole_reads_as_zeros_and_is_never_fetched() {
    let data = patterned(6 * BLOCK);
    let source = Recording::new(data.clone());
    let calls = source.calls();
    let hole = (BLOCK as u64 + 10)..(4 * BLOCK as u64 + 20);
    let mut reader = BlockReader::new(source).with_holes(vec![hole.clone()]);

    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();

    let mut expected = data;
    expected[hole.start as usize..hole.end as usize].fill(0);
    assert_eq!(out, expected);
    // Blocks 2 and 3 sit wholly inside the hole.
    let blocks: Vec<usize> = fetched(&calls).iter().map(|offset| *offset as usize / BLOCK).collect();
    assert_eq!(blocks, vec![0, 1, 4, 5]);
}

#[test]
fn holes_in_any_order_and_overlapping_merge() {
    let data = patterned(4 * BLOCK);
    let source = Recording::new(data.clone());
    let calls = source.calls();
    let holes = vec![
        (2 * BLOCK as u64)..(3 * BLOCK as u64 + 5),
        (BLOCK as u64)..(2 * BLOCK as u64 + 50),
        7..7,
    ];
    let mut reader = BlockReader::new(source).with_holes(holes);

    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();

    let mut expected = data;
    expected[BLOCK..3 * BLOCK + 5].fill(0);
    assert_eq!(out, expected);
    let blocks: Vec<usize> = fetched(&calls).iter().map(|offset| *offset as usize / BLOCK).collect();
    assert_eq!(blocks, vec![0, 3]);
}

#[test]
fn a_hole_past_the_end_stops_at_the_end() {
    let hole = 50..1_000;
    let mut reader = BlockReader::new(Recording::new(patterned(100))).with_holes(vec![hole]);
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    assert_eq!(out.len(), 100);
    assert!(out[50..].iter().all(|byte| *byte == 0));
}

#[test]
fn short_answers_are_asked_again_until_the_block_is_whole() {
    let data = patterned(BLOCK + 10);
    let mut source = Recording::new(data.clone());
    source.chunk = 1_000;
    let mut reader = BlockReader::new(source);

    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();

    assert_eq!(out, data);
}

#[test]
fn a_file_that_shrank_ends_where_its_bytes_do() {
    let data = patterned(BLOCK + 70);
    let mut source = Recording::new(data.clone());
    source.claimed_size = 3 * BLOCK as u64;
    let mut reader = BlockReader::new(source);

    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();

    assert_eq!(out, data);
}

#[test]
fn a_source_refusal_reaches_the_parser_typed() {
    struct Refusing;
    impl ByteSource for Refusing {
        fn size(&self) -> u64 {
            10
        }
        fn read_at(&self, _offset: u64, _len: usize) -> io::Result<Vec<u8>> {
            Err(ReadError::Cancelled.into_io())
        }
    }

    let error = BlockReader::new(Refusing).read(&mut [0u8; 4]).unwrap_err();

    assert_eq!(ReadError::from_io(error), ReadError::Cancelled);
}

// ---- VolumeByteSource ------------------------------------------------------

const FILE: &str = "/photo.jpg";

async fn volume_with(data: &[u8]) -> Arc<InMemoryVolume> {
    let volume = Arc::new(InMemoryVolume::new("test"));
    volume
        .create_file(&AppPath::legacy_unchecked(Path::new(FILE)), data)
        .await
        .unwrap();
    volume
}

/// Reads all of `FILE` through a block reader on a blocking thread, as the app does.
async fn read_through(
    volume: Arc<dyn Volume>,
    size: u64,
    deadline: Deadline,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<u8>, ReadError> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let source = VolumeByteSource::new(volume, PathBuf::from(FILE), size, handle, deadline, cancel);
        let mut out = Vec::new();
        BlockReader::new(source)
            .read_to_end(&mut out)
            .map(|_| out)
            .map_err(ReadError::from_io)
    })
    .await
    .unwrap()
}

fn roomy() -> Deadline {
    Deadline::new(Duration::from_secs(30))
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_a_volume_file_through_ranged_reads() {
    let data = patterned(BLOCK + 300);
    let volume = volume_with(&data).await;

    let out = read_through(volume.clone(), data.len() as u64, roomy(), Arc::default()).await;

    assert_eq!(out.unwrap(), data);
    assert_eq!(volume.read_range_log(), vec![(0, BLOCK), (BLOCK as u64, 300)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_set_cancel_flag_stops_before_asking_the_volume() {
    let volume = volume_with(&patterned(100)).await;

    let out = read_through(volume.clone(), 100, roomy(), Arc::new(AtomicBool::new(true))).await;

    assert_eq!(out, Err(ReadError::Cancelled));
    assert!(volume.read_range_log().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spent_deadline_times_out_before_asking_the_volume() {
    let volume = volume_with(&patterned(100)).await;

    let out = read_through(volume.clone(), 100, Deadline::new(Duration::ZERO), Arc::default()).await;

    assert_eq!(out, Err(ReadError::TimedOut));
    assert!(volume.read_range_log().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_that_never_answers_times_out() {
    let volume: Arc<dyn Volume> = Arc::new(WedgedVolume::new("wedged"));

    let out = read_through(volume, 100, Deadline::new(Duration::from_millis(20)), Arc::default()).await;

    assert_eq!(out, Err(ReadError::TimedOut));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_without_ranged_reads_is_not_supported() {
    let volume = Arc::new(InMemoryVolume::new("test").with_read_range_failing(VolumeError::NotSupported));
    volume
        .create_file(&AppPath::legacy_unchecked(Path::new(FILE)), b"bytes")
        .await
        .unwrap();

    let out = read_through(volume, 5, roomy(), Arc::default()).await;

    assert_eq!(out, Err(ReadError::NotSupported));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_file_is_an_io_error() {
    let volume: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("test"));

    let out = read_through(volume, 5, roomy(), Arc::default()).await;

    assert!(matches!(out, Err(ReadError::Io(_))), "{out:?}");
}
