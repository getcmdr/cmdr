//! Moving bytes in and out of a local file, for both local volumes: the chunked read stream a
//! volume hands out, the ranged read beside it, the resumed read, and the durable streaming
//! write every cross-volume copy landing on local disk goes through, with the metadata the
//! source carried (`cmdr_fs::metadata`, the one place the ordering lives).
//!
//! One body for `LocalPosixVolume` and `LocalWindowsVolume`: each sibling crosses its app path
//! to the OS path once (`os_path`, `locate`) and hands that in with its `DeviceBarriers`. What
//! differs per OS is small and sits behind `streams/platform/`: the version a resume compares,
//! the positional read, the owner-only create mode, and how an error names its path (the app
//! spelling on Windows, never the `\\?\` form).

use cmdr_fs::durable::DeviceBarriers;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::metadata::{
    CarriedMetadata, MetadataClasses, StagedItem, StagedTimes, apply_staged, default_file_mode, routed_mode,
};
use cmdr_fs::volume::{SourceVersion, StreamLength, StreamWriteProgress, StreamWritten, WriteMode};
use cmdr_fs::volume::{VolumeError, VolumeReadStream};
use std::future::Future;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::task::spawn_blocking;

mod platform;

use platform::{LocalVersion, error_at, read_at, shown};

/// A boxed future a stream body hands back, as the `Volume` trait's methods return it.
type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, VolumeError>> + Send + 'a>>;

/// What a streamed write reports its progress to, and asks whether to stop.
pub(crate) type OnProgress<'a> = &'a (dyn Fn(StreamWriteProgress) -> ControlFlow<()> + Sync);

/// Streaming reader for a local file.
///
/// Reads the file in 1 MiB chunks on the blocking thread pool via
/// `tokio::task::spawn_blocking`. Each `next_chunk` call hands the file handle
/// to the blocking pool, reads one chunk, and returns ownership along with the
/// data.
struct LocalReadStream {
    file: Option<std::fs::File>,
    total_size: u64,
    bytes_read: u64,
    /// The `stat` the open already took: the date, the creation date, the
    /// mode, and the flags, for nothing.
    stat: std::fs::Metadata,
    /// Which version of the file this read saw, taken at the open.
    version: LocalVersion,
}

/// How a streamed write opens its file.
#[derive(Clone, Copy)]
pub(crate) enum Opening {
    /// A new write, under its mode.
    Fresh(WriteMode),
    /// Carrying on the file a failed write left, from this byte.
    Continue(u64),
}

/// Why a streamed write's open didn't hand back a file.
enum OpenRefusal {
    /// The file to carry on is gone or shorter than the byte to carry on from.
    CantContinue,
    Io(std::io::Error),
}

impl From<std::io::Error> for OpenRefusal {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// 1 MiB chunks, matching `chunked_copy.rs`'s constant.
const LOCAL_STREAM_CHUNK_SIZE: usize = 1024 * 1024;

impl VolumeReadStream for LocalReadStream {
    fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
        Box::pin(async move {
            let mut file = self.file.take()?;

            let (file_ret, result) = spawn_blocking(move || {
                use std::io::Read;
                let mut buf = vec![0u8; LOCAL_STREAM_CHUNK_SIZE];
                let n = match file.read(&mut buf) {
                    Ok(n) => n,
                    // No path: the handle is already open, so `ENOENT` and `EACCES`
                    // can't reach here; a mid-stream read failure is an `IoError`.
                    Err(e) => return (file, Err(VolumeError::from_io_without_path(&e))),
                };
                buf.truncate(n);
                (file, Ok(buf))
            })
            .await
            .expect("spawn_blocking read-chunk closure doesn't panic and the task is uncancelable");

            match result {
                Ok(buf) if buf.is_empty() => {
                    // EOF: drop the file handle.
                    drop(file_ret);
                    None
                }
                Ok(buf) => {
                    self.bytes_read += buf.len() as u64;
                    self.file = Some(file_ret);
                    Some(Ok(buf))
                }
                Err(e) => {
                    drop(file_ret);
                    Some(Err(e))
                }
            }
        })
    }

    fn total_size(&self) -> StreamLength {
        StreamLength::Known(self.total_size)
    }

    fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    fn modified_at(&self) -> Option<std::time::SystemTime> {
        self.stat.modified().ok()
    }

    /// Off the open's `stat`, plus the xattrs off the fd when the destination
    /// keeps them (one `flistxattr` for a file with none).
    fn metadata(&self, wanted: MetadataClasses) -> CarriedMetadata {
        match &self.file {
            Some(file) => CarriedMetadata::of_local_file_wanting(file, &self.stat, wanted),
            // Past EOF the fd is gone; the stat still answers.
            None => CarriedMetadata::of_local_stat(&self.stat).restricted_to(wanted.without(MetadataClasses::XATTRS)),
        }
    }

    /// The same classes, the xattrs by name only.
    fn metadata_outline(&self, wanted: MetadataClasses) -> CarriedMetadata {
        match &self.file {
            Some(file) => CarriedMetadata::outline_of_local_file(file, &self.stat, wanted),
            None => self.metadata(wanted),
        }
    }

    fn source_version(&self) -> Option<SourceVersion> {
        Some(SourceVersion::new(self.version))
    }
}

/// Opens `os` for chunked reading, refusing a directory.
pub(crate) fn open_read_stream<'a>(os: PathBuf) -> Boxed<'a, Box<dyn VolumeReadStream>> {
    Box::pin(async move {
        spawn_blocking(move || {
            let metadata = std::fs::metadata(&os).map_err(|e| error_at(&e, &os))?;
            if metadata.is_dir() {
                return Err(VolumeError::IoError {
                    message: "Cannot stream a directory".into(),
                    raw_os_error: None,
                });
            }
            let total_size = metadata.len();
            let file = std::fs::File::open(&os).map_err(|e| error_at(&e, &os))?;
            let version = LocalVersion::of(&file, &metadata, &os);
            Ok(Box::new(LocalReadStream {
                file: Some(file),
                total_size,
                bytes_read: 0,
                stat: metadata,
                version,
            }) as Box<dyn VolumeReadStream>)
        })
        .await
        .expect("spawn_blocking open_read_stream closure doesn't panic and the task is uncancelable")
    })
}

/// [`Volume::resume_read_stream`](cmdr_fs::volume::Volume::resume_read_stream): the file
/// opened again and read from `offset`, once its own version still names the node, the
/// length, and the times `version` saw; any other file is `SourceChanged` before a byte is
/// read.
pub(crate) fn resume_read_stream<'a>(
    os: PathBuf,
    version: &SourceVersion,
    offset: u64,
) -> Boxed<'a, Box<dyn VolumeReadStream>> {
    let expected = version.downcast_ref::<LocalVersion>().copied();
    Box::pin(async move {
        let Some(expected) = expected else {
            return Err(VolumeError::NotSupported);
        };
        spawn_blocking(move || {
            use std::io::Seek;
            let mut file = std::fs::File::open(&os).map_err(|e| error_at(&e, &os))?;
            let stat = file.metadata().map_err(|e| error_at(&e, &os))?;
            let version = LocalVersion::of(&file, &stat, &os);
            if version != expected {
                return Err(VolumeError::SourceChanged(shown(&os)));
            }
            file.seek(std::io::SeekFrom::Start(offset))
                .map_err(|e| error_at(&e, &os))?;
            Ok(Box::new(LocalReadStream {
                file: Some(file),
                total_size: stat.len(),
                bytes_read: 0,
                stat,
                version,
            }) as Box<dyn VolumeReadStream>)
        })
        .await
        .expect("spawn_blocking resume_read_stream closure doesn't panic and the task is uncancelable")
    })
}

/// One positional read window (`pread`, `ReadFile` at an offset), looped until it's full or
/// the file ends.
pub(crate) fn read_range<'a>(os: PathBuf, offset: u64, len: usize) -> Boxed<'a, Vec<u8>> {
    Box::pin(async move {
        spawn_blocking(move || {
            let file = std::fs::File::open(&os).map_err(|e| error_at(&e, &os))?;
            let mut buf = vec![0u8; len];
            let mut filled = 0usize;
            // A positional read may come back short, so loop until the window is full or
            // the file ends (a read at or past the end returns 0).
            while filled < len {
                let n = read_at(&file, &mut buf[filled..], offset + filled as u64).map_err(|e| error_at(&e, &os))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            buf.truncate(filled);
            Ok(buf)
        })
        .await
        .expect("spawn_blocking read_range closure doesn't panic and the task is uncancelable")
    })
}

/// [`Volume::flush_writes`](cmdr_fs::volume::Volume::flush_writes): the
/// closing pass of a volume's barriers, on the blocking pool. Each noted
/// directory gets its sync, then every device a write reached since the
/// last pass gets one barrier. A file sync that failed since the last
/// flush, or a barrier the drive refuses, is the answer, as an I/O error
/// naming the item.
///
/// ❗ The barriers are the VOLUME's, shared by every operation writing to
/// it, so a file sync that failed answers whichever flush comes next. With
/// two operations writing to one local volume at once, that can be the
/// other one's: it reports a failure that wasn't its own, and the one whose
/// file failed then flushes clean. Accepted: it takes a failing `fsync`
/// during two overlapping writes to one volume, and a failed sync here used
/// to reach no flush at all.
pub(crate) fn flush_writes<'a>(barriers: Arc<DeviceBarriers>) -> Boxed<'a, ()> {
    Box::pin(async move {
        spawn_blocking(move || barriers.flush())
            .await
            .expect("spawn_blocking flush closure doesn't panic and the task is uncancelable")
            .map_err(|failure| error_at(&failure.error, &failure.path))
    })
}

/// Streams `stream` into `dest`, then hands the file to the drive
/// (`durable::sync_to_drive`), puts the source's metadata on it, and notes
/// the file and its parent for the next barrier pass before reporting
/// success. Durable once [`flush_writes`], or an interval pass, has run since.
///
/// `Opening::Fresh` is `Volume::write_from_stream`; `Opening::Continue` is
/// `Volume::continue_write_from_stream`: the same write, into the file a failed one left,
/// cut to its byte and carried on from there, refused (`NotSupported`) for a target that's
/// gone or shorter, creating nothing.
///
/// The source's user flags aren't put on: Locked would refuse the landing
/// rename, so the caller applies them after it (`Volume::apply_metadata`).
pub(crate) fn stream_into<'a>(
    dest_abs: PathBuf,
    barriers: Arc<DeviceBarriers>,
    opening: Opening,
    length: StreamLength,
    mut stream: Box<dyn VolumeReadStream>,
    on_progress: OnProgress<'a>,
) -> Boxed<'a, StreamWritten> {
    Box::pin(async move {
        // Ensure parent directory exists
        if let Some(parent) = dest_abs.parent() {
            let parent = parent.to_path_buf();
            let parent_for_error = parent.clone();
            spawn_blocking(move || std::fs::create_dir_all(&parent))
                .await
                .expect("spawn_blocking create_dir_all closure doesn't panic and the task is uncancelable")
                .map_err(|e| error_at(&e, &parent_for_error))?;
        }

        // Asked once, before the first chunk: everything a local file keeps
        // but the flags (above). A routed source's mode is folded through
        // the umask, never put on as is.
        let carried = stream.metadata(MetadataClasses::ALL.without(MetadataClasses::FLAGS));
        let landed_mode = carried.mode.and_then(|mode| routed_mode(mode, default_file_mode()));
        let carried = CarriedMetadata {
            mode: landed_mode,
            ..carried
        };

        // Open destination file on the blocking pool. `CreateNew` is
        // `O_EXCL` (`CREATE_NEW` on Windows): a file already at the name is
        // refused atomically and left alone, before anything of ours exists to
        // clean up. On POSIX a file whose source has a mode is created
        // owner-only and gets that mode after its last byte, so a private file
        // is never readable by others while it streams; one without keeps the
        // umask's default.
        let dest_for_open = dest_abs.clone();
        let mut file = spawn_blocking(move || -> Result<std::fs::File, OpenRefusal> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true);
            match opening {
                Opening::Fresh(mode) => {
                    if landed_mode.is_some() {
                        platform::owner_only(&mut options);
                    }
                    match mode {
                        WriteMode::CreateNew | WriteMode::CreateNewInFreshFolder => options.create_new(true),
                        WriteMode::CreateOrReplace => options.create(true).truncate(true),
                    };
                    Ok(options.open(&dest_for_open)?)
                }
                // The file a failed write left, never a new one: a gone or
                // short one can't be carried on, and a byte past `at` is
                // one nobody reported, so it's cut.
                Opening::Continue(at) => {
                    use std::io::Seek;
                    let mut file = options.open(&dest_for_open).map_err(|_| OpenRefusal::CantContinue)?;
                    if file.metadata()?.len() < at {
                        return Err(OpenRefusal::CantContinue);
                    }
                    file.set_len(at)?;
                    file.seek(std::io::SeekFrom::Start(at))?;
                    Ok(file)
                }
            }
        })
        .await
        .expect("spawn_blocking open closure doesn't panic and the task is uncancelable")
        .map_err(|refusal| match refusal {
            OpenRefusal::CantContinue => VolumeError::NotSupported,
            OpenRefusal::Io(e) => error_at(&e, &dest_abs),
        })?;

        let mut bytes_written = match opening {
            Opening::Fresh(_) => 0u64,
            Opening::Continue(at) => at,
        };
        while let Some(chunk_result) = stream.next_chunk().await {
            let chunk = chunk_result?;
            if chunk.is_empty() {
                continue;
            }
            let chunk_len = chunk.len() as u64;

            // Write the chunk on the blocking pool.
            let (file_ret, write_res) = spawn_blocking(move || {
                use std::io::Write;
                let res = file.write_all(&chunk);
                (file, res)
            })
            .await
            .expect("spawn_blocking write_all closure doesn't panic and the task is uncancelable");
            file = file_ret;
            write_res.map_err(|e| error_at(&e, &dest_abs))?;

            bytes_written += chunk_len;

            if on_progress(StreamWriteProgress::in_order(bytes_written, length)) == ControlFlow::Break(()) {
                // Close the handle first (Windows won't delete a file that's still
                // open), then try to clean up the partial file.
                drop(file);
                let partial = dest_abs.clone();
                let _ = spawn_blocking(move || std::fs::remove_file(&partial)).await;
                return Err(VolumeError::Cancelled("Operation cancelled by user".into()));
            }
        }

        // Hand the file to the drive before signalling success. A bare
        // `file.flush()` is a userspace no-op on a raw `std::fs::File`, so
        // without the sync the bytes would live only in the OS page cache.
        // A cross-volume copy/move landing on a local disk (MTP → Local,
        // SMB → Local, USB import) all flows through this function. The sync
        // is a plain `fsync` (`FlushFileBuffers` on Windows), and the device
        // barrier that empties a Mac drive's cache comes from `barriers`: on an
        // interval, and in `flush_writes`, which the caller runs before
        // reporting complete or deleting a moved source (`../DETAILS.md`, the
        // durability decision). One barrier per file was the whole cost of a
        // many-small-file copy (#1145).
        //
        // ❗ A sync per file HERE, unlike the local engine's, which waits for
        // the pass: this engine's copies run concurrently and overlap their
        // syncs, and a pass that took them over measured slower
        // (`../../cmdr-operations/src/transfer/DETAILS.md` § Durability).
        //
        // A sync that fails doesn't fail the write (the bytes are written
        // either way). It's left in the barriers, so the closing
        // `flush_writes` answers it and a move keeps its source. Where no
        // barrier follows the sync (Linux, Windows, a network mount), that's
        // the only place it can surface.
        //
        // Then the source's metadata, on the open handle, after the last
        // byte AND the flush: xattrs, the mode, then birth and mtime in one
        // call (`apply_staged`). ❗ Flush first: on a macOS SMB mount, writes
        // still buffered when the dates go on reach the server at the
        // handle's real close, which stamps its own mtime. Best effort: the
        // bytes are the copy.
        let times = StagedTimes {
            modified: stream.modified_at(),
            accessed: None,
        };
        let dest_for_sync = dest_abs.clone();
        let carried_for_apply = carried.clone();
        let (file, report) = spawn_blocking(move || {
            use std::io::Write;
            // Userspace flush first (harmless no-op on a raw File, but
            // correct if the writer is ever wrapped in a BufWriter).
            let _ = file.flush();
            if let Err(e) = barriers.hand_over_file(&file, &dest_for_sync) {
                barriers.note_unconfirmed(&dest_for_sync, e);
            }
            // The entry naming it gets its directory sync at the next pass, each
            // directory once however many files it gained.
            if let Some(parent) = dest_for_sync.parent() {
                barriers.note_dir(parent);
            }
            let report = apply_staged(StagedItem::File(&file), &carried_for_apply, times);
            if report.any_refused() {
                log::warn!(
                    target: "local_streams",
                    "write_from_stream: {} took only some of the source's metadata: {report:?}",
                    LogPrivate::file_path(&dest_for_sync)
                );
            }
            (file, report)
        })
        .await
        .expect("spawn_blocking sync_data closure doesn't panic and the task is uncancelable");
        drop(file);

        // No `notify_mutation` call here: a local volume's mutation methods
        // (create_file/delete/rename) rely on the OS watcher (FSEvents, inotify,
        // `ReadDirectoryChangesW`) to patch the cache. The SMB / MTP overrides
        // need it because their out-of-band notification channels can lose
        // events; we don't.
        Ok(StreamWritten::with_report(
            bytes_written,
            &report,
            &carried,
            times.modified,
        ))
    })
}
