//! Benchmarks for the write engine's hot paths: the referee for any seam that
//! lands per chunk or per file, and the runtime rows of
//! `docs/notes/transfer-extraction-baseline.md`.
//!
//! Run with:
//! `cargo bench -p cmdr-operations --bench transfer_benchmarks`.
//! `-- --test` runs each case once; `-- --save-baseline <name>` / `-- --baseline <name>`
//! compares two runs. Results land in `target/criterion/`.
//!
//! The engine is reached through `cmdr_operations::testing::bench`, thin wrappers
//! over the real internal entry points. What's measured and why:
//!
//! - **`local_copy_small_files`**: 1,000 × 4 KiB files in 10 folders, copied
//!   through `copy_files_with_progress_inner`. APFS clones small files, so this
//!   is engine overhead plus metadata syscalls: the own-write note, the ledger,
//!   the landing, progress, and the tree. Where a per-file seam regression shows.
//! - **`local_copy_small_files_journaled`**: the same with a temp-DB operation
//!   log installed, so journaling is on the measured path.
//! - **`stream_large_file`**: 64 MiB from an `InMemoryVolume` into a local volume
//!   through `copy_volumes_with_progress`, the chunked `write_from_stream` path
//!   with `sync_data`. The per-chunk loop: what catches a `dyn` call landing inside
//!   it. (A same-volume local large copy is an APFS clone and measures nothing.)
//! - **`delete_walk`**: `delete_files_with_progress_inner` over 5,000 files.
//! - **`scan_preview`**: the local scan-preview worker over 10,000 files.
//!   Read-only and idempotent, so it's the steadiest number here.
//!
//! Every case proves its fixture exercised the path before timing it (a copy
//! that landed nothing, or a walk that settled without a result, reports a
//! confident number about nothing). Fixtures live in temp dirs that drop with
//! the case; the per-iteration destinations are built and torn down untimed.
//!
//! Disk-bound cases on a shared Mac don't hold ±2%, so each sets its own
//! `noise_threshold` and the recorded spread is the contract, not a fixed gate.
//! These run under the `bench` profile on the system allocator, so they don't mix
//! with test-suite figures (`docs/notes/index-extraction-baseline.md`).

use cmdr_fs::app_path::AppPath;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_operations::testing::bench::{BenchEngine, BenchJournal};
use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use tempfile::TempDir;

/// Folders in the small-files source tree, and files in each.
const SMALL_FILE_DIRS: usize = 10;
const SMALL_FILES_PER_DIR: usize = 100;
const SMALL_FILE_COUNT: usize = SMALL_FILE_DIRS * SMALL_FILES_PER_DIR;
const SMALL_FILE_SIZE: usize = 4 * 1024;

const LARGE_FILE_SIZE: usize = 64 * 1024 * 1024;

/// The delete tree: 50 folders × 100 files.
const DELETE_DIRS: usize = 50;
const DELETE_FILES_PER_DIR: usize = 100;
const DELETE_FILE_COUNT: usize = DELETE_DIRS * DELETE_FILES_PER_DIR;

/// The scan tree: 100 folders × 100 files.
const SCAN_DIRS: usize = 100;
const SCAN_FILES_PER_DIR: usize = 100;
const SCAN_FILE_COUNT: usize = SCAN_DIRS * SCAN_FILES_PER_DIR;

fn temp_dir(prefix: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("create a temp dir for a bench fixture")
}

/// Writes `dirs` × `files_per_dir` files of `size` bytes under `root/tree`, and
/// returns `root/tree`: the single top-level source the operations take.
fn build_tree(root: &Path, dirs: usize, files_per_dir: usize, size: usize) -> PathBuf {
    let tree = root.join("tree");
    // Not all zeros, so nothing along the way can treat the content as a hole.
    let content: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    for d in 0..dirs {
        let dir = tree.join(format!("dir_{d:03}"));
        std::fs::create_dir_all(&dir).expect("create a fixture folder");
        for f in 0..files_per_dir {
            std::fs::write(dir.join(format!("file_{f:04}.bin")), &content).expect("write a fixture file");
        }
    }
    tree
}

/// Regular files under `root`, recursively.
fn count_files(root: &Path) -> usize {
    let mut count = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("list a fixture folder") {
            let entry = entry.expect("read a fixture entry");
            let file_type = entry.file_type().expect("stat a fixture entry");
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                count += 1;
            }
        }
    }
    count
}

/// The shape every disk-bound case shares: few long samples of equal length,
/// with a noise threshold wide enough for a shared machine's disk.
fn disk_bound_group<'a>(c: &'a mut Criterion, name: &str, noise_threshold: f64) -> BenchmarkGroup<'a, WallTime> {
    let mut group = c.benchmark_group(name);
    group
        .sampling_mode(SamplingMode::Flat)
        .sample_size(10)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(15))
        .noise_threshold(noise_threshold);
    group
}

/// A source tree of small files, built once per case.
struct SmallFiles {
    _dir: TempDir,
    source: PathBuf,
}

impl SmallFiles {
    fn build() -> Self {
        let dir = temp_dir("cmdr-transfer-bench-src-");
        let source = build_tree(dir.path(), SMALL_FILE_DIRS, SMALL_FILES_PER_DIR, SMALL_FILE_SIZE);
        Self { _dir: dir, source }
    }

    /// A fresh, empty destination folder. Returned from the timed routine so
    /// Criterion drops it, and the tree copied into it, after the measurement.
    fn destination() -> TempDir {
        temp_dir("cmdr-transfer-bench-dst-")
    }

    /// Proof a copy into `destination` landed every file.
    fn assert_landed(&self, destination: &Path) {
        let landed = count_files(&destination.join("tree"));
        assert_eq!(
            landed, SMALL_FILE_COUNT,
            "the copy landed {landed} of {SMALL_FILE_COUNT} files; the numbers below would time something else"
        );
    }
}

fn bench_local_copy_small_files(c: &mut Criterion) {
    let engine = BenchEngine::new();
    let fixture = SmallFiles::build();
    let sources = vec![fixture.source.clone()];

    let probe = SmallFiles::destination();
    engine.copy_local(&sources, probe.path()).expect("the probe copy");
    fixture.assert_landed(probe.path());
    drop(probe);

    let mut group = disk_bound_group(c, "local_copy_small_files", 0.05);
    group.throughput(Throughput::Elements(SMALL_FILE_COUNT as u64));
    group.bench_function(BenchmarkId::from_parameter(SMALL_FILE_COUNT), |b| {
        b.iter_batched(
            SmallFiles::destination,
            |destination| {
                engine
                    .copy_local(&sources, destination.path())
                    .expect("copy the small files");
                destination
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_local_copy_small_files_journaled(c: &mut Criterion) {
    let fixture = SmallFiles::build();
    let sources = vec![fixture.source.clone()];
    let journal_dir = temp_dir("cmdr-transfer-bench-journal-");
    // An engine of the case's own, journaling into a temp-DB log.
    let journal = BenchJournal::open(&journal_dir.path().join("operation-log.db")).expect("open the journal");

    let probe = SmallFiles::destination();
    let op_id = journal.copy_local(&sources, probe.path()).expect("the probe copy");
    fixture.assert_landed(probe.path());
    let journaled = journal.journaled_files(&op_id).expect("read the probe's journal rows");
    assert_eq!(
        journaled, SMALL_FILE_COUNT,
        "the probe copy journaled {journaled} of {SMALL_FILE_COUNT} files, so journaling isn't on the measured path"
    );
    drop(probe);

    let mut group = disk_bound_group(c, "local_copy_small_files_journaled", 0.05);
    group.throughput(Throughput::Elements(SMALL_FILE_COUNT as u64));
    group.bench_function(BenchmarkId::from_parameter(SMALL_FILE_COUNT), |b| {
        b.iter_batched(
            SmallFiles::destination,
            |destination| {
                journal
                    .copy_local(&sources, destination.path())
                    .expect("copy the small files, journaled");
                destination
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_stream_large_file(c: &mut Criterion) {
    let engine = BenchEngine::new();
    // Multi-threaded, like the app's: the local volume runs its syscalls on the
    // blocking pool.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build the bench runtime");
    let source_path = PathBuf::from("/large.bin");
    let content: Vec<u8> = (0..LARGE_FILE_SIZE).map(|i| (i % 251) as u8).collect();
    let source: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("Transfer bench source"));
    runtime
        .block_on(source.create_file(&AppPath::legacy_unchecked(&source_path), &content))
        .expect("seed the large source file");
    drop(content);
    let sources = vec![source_path];

    // The destination is a fresh folder with a volume rooted at it, both built
    // untimed.
    let destination = || {
        let dir = temp_dir("cmdr-transfer-bench-stream-");
        let volume = engine.local_volume(dir.path());
        (dir, volume)
    };
    let copy = |dir: &TempDir, volume: &Arc<dyn Volume>| {
        runtime
            .block_on(engine.copy_volumes(Arc::clone(&source), &sources, Arc::clone(volume), dir.path()))
            .expect("stream the large file");
    };

    let (probe_dir, probe_volume) = destination();
    copy(&probe_dir, &probe_volume);
    let landed = std::fs::metadata(probe_dir.path().join("large.bin"))
        .expect("the probe copy landed the file")
        .len();
    assert_eq!(
        landed, LARGE_FILE_SIZE as u64,
        "the probe copy landed {landed} of {LARGE_FILE_SIZE} bytes"
    );
    drop(probe_dir);

    let mut group = disk_bound_group(c, "stream_large_file", 0.05);
    group.throughput(Throughput::Bytes(LARGE_FILE_SIZE as u64));
    group.bench_function(BenchmarkId::from_parameter("64MiB"), |b| {
        b.iter_batched(
            destination,
            |(dir, volume)| {
                copy(&dir, &volume);
                (dir, volume)
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_delete_walk(c: &mut Criterion) {
    let engine = BenchEngine::new();
    // Each iteration deletes a tree of its own, built untimed. Empty files: the
    // walk's cost is per entry, and a byte in each only slows the setup.
    let tree = || {
        let dir = temp_dir("cmdr-transfer-bench-delete-");
        let source = build_tree(dir.path(), DELETE_DIRS, DELETE_FILES_PER_DIR, 0);
        (dir, source)
    };

    let (probe_dir, probe_source) = tree();
    assert_eq!(count_files(&probe_source), DELETE_FILE_COUNT, "the delete fixture");
    engine
        .delete_local(std::slice::from_ref(&probe_source))
        .expect("the probe delete");
    assert!(
        !probe_source.exists(),
        "the probe delete left {} behind",
        probe_source.display()
    );
    drop(probe_dir);

    let mut group = disk_bound_group(c, "delete_walk", 0.05);
    group.throughput(Throughput::Elements(DELETE_FILE_COUNT as u64));
    group.bench_function(BenchmarkId::from_parameter(DELETE_FILE_COUNT), |b| {
        b.iter_batched(
            tree,
            |(dir, source)| {
                engine
                    .delete_local(std::slice::from_ref(&source))
                    .expect("delete the tree");
                dir
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_scan_preview(c: &mut Criterion) {
    let engine = BenchEngine::new();
    let dir = temp_dir("cmdr-transfer-bench-scan-");
    let sources = vec![build_tree(dir.path(), SCAN_DIRS, SCAN_FILES_PER_DIR, 0)];

    let found = engine.scan_preview_local(sources.clone());
    assert_eq!(
        found,
        Some(SCAN_FILE_COUNT),
        "the probe preview cached {found:?} files of {SCAN_FILE_COUNT}"
    );

    let mut group = c.benchmark_group("scan_preview");
    group
        .sampling_mode(SamplingMode::Flat)
        .sample_size(20)
        .measurement_time(Duration::from_secs(10))
        .noise_threshold(0.03);
    group.throughput(Throughput::Elements(SCAN_FILE_COUNT as u64));
    group.bench_function(BenchmarkId::from_parameter(SCAN_FILE_COUNT), |b| {
        // The worker owns its sources, so each iteration gets a copy, untimed.
        b.iter_batched(
            || sources.clone(),
            |sources| engine.scan_preview_local(sources),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_local_copy_small_files,
    bench_local_copy_small_files_journaled,
    bench_stream_large_file,
    bench_delete_walk,
    bench_scan_preview
);
criterion_main!(benches);
