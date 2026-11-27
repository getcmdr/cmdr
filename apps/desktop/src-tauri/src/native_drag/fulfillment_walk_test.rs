//! How a folder fulfillment walks its source (`fulfillment.rs::populate_directory`):
//! any depth on the thread it really runs on, and never through a link back up
//! its own path (#1541).

use super::*;
use crate::file_system::volume::{InMemoryVolume, ListingProgress};
use cmdr_fs::app_path::AppPath;
use cmdr_fs::file_identity::FileId;
use cmdr_fs::volume::BackendKind;
use cmdr_fs::volume::EntryKind;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

struct FixedResolver(Arc<dyn Volume>);

impl VolumeResolver for FixedResolver {
    fn resolve(&self, _volume_id: &str, _source_path: &AppPath) -> Option<Arc<dyn Volume>> {
        Some(Arc::clone(&self.0))
    }
}

/// The promise queue's thread: a GCD thread, 512 KiB of stack, which polls the
/// fulfillment through `block_on` (`promises.rs`).
const PROMISE_QUEUE_STACK: usize = 512 * 1024;

/// Runs `fulfill_with_resolver` the way the delegate does: `block_on` on a
/// thread of [`PROMISE_QUEUE_STACK`], the runtime's workers elsewhere.
fn fulfill_on_the_promise_queue(
    volume: Arc<dyn Volume>,
    source: &'static str,
    dest: PathBuf,
) -> Result<FulfillOutcome, FulfillError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = runtime.handle().clone();
    std::thread::Builder::new()
        .stack_size(PROMISE_QUEUE_STACK)
        .spawn(move || {
            handle.block_on(fulfill_with_resolver(
                &FixedResolver(volume),
                "phone",
                Path::new(source),
                &dest,
            ))
        })
        .unwrap()
        .join()
        .expect("the fulfillment ends without overflowing its stack")
}

/// 400 levels: past what a future per level held on this stack, inside macOS's
/// 1,024-byte `PATH_MAX` with the temp folder in front (two bytes a level).
#[cfg(unix)]
#[test]
fn a_folder_four_hundred_levels_deep_lands_whole_on_the_promise_queues_stack() {
    const DEPTH: usize = 400;
    let source_dir = tempfile::tempdir().unwrap();
    let spine = "/d".repeat(DEPTH);
    let bottom = source_dir.path().join(format!("deep{spine}"));
    std::fs::create_dir_all(&bottom).unwrap();
    std::fs::write(bottom.join("bottom.txt"), b"bottom").unwrap();
    let volume: Arc<dyn Volume> = Arc::new(cmdr_local::LocalVolume::local_folder(
        "Phone",
        source_dir.path().to_path_buf(),
        crate::volume_host::host(),
    ));
    let dest_dir = tempfile::tempdir().unwrap();
    let dest = dest_dir.path().join("deep copy");

    let outcome = fulfill_on_the_promise_queue(volume, "/deep", dest.clone());

    assert!(outcome.expect("the deep folder lands").is_dir);
    let landed = dest.join(spine.trim_start_matches('/')).join("bottom.txt");
    assert_eq!(std::fs::read(landed).unwrap(), b"bottom");
}

/// A Samba share lists `loop -> ..` as a plain folder holding what its parent
/// holds. Its node is its parent's, and a stat says it's a link.
struct HiddenLoopVolume {
    inner: InMemoryVolume,
}

const SHARED_FOLDER: &str = "/A";
const HIDDEN_LOOP: &str = "/A/loop";

impl Volume for HiddenLoopVolume {
    fn name(&self) -> &str {
        "Share"
    }
    fn root(&self) -> &AppPath {
        AppPath::from_normalized("/").expect("a canonical literal")
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Smb
    }
    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        self.inner.list_directory(path, on_progress)
    }
    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        self.inner.get_metadata(path)
    }
    fn entry_kind<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<EntryKind, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            if path.as_str() == HIDDEN_LOOP {
                return Ok(EntryKind::Symlink);
            }
            self.inner.entry_kind(path).await
        })
    }
    fn reached_node<'a>(&'a self, path: &'a AppPath) -> Pin<Box<dyn Future<Output = Option<FileId>> + Send + 'a>> {
        Box::pin(async move {
            match path.as_str() {
                SHARED_FOLDER | HIDDEN_LOOP => Some(FileId::from_stored(1, 7)),
                "/" => Some(FileId::from_stored(1, 2)),
                _ => None,
            }
        })
    }
    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        self.inner.open_read_stream(path)
    }
}

/// The loop is left out, with nothing created for it, and everything else lands.
#[test]
fn a_hidden_link_back_up_the_dragged_folder_is_left_out() {
    let inner = InMemoryVolume::new("Share");
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        for dir in [SHARED_FOLDER, HIDDEN_LOOP] {
            inner
                .create_directory(AppPath::from_normalized(dir).unwrap())
                .await
                .unwrap();
        }
        for file in ["/A/kept.txt", "/A/loop/kept.txt"] {
            inner
                .create_file(AppPath::from_normalized(file).unwrap(), b"kept")
                .await
                .unwrap();
        }
    });
    let dest_dir = tempfile::tempdir().unwrap();
    let dest = dest_dir.path().join("A");

    let outcome = fulfill_on_the_promise_queue(Arc::new(HiddenLoopVolume { inner }), SHARED_FOLDER, dest.clone());

    assert!(outcome.expect("the folder lands without its loop").is_dir);
    assert_eq!(std::fs::read(dest.join("kept.txt")).unwrap(), b"kept");
    assert!(!dest.join("loop").exists(), "nothing lands for the link back up");
}
