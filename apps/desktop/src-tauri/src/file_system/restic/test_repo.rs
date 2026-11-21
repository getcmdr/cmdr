//! A real restic repo on a real local drive, registered in the app's volume
//! registry: the fixture the end-to-end cells share (copy-out, the viewer,
//! `inspect_file`). They reach the repo the way a pane does, through
//! `VolumeManager::resolve`, so the route, the parked hub, and the view are the
//! app's own.

use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cmdr_fs::restic_view::VIRTUAL_ENTRY_NAME;
use cmdr_fs::volume::{UnlockOutcome, Volume, VolumeError, VolumeReadStream};
use cmdr_local::LocalPosixVolume;
use cmdr_operations::forward_volume_methods;
use cmdr_restic::test_fixtures::FixtureRepo;

use crate::file_system::volume::manager::get_volume_manager;

/// The fixture repo's password.
pub(crate) const PASSWORD: &str = "restic-e2e";

/// One file of the folder a fixture backs up.
pub(crate) struct SourceFile {
    /// Relative to the backed-up folder.
    pub(crate) path: &'static str,
    pub(crate) bytes: &'static [u8],
    pub(crate) mode: u32,
    /// Unix seconds.
    pub(crate) modified: u64,
}

/// An entry of the backed-up folder with no bytes of its own.
pub(crate) enum NoBytes {
    /// A fifo at this relative path.
    Fifo(&'static str),
    /// A symlink at the first relative path, pointing at the second.
    Symlink(&'static str, &'static str),
}

/// A drive holding a backed-up folder's repo, registered under its own id.
pub(crate) struct BackedUpDrive {
    pub(crate) volume_id: String,
    /// The repo's own folder on the drive.
    pub(crate) repo: PathBuf,
    /// Where the backed-up folder's contents sit in the view: `latest`, then
    /// the folder's absolute path (restic records a backup from `/`).
    pub(crate) backed_up: PathBuf,
    /// The drive's read gate, open until a test closes it.
    pub(crate) gate: Arc<ReadGate>,
    _dir: tempfile::TempDir,
}

/// Backs `files` and `no_bytes` up into a fresh repo on a fresh local drive,
/// and registers the drive as `<tag>-<unique>`. Its reads go through a
/// [`ReadGate`].
pub(crate) async fn backed_up_drive(tag: &str, files: &[SourceFile], no_bytes: &[NoBytes]) -> BackedUpDrive {
    let dir = tempfile::tempdir().expect("tempdir");
    // The canonical spelling: on macOS the temp dir sits behind the `/var` →
    // `/private/var` symlink, and restic records the path it was handed.
    let root = std::fs::canonicalize(dir.path()).expect("canonical tempdir");
    let source = root.join("source");
    for file in files {
        let path = source.join(file.path);
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("source folder");
        std::fs::write(&path, file.bytes).expect("source file");
        set_mode(&path, file.mode);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|handle| handle.set_modified(UNIX_EPOCH + Duration::from_secs(file.modified)))
            .expect("source mtime");
    }
    for entry in no_bytes {
        match entry {
            NoBytes::Fifo(path) => make_fifo(&source.join(path)),
            NoBytes::Symlink(path, target) => {
                let path = source.join(path);
                std::fs::create_dir_all(path.parent().expect("a link has a parent")).expect("link folder");
                std::os::unix::fs::symlink(target, path).expect("symlink");
            }
        }
    }

    let fixture = FixtureRepo::init(PASSWORD);
    fixture.backup(&source, SystemTime::now());
    let local = Arc::new(LocalPosixVolume::local_folder(
        "Drive",
        &root,
        crate::volume_host::host(),
    ));
    let repo = root.join("repo");
    fixture.write_into(local.as_ref(), &repo).await;

    let gate = Arc::new(ReadGate::default());
    let volume_id = cmdr_fs::testing::unique_test_id(tag);
    get_volume_manager().register(
        &volume_id,
        Arc::new(GatedDrive {
            inner: local,
            gate: Arc::clone(&gate),
        }),
    );
    let backed_up = repo
        .join(VIRTUAL_ENTRY_NAME)
        .join("latest")
        .join(source.strip_prefix("/").expect("an absolute source"));
    BackedUpDrive {
        volume_id,
        repo,
        backed_up,
        gate,
        _dir: dir,
    }
}

impl BackedUpDrive {
    /// The repo's `Browse snapshots` row, its scratch dir crossed once.
    pub(crate) fn view_path(&self) -> cmdr_fs::app_path::AppPathBuf {
        cmdr_fs::app_path::AppPathBuf::from_native_unicode(&self.repo.join(VIRTUAL_ENTRY_NAME))
            .expect("a scratch dir has a Unicode name")
    }

    /// The view the route serves for this repo.
    pub(crate) async fn view(&self) -> Arc<dyn Volume> {
        get_volume_manager()
            .resolve(&self.volume_id, &self.view_path())
            .await
            .volume
            .expect("the route serves the repo's view")
    }

    /// Unlocks the repo through the view's password seam, the way the dialog
    /// does (without remembering it).
    pub(crate) async fn unlock(&self) {
        let view = self.view().await;
        let outcome = view
            .as_password_locked()
            .expect("a repo's view is password-locked")
            .unlock(PASSWORD.to_string())
            .await;
        assert!(matches!(outcome, Ok(UnlockOutcome::Unlocked)), "{outcome:?}");
    }
}

/// Holds the drive's reads of one repo folder once closed on it (`index` for
/// the index load, `data` for tree and file reads), so a test can stand a
/// reader inside a read for as long as it likes. Everything else passes.
pub(crate) struct ReadGate {
    closed_on: tokio::sync::watch::Sender<Option<&'static str>>,
    held: AtomicUsize,
}

impl Default for ReadGate {
    fn default() -> Self {
        Self {
            closed_on: tokio::sync::watch::Sender::new(None),
            held: AtomicUsize::new(0),
        }
    }
}

impl ReadGate {
    /// Holds every read under a repo folder named `folder` from now on.
    pub(crate) fn close_on(&self, folder: &'static str) {
        self.closed_on.send_replace(Some(folder));
    }

    pub(crate) fn open(&self) {
        self.closed_on.send_replace(None);
    }

    /// How many reads are waiting at the gate.
    pub(crate) fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
    }

    async fn pass(&self, path: &AppPath) {
        let mut closed_on = self.closed_on.subscribe();
        let gated =
            |closed_on: &Option<&'static str>| closed_on.is_some_and(|folder| path.names().any(|name| name == folder));
        if !gated(&closed_on.borrow_and_update()) {
            return;
        }
        self.held.fetch_add(1, Ordering::SeqCst);
        // An `Err` means the sender is gone, and the gate lives as long as the
        // drive holding it.
        let _ = closed_on.wait_for(|closed_on| !gated(closed_on)).await;
        self.held.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The local drive with its reads through a [`ReadGate`]. It offers no held
/// readers, so every pack window is a `read_range` the gate sees.
struct GatedDrive {
    inner: Arc<LocalPosixVolume>,
    gate: Arc<ReadGate>,
}

impl Volume for GatedDrive {
    forward_volume_methods!(
        inner => name,
        root,
        lane_key,
        list_directory,
        get_metadata,
        exists,
        is_directory,
        get_space_info,
        supports_streaming,
    );

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn open_read_stream<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            self.gate.pass(path).await;
            self.inner.open_read_stream(path).await
        })
    }

    fn read_range<'a>(
        &'a self,
        path: &'a AppPath,
        offset: u64,
        len: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            self.gate.pass(path).await;
            self.inner.read_range(path, offset, len).await
        })
    }
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("source mode");
}

fn make_fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    std::fs::create_dir_all(path.parent().expect("a fifo has a parent")).expect("fifo folder");
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("a path without NUL");
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call;
    // `mkfifo` only reads it and writes nothing back through the pointer.
    let made = unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) };
    assert_eq!(made, 0, "mkfifo {}", path.display());
}
