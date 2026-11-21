use super::*;
use cmdr_fs::app_path;

#[test]
fn os_join_prepends_the_mount_root() {
    assert_eq!(
        os_join("/Volumes/testshare", "/DCIM/x.jpg"),
        "/Volumes/testshare/DCIM/x.jpg"
    );
    assert_eq!(
        os_join("/Volumes/testshare/", "/DCIM/x.jpg"),
        "/Volumes/testshare/DCIM/x.jpg"
    );
    // Root / local volume: index-relative is already absolute.
    assert_eq!(os_join("/", "/a/b.jpg"), "/a/b.jpg");
    assert_eq!(os_join("", "/a/b.jpg"), "/a/b.jpg");
}

#[test]
fn os_folder_to_index_prefix_is_the_inverse_of_os_join() {
    // Network volume: strip the mount root to reach the stored index-path space.
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/testshare/Photos", "/Volumes/testshare"),
        Some("/Photos".to_string())
    );
    // The mount root itself ⇒ the whole volume (empty prefix matches every path).
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/testshare", "/Volumes/testshare"),
        Some(String::new())
    );
    // Local / root volume: index path == OS path, so the folder passes through.
    assert_eq!(
        os_folder_to_index_prefix("/Users/me/Documents/IDs", "/"),
        Some("/Users/me/Documents/IDs".to_string())
    );
    // A folder on a DIFFERENT volume isn't under this mount ⇒ None (skip it).
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/other/x", "/Volumes/testshare"),
        None
    );
    // A name-prefix sibling is NOT within the mount.
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/testshare2/x", "/Volumes/testshare"),
        None
    );
}

/// The exclusion veto already matches a folder against the mount root with the
/// platform's case and Unicode folding (`path_is_within`); the purge has to fold the
/// same way, or an exclusion spelled differently from the mount root is honored by
/// every search while its stored rows stay on disk. The stored rest keeps the
/// folder's own spelling.
#[cfg(target_os = "macos")]
#[test]
fn os_folder_to_index_prefix_folds_the_mount_root_like_the_veto() {
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/TESTSHARE/Photos", "/Volumes/testshare"),
        Some("/Photos".to_string()),
        "a mount root differing only in case"
    );
    // "Fotók" in NFC (precomposed ó) against a mount root in NFD (o + combining acute).
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/Fot\u{f3}k/Kép", "/Volumes/Foto\u{301}k"),
        Some("/Kép".to_string()),
        "a mount root differing only in Unicode form"
    );
    assert_eq!(
        os_folder_to_index_prefix("/volumes/Testshare", "/Volumes/testshare/"),
        Some(String::new()),
        "the mount root itself, folded"
    );
    assert_eq!(
        os_folder_to_index_prefix("/Volumes/TESTSHARE2/x", "/Volumes/testshare"),
        None
    );
}

#[test]
fn fs_fetch_reads_a_real_file() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("x.bin");
    std::fs::write(&path, b"hello bytes").expect("write");
    let bytes = FsByteFetcher
        .fetch(&path.to_string_lossy(), None, Duration::from_secs(5))
        .expect("fetch");
    assert_eq!(bytes, b"hello bytes");
}

/// The hung-mount backstop: a read that never returns must come back as a
/// `Disconnected` (pause the pass, resume on reconnect) rather than wedge the
/// enrichment thread forever. The branch a dead NAS reaches in production, where
/// `std::fs` blocks in the kernel with no errno ever arriving.
///
/// A FIFO with no writer stands in for it: `File::open` on the read end blocks
/// until someone opens the write end, which nothing here ever does. Deterministic
/// and local — the previous cover for this path was a real-NAS test that handed a
/// working mount a 30 s budget, so it never actually reached the timeout.
///
/// The detached reader thread stays parked on that open for the rest of the
/// process. It holds no lock and nextest forks a process per test, so it goes away
/// with the test; under a shared-process `cargo test` it idles until the run ends.
#[test]
#[cfg(unix)]
fn fs_fetch_times_out_on_a_hung_read_and_reports_a_disconnect() {
    let dir = tempfile::tempdir().expect("temp");
    let fifo = dir.path().join("hung.jpg");
    cmdr_fs::testing::make_fifo(&fifo).expect("mkfifo");

    let budget = Duration::from_millis(50);
    let started = std::time::Instant::now();
    let err = FsByteFetcher
        .fetch(&fifo.to_string_lossy(), None, budget)
        .expect_err("a read that never returns must not yield bytes");

    // The variant is what the pass branches on.
    assert!(
        matches!(err, FetchError::Disconnected(_)),
        "a hung read must classify as Disconnected, got {err:?}"
    );
    // The CLOCK is what proves the TIMEOUT arm rather than an errno the open
    // happened to return instead: `recv_timeout` reaches that arm only once its
    // deadline has passed, so an elapsed at least as long as the budget can't have
    // come from an errno or a dropped sender, both of which return at once.
    let elapsed = started.elapsed();
    assert!(
        elapsed >= budget,
        "returning inside the {budget:?} budget means an errno arm, not the timeout: took {elapsed:?}"
    );
    // And it gave up on the budget instead of blocking. Generous upper bound: this
    // asserts "bounded", not a scheduling deadline.
    assert!(
        elapsed < Duration::from_secs(5),
        "the fetch should return on its {budget:?} budget, took {elapsed:?}"
    );
}

#[test]
fn fs_fetch_missing_file_is_not_found() {
    let err = FsByteFetcher
        .fetch("/nonexistent/cmdr/media/x.jpg", None, Duration::from_secs(5))
        .expect_err("missing file errors");
    assert!(matches!(err, FetchError::NotFound));
}

/// THE M1 classification fix: a permission-denied file is a PER-FILE fault
/// (skip-and-count), never a "disconnected" (which pauses the whole pass —
/// the TCC-EPERM-stalls-the-NAS bug this distinction exists for).
#[test]
#[cfg(unix)]
fn fs_fetch_permission_denied_is_unreadable_not_disconnected() {
    use std::os::unix::fs::PermissionsExt;
    // Root bypasses file permissions entirely (CAP_DAC_OVERRIDE): a 0o000 file
    // opens fine, so the EACCES scenario is UNATTAINABLE under root — and the
    // local Linux test lane runs in Docker as root. The errno → classification
    // contract this test guards end to end is covered unconditionally by
    // `io_errors_classify_by_errno_kind`; this real-chmod variant runs wherever
    // a non-root euid can actually produce the errno (macOS dev machines, CI
    // runners).
    // SAFETY: geteuid takes no arguments, touches no memory, and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("locked.jpg");
    std::fs::write(&path, b"jpeg bytes").expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let err = FsByteFetcher
        .fetch(&path.to_string_lossy(), None, Duration::from_secs(5))
        .expect_err("an unreadable file errors");
    // Restore permissions so the tempdir cleanup can delete it.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod back");
    assert!(
        matches!(err, FetchError::Unreadable(_)),
        "EACCES must classify as a per-file Unreadable, got {err:?}"
    );
}

/// The direct-session fetcher reads whole files through the `Volume` trait
/// (the M1 read-path fix), from a plain thread with no ambient runtime — the
/// exact shape of the enrichment pass's `spawn_blocking` / fetcher threads.
#[test]
fn volume_fetch_reads_bytes_through_the_volume_trait() {
    use cmdr_fs::volume::InMemoryVolume;
    use cmdr_fs::volume::Volume;
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let volume = std::sync::Arc::new(InMemoryVolume::new("test"));
    rt.block_on(volume.create_file(app_path!("/DCIM/a.jpg"), b"direct bytes"))
        .expect("seed");
    let fetcher = VolumeByteFetcher::new(volume, rt.handle().clone());

    let bytes = thread::scope(|s| {
        s.spawn(|| fetcher.fetch("/DCIM/a.jpg", Some(12), Duration::from_secs(5)))
            .join()
            .expect("thread")
    })
    .expect("fetch");
    assert_eq!(bytes, b"direct bytes");
}

/// Typed classification end to end on the direct path: a vanished file is
/// `NotFound` (skip; GC collects it), and a known-oversized file is `TooLarge`
/// WITHOUT touching the wire.
#[test]
fn volume_fetch_classifies_not_found_and_oversize_hint() {
    use cmdr_fs::volume::InMemoryVolume;
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let volume = std::sync::Arc::new(InMemoryVolume::new("test"));
    let fetcher = VolumeByteFetcher::new(volume, rt.handle().clone());

    let err = fetcher
        .fetch("/gone.jpg", Some(10), Duration::from_secs(5))
        .expect_err("missing file");
    assert!(matches!(err, FetchError::NotFound), "got {err:?}");

    let err = fetcher
        .fetch("/whatever.jpg", Some(MAX_IMAGE_BYTES + 1), Duration::from_secs(5))
        .expect_err("over-cap hint");
    assert!(matches!(err, FetchError::TooLarge), "got {err:?}");
}

/// The joined text reaches the volume as one typed app path: a name holding `\` or `:`
/// stays one name, and text that is no app path (relative, a `..`, a NUL) is a per-file
/// skip, never a read of whatever the text would climb to.
#[test]
fn volume_fetch_reads_the_joined_text_as_one_app_path() {
    use cmdr_fs::volume::{InMemoryVolume, Volume};
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let volume = std::sync::Arc::new(InMemoryVolume::new("test"));
    rt.block_on(volume.create_file(app_path!("/DCIM/a\\b 12:00.jpg"), b"odd name"))
        .expect("seed");
    rt.block_on(volume.create_file(app_path!("/a.jpg"), b"top"))
        .expect("seed");
    let fetcher = VolumeByteFetcher::new(volume, rt.handle().clone());

    let bytes = thread::scope(|s| {
        s.spawn(|| fetcher.fetch("/DCIM/a\\b 12:00.jpg", None, Duration::from_secs(5)))
            .join()
            .expect("thread")
    })
    .expect("fetch");
    assert_eq!(bytes, b"odd name");

    for text in ["a.jpg", "/DCIM/../a.jpg", "/a\0.jpg", ""] {
        let err = thread::scope(|s| {
            s.spawn(|| fetcher.fetch(text, None, Duration::from_secs(5)))
                .join()
                .expect("thread")
        })
        .expect_err("no app path names a file to read");
        assert!(matches!(err, FetchError::Unreadable(_)), "{text:?}: got {err:?}");
    }
}

/// The `VolumeError` line between pause and skip: only a typed
/// `DeviceDisconnected` / `ConnectionTimeout` pauses; every other volume error
/// (permission, I/O, MTP's session reset) is a per-file skip.
#[test]
fn volume_errors_classify_disconnect_vs_unreadable_by_typed_variant() {
    use cmdr_fs::volume::VolumeError;
    assert!(matches!(
        classify_volume_error(VolumeError::DeviceDisconnected("gone".into())),
        FetchError::Disconnected(_)
    ));
    assert!(matches!(
        classify_volume_error(VolumeError::ConnectionTimeout("slow".into())),
        FetchError::Disconnected(_)
    ));
    assert!(matches!(
        classify_volume_error(VolumeError::NotFound("x".into())),
        FetchError::NotFound
    ));
    for per_file in [
        VolumeError::PermissionDenied {
            path: "locked".into(),
            raw_os_error: None,
        },
        VolumeError::IsADirectory("dir".into()),
        VolumeError::IoError {
            message: "bad sector".into(),
            raw_os_error: Some(5),
        },
        // MTP-only today, and its docs forbid mapping it to a disconnect.
        VolumeError::DeviceSessionReset("reset".into()),
    ] {
        let classified = classify_volume_error(per_file);
        assert!(
            matches!(classified, FetchError::Unreadable(_)),
            "expected Unreadable, got {classified:?}"
        );
    }
}

/// A mid-stream typed disconnect (the cable-yank case) surfaces as
/// `Disconnected` even after good chunks arrived — a partial read must never
/// pass itself off as the file's bytes or as a per-file fault.
#[test]
fn volume_fetch_maps_a_mid_stream_disconnect_to_disconnected() {
    use cmdr_fs::volume::{Volume, VolumeError, VolumeReadStream};
    use std::pin::Pin;

    struct YankedStream {
        sent: bool,
    }
    impl VolumeReadStream for YankedStream {
        fn next_chunk(&mut self) -> Pin<Box<dyn Future<Output = Option<Result<Vec<u8>, VolumeError>>> + Send + '_>> {
            Box::pin(async move {
                if self.sent {
                    Some(Err(VolumeError::DeviceDisconnected("cable yanked".into())))
                } else {
                    self.sent = true;
                    Some(Ok(vec![0u8; 1024]))
                }
            })
        }
        fn total_size(&self) -> cmdr_fs::volume::StreamLength {
            cmdr_fs::volume::StreamLength::Known(4096)
        }
        fn bytes_read(&self) -> u64 {
            if self.sent { 1024 } else { 0 }
        }

        fn modified_at(&self) -> Option<std::time::SystemTime> {
            None
        }
    }

    struct YankedVolume;
    impl Volume for YankedVolume {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn name(&self) -> &str {
            "yanked"
        }
        fn root(&self) -> &AppPath {
            AppPath::from_normalized("/").expect("a canonical literal")
        }
        fn list_directory<'a>(
            &'a self,
            _path: &'a AppPath,
            _on_progress: Option<&'a (dyn Fn(cmdr_fs::volume::ListingProgress) + Sync)>,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<cmdr_fs::entry::FileEntry>, VolumeError>> + Send + 'a>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn get_metadata<'a>(
            &'a self,
            _path: &'a AppPath,
        ) -> Pin<Box<dyn Future<Output = Result<cmdr_fs::entry::FileEntry, VolumeError>> + Send + 'a>> {
            Box::pin(async { Err(VolumeError::NotSupported) })
        }
        fn exists<'a>(&'a self, _path: &'a AppPath) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
            Box::pin(async { true })
        }
        fn is_directory<'a>(
            &'a self,
            _path: &'a AppPath,
        ) -> Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
            Box::pin(async { Ok(false) })
        }
        fn open_read_stream<'a>(
            &'a self,
            _path: &'a AppPath,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn VolumeReadStream>, VolumeError>> + Send + 'a>> {
            Box::pin(async { Ok(Box::new(YankedStream { sent: false }) as Box<dyn VolumeReadStream>) })
        }
    }

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let fetcher = VolumeByteFetcher::new(std::sync::Arc::new(YankedVolume), rt.handle().clone());
    let err = fetcher
        .fetch("/DCIM/a.jpg", Some(4096), Duration::from_secs(5))
        .expect_err("yank errors");
    assert!(matches!(err, FetchError::Disconnected(_)), "got {err:?}");
}

/// The characterization table, every errno: the nine that say the mount is gone
/// pause, a missing file is its own answer, and every other one skips the file.
#[cfg(unix)]
#[test]
fn every_errno_keeps_its_fetch_answer() {
    let transport_loss = [
        libc::ETIMEDOUT,
        libc::ENOTCONN,
        libc::ENETDOWN,
        libc::ENETUNREACH,
        libc::EHOSTDOWN,
        libc::EHOSTUNREACH,
        libc::ENODEV,
        libc::ENXIO,
        libc::ESTALE,
    ];
    for errno in 0..=4096 {
        let classified = classify_io_error(&std::io::Error::from_raw_os_error(errno), "/m/x.jpg", "read");
        match classified {
            FetchError::NotFound => assert_eq!(errno, libc::ENOENT, "errno {errno}"),
            FetchError::Disconnected(_) => assert!(transport_loss.contains(&errno), "errno {errno}"),
            FetchError::Unreadable(_) => {
                assert!(
                    errno != libc::ENOENT && !transport_loss.contains(&errno),
                    "errno {errno}"
                );
            }
            other => panic!("errno {errno}: {other:?}"),
        }
    }
}

/// The errno line between "this FILE is bad" and "the MOUNT is gone": network-
/// transport errnos pause (disconnect), anything else skips (unreadable).
#[cfg(unix)]
#[test]
fn io_errors_classify_by_errno_kind() {
    use std::io::{Error, ErrorKind};
    // Typed transport-loss errnos ⇒ Disconnected (pause, resume on reconnect).
    for errno in [
        libc::ETIMEDOUT,
        libc::ENOTCONN,
        libc::ENETDOWN,
        libc::ENETUNREACH,
        libc::EHOSTDOWN,
        libc::EHOSTUNREACH,
        libc::ENODEV,
        libc::ENXIO,
        libc::ESTALE,
    ] {
        let classified = classify_io_error(&Error::from_raw_os_error(errno), "/m/x.jpg", "read");
        assert!(
            matches!(classified, FetchError::Disconnected(_)),
            "errno {errno} is transport loss, got {classified:?}"
        );
    }
    // Per-file errnos ⇒ Unreadable (skip-and-count, never a pause).
    for errno in [libc::EACCES, libc::EPERM, libc::EISDIR, libc::EIO] {
        let classified = classify_io_error(&Error::from_raw_os_error(errno), "/m/x.jpg", "read");
        assert!(
            matches!(classified, FetchError::Unreadable(_)),
            "errno {errno} is a per-file fault, got {classified:?}"
        );
    }
    // NotFound stays its own variant (vanished source; GC collects it).
    let nf = classify_io_error(&Error::from(ErrorKind::NotFound), "/m/x.jpg", "open");
    assert!(matches!(nf, FetchError::NotFound));
}
