//! Every blocking job the archive starts (the parse, an entry's decompress, a
//! one-pass subtree extract, and the remote byte source's bridge) runs on the
//! runtime the `VolumeHost` hands down, never an ambient one. Each test drives an
//! `ArchiveVolume` from a plain OS thread with NO tokio context, polling with a
//! std-only executor, so an ambient `spawn_blocking` / `Handle::current` panics.

use std::future::Future;
use std::io::Write;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

use super::*;
use crate::test_fixtures::{build_zip, deflated, stored};
use crate::testing::at;
use cmdr_fs::app_path;
use cmdr_fs::testing::AppTestDir;
use cmdr_fs::volume::{InMemoryVolume, VolumeReadStream};

/// Wakes the polling thread by unparking it.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls `future` to completion on THIS thread, with no tokio context entered.
fn poll_to_completion<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::park();
    }
}

/// Runs `body` on a fresh OS thread that has no ambient runtime, handing it a
/// host whose runtime is one this test owns.
fn on_a_thread_without_a_runtime<T: Send + 'static>(body: impl FnOnce(VolumeHost) -> T + Send + 'static) -> T {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .expect("build the host's runtime");
    let host = VolumeHost::builder().runtime(runtime.handle().clone()).build();
    let answer = std::thread::spawn(move || body(host))
        .join()
        .expect("the archive must not reach for an ambient runtime");
    drop(runtime);
    answer
}

async fn drain(stream: &mut dyn VolumeReadStream) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.expect("read a chunk"));
    }
    out
}

/// Writes `bytes` as `name` in `dir`, answering its app path.
fn write_fixture(dir: &AppTestDir, name: &str, bytes: &[u8]) -> AppPathBuf {
    std::fs::write(dir.native().join(name), bytes).expect("write the fixture");
    at(dir.app_path(), name)
}

#[test]
fn a_local_zip_parses_and_decompresses_on_the_hosts_runtime() {
    let dir = AppTestDir::new("archive-runtime-local");
    let zip = write_fixture(
        &dir,
        "local.zip",
        &build_zip(&[deflated("docs/a.txt", "hello, runtime")]),
    );

    let (listed, bytes) = on_a_thread_without_a_runtime(move |host| {
        let parent = Arc::new(InMemoryVolume::new("parent").with_local_fs_access());
        let volume = ArchiveVolume::new_at(parent, zip.clone(), ArchiveFormat::Zip, host);
        poll_to_completion(async {
            let listed = volume.list_directory(&at(&zip, "docs"), None).await.expect("list");
            let mut stream = volume.open_read_stream(&at(&zip, "docs/a.txt")).await.expect("open");
            (listed.len(), drain(stream.as_mut()).await)
        })
    });

    assert_eq!(listed, 1);
    assert_eq!(bytes, b"hello, runtime");
}

#[test]
fn a_remote_zip_bridges_its_reads_on_the_hosts_runtime() {
    // A remote parent: every byte arrives through `VolumeByteSource`, which blocks
    // on the parent's async `read_range` from inside the blocking parse.
    let zip = app_path!("/device/remote.zip").to_owned();
    let bytes = build_zip(&[stored("a.txt", "over the wire")]);

    let read = on_a_thread_without_a_runtime(move |host| {
        let parent = InMemoryVolume::new("Remote");
        poll_to_completion(async {
            parent.create_file(&zip, &bytes).await.expect("seed the remote zip");
            let volume = ArchiveVolume::new_at(Arc::new(parent), zip.clone(), ArchiveFormat::Zip, host);
            let mut stream = volume.open_read_stream(&at(&zip, "a.txt")).await.expect("open");
            drain(stream.as_mut()).await
        })
    });

    assert_eq!(read, b"over the wire");
}

#[test]
fn a_tar_gz_subtree_extract_decodes_on_the_hosts_runtime() {
    let dir = AppTestDir::new("archive-runtime-tgz");
    let mut builder = ::tar::Builder::new(Vec::new());
    let mut header = ::tar::Header::new_ustar();
    header.set_mode(0o644);
    header.set_size(5);
    header.set_entry_type(::tar::EntryType::Regular);
    header.set_cksum();
    builder
        .append_data(&mut header, "docs/b.txt", &b"tarry"[..])
        .expect("append");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&builder.into_inner().expect("finish tar")).expect("gzip");
    let tgz = write_fixture(&dir, "bundle.tar.gz", &gz.finish().expect("finish gzip"));

    let read = on_a_thread_without_a_runtime(move |host| {
        let parent = Arc::new(InMemoryVolume::new("parent").with_local_fs_access());
        let volume = ArchiveVolume::new_at(parent, tgz.clone(), ArchiveFormat::Tar(crate::TarCodec::Gzip), host);
        poll_to_completion(async {
            let mut extract = volume
                .open_sequential_extract(&at(&tgz, "docs"))
                .await
                .expect("open the extract");
            extract.next_file().await.expect("next_file").expect("the one member");
            drain(extract.current_stream().as_mut()).await
        })
    });

    assert_eq!(read, b"tarry");
}
