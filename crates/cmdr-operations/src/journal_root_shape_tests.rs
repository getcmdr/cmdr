//! What a cross-volume copy and move journal for each shape of root the drivers meet:
//! every file row's SOURCE and DESTINATION, asserted both, for a volume rooted at `/`, a
//! drive, a scheme root, a volume-relative test root, and a source that IS its volume's
//! root. Undo reads exactly these two ends, so a row whose destination reads as its
//! source (an earlier pass's first CI run) makes undo skip every file.

use std::sync::Arc;
use std::time::Duration;

use super::event_sinks::CollectorEventSink;
use super::journal;
use super::types::VolumeCopyConfig;
use super::{copy_volumes_with_progress, move_volumes_with_progress};
use crate::testing::{TestOperationLog, TestOperations};

use cmdr_fs::app_path::{AppPathBuf, PathArithmetic, PathFlavor};
use cmdr_fs::volume::{InMemoryVolume, Volume};
use cmdr_operation_log::store::open_read_connection;
use cmdr_operation_log::types::{EntryType, Initiator, OpKind};

#[derive(Clone, Copy, Debug)]
enum Transfer {
    Copy,
    Move,
}

/// One shape: the files the source volume holds, the sources selected, the destination
/// folder, and every (source, destination) file row the journal must hold.
struct Shape {
    name: &'static str,
    files: &'static [&'static str],
    sources: &'static [&'static str],
    dest: &'static str,
    rows: &'static [(&'static str, &'static str)],
}

const SHAPES: &[Shape] = &[
    Shape {
        name: "a volume rooted at /",
        files: &["/src/f.txt", "/src/d/g.txt"],
        sources: &["/src/f.txt", "/src/d"],
        dest: "/dst",
        rows: &[("/src/d/g.txt", "/dst/d/g.txt"), ("/src/f.txt", "/dst/f.txt")],
    },
    Shape {
        name: "a drive",
        files: &["C:/src/f.txt", "C:/src/d/g.txt"],
        sources: &["C:/src/f.txt", "C:/src/d"],
        dest: "D:/dst",
        rows: &[("C:/src/d/g.txt", "D:/dst/d/g.txt"), ("C:/src/f.txt", "D:/dst/f.txt")],
    },
    Shape {
        name: "a scheme root",
        files: &["sftp://host/src/f.txt", "sftp://host/src/d/g.txt"],
        sources: &["sftp://host/src/f.txt", "sftp://host/src/d"],
        dest: "sftp://other/dst",
        rows: &[
            ("sftp://host/src/d/g.txt", "sftp://other/dst/d/g.txt"),
            ("sftp://host/src/f.txt", "sftp://other/dst/f.txt"),
        ],
    },
    Shape {
        name: "a volume-relative test root",
        files: &["/src/f.txt", "/src/d/g.txt"],
        sources: &["src/f.txt", "src/d"],
        dest: "dst",
        rows: &[("src/d/g.txt", "dst/d/g.txt"), ("src/f.txt", "dst/f.txt")],
    },
    Shape {
        // #1457: one name on every host, both ends.
        name: "a top-level name holding a backslash",
        files: &["/src/a\\b.txt"],
        sources: &["/src/a\\b.txt"],
        dest: "/dst",
        rows: &[("/src/a\\b.txt", "/dst/a\\b.txt")],
    },
    Shape {
        name: "a source that is its volume's root",
        files: &["/f.txt", "/d/g.txt"],
        sources: &["/"],
        dest: "/dst",
        rows: &[("/d/g.txt", "/dst/d/g.txt"), ("/f.txt", "/dst/f.txt")],
    },
];

/// A shape's path as a pane would hand it over: the app's own text, a drive path on
/// every host (as a Windows pane names it), and a suite's volume-relative text as the
/// bridge value the typed starts pass it on as.
fn typed(text: &str) -> AppPathBuf {
    AppPathBuf::from_wire(text)
        .or_else(|_| AppPathBuf::parse(text, PathFlavor::Windows))
        .unwrap_or_else(|_| crate::testing::app_path_of(std::path::Path::new(text)))
}

async fn journaled_rows(shape: &Shape, transfer: Transfer) -> Vec<(String, String)> {
    let log = TestOperationLog::new("journal-root-shape");
    let engine = TestOperations::journaling_into(&log);
    let source = Arc::new(InMemoryVolume::new("Src").with_space_info(1_000_000, 900_000));
    let dest = Arc::new(InMemoryVolume::new("Dst").with_space_info(1_000_000, 900_000));
    for file in shape.files {
        let path = typed(file);
        // Its folders too, so a walk of the root lists them.
        if let Some(parent) = path.parent() {
            source.create_directory_all(parent).await.unwrap();
        }
        source.create_file(&path, file.as_bytes()).await.unwrap();
    }
    let state = Arc::new(
        engine
            .new_operation_state(Duration::ZERO)
            .with_journal_volumes("src".into(), "dst".into()),
    );
    let op_id = format!("{:?}-{}", transfer, shape.name);
    let kind = match transfer {
        Transfer::Copy => OpKind::Copy,
        Transfer::Move => OpKind::Move,
    };
    journal::open_volume_op(
        state.scope(),
        &op_id,
        kind,
        Initiator::User,
        "src",
        Some("dst"),
        shape.sources.len() as u64,
    );
    let sources: Vec<AppPathBuf> = shape.sources.iter().map(|source| typed(source)).collect();
    let dest_dir = typed(shape.dest);
    let config = VolumeCopyConfig {
        progress_interval_ms: 0,
        ..VolumeCopyConfig::default()
    };
    let (src, dst) = (
        Arc::clone(&source) as Arc<dyn Volume>,
        Arc::clone(&dest) as Arc<dyn Volume>,
    );
    let events = Arc::new(CollectorEventSink::new());
    let result = match transfer {
        Transfer::Copy => {
            copy_volumes_with_progress(events, &op_id, &state, src, &sources, dst, &dest_dir, &config).await
        }
        Transfer::Move => {
            move_volumes_with_progress(events, &op_id, &state, src, &sources, dst, &dest_dir, &config).await
        }
    };
    result.unwrap_or_else(|f| panic!("{transfer:?} {}: {:?}", shape.name, f.error));
    journal::finalize_op(state.scope(), kind, journal::execution_status_from_error(None));

    let conn = open_read_connection(log.db()).expect("read conn");
    let detail = cmdr_operation_log::query::get_operation(&conn, &op_id, 1_000, 0)
        .expect("read detail")
        .expect("the transfer is journaled");
    let mut rows: Vec<(String, String)> = detail
        .items
        .iter()
        .filter(|item| item.entry_type == EntryType::File)
        .map(|item| (item.source_path.clone(), item.dest_path.clone().unwrap_or_default()))
        .collect();
    rows.sort();
    rows
}

/// How the store reads back a row's text: it interns folders by `/`, so text with no root
/// the host reads (a test's volume-relative `src/f.txt`, and off Windows a drive's
/// `C:/src`) comes back under `/`. Pinned as it is: the store's reading isn't the drivers'.
fn as_stored(text: &str) -> String {
    let rooted = text.starts_with('/') || text.contains("://") || (cfg!(windows) && text.get(1..3) == Some(":/"));
    if rooted { text.to_string() } else { format!("/{text}") }
}

/// ❗ Each file row names where it came from AND where it landed, for every root shape,
/// on both drivers that journal per leaf.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_root_shape_journals_each_files_source_and_destination() {
    let mut wrong = Vec::new();
    for transfer in [Transfer::Copy, Transfer::Move] {
        for shape in SHAPES {
            let rows = journaled_rows(shape, transfer).await;
            let expected: Vec<(String, String)> = shape
                .rows
                .iter()
                .map(|&(source, dest)| (as_stored(source), as_stored(dest)))
                .collect();
            if rows != expected {
                wrong.push(format!("{transfer:?} {}: {rows:?}, expected {expected:?}", shape.name));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
