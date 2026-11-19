//! A GET whose answer names no length, or ends before the length it named,
//! against [`FakeDav`] (#538).
//!
//! A proxy can strip `Content-Length` and send the body close-delimited. The
//! stream then declares what the CALLER knows (the listing's size, its hint),
//! never "exactly zero bytes", and a body that ends before the length the
//! server did name (a `206`'s `Content-Range`) fails instead of ending
//! cleanly, so nothing short is ever handed on as the whole file.

use super::testing::at;
use cmdr_fs::volume::{StreamLength, Volume, VolumeError, VolumeReadStream};

use super::fake_dav::{FILE_LEN, FakeDav};

/// Every chunk until the end, or the first failure.
async fn drain(stream: &mut Box<dyn VolumeReadStream>) -> Result<usize, VolumeError> {
    let mut read = 0;
    while let Some(chunk) = stream.next_chunk().await {
        read += chunk?.len();
    }
    Ok(read)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_without_a_length_declares_the_callers_hint() {
    let server = FakeDav::start().await;
    let (volume, _) = server.connected().await;
    let path = at(&volume, "nolength.txt");

    let mut stream = volume
        .open_read_stream_with_hint(&path, Some(FILE_LEN as u64))
        .await
        .unwrap();

    assert_eq!(stream.total_size(), StreamLength::Known(FILE_LEN as u64));
    assert_eq!(drain(&mut stream).await.unwrap(), FILE_LEN);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_without_a_length_or_a_hint_says_it_doesnt_know() {
    let server = FakeDav::start().await;
    let (volume, _) = server.connected().await;

    let stream = volume.open_read_stream(&at(&volume, "nolength.txt")).await.unwrap();

    assert_eq!(
        stream.total_size(),
        StreamLength::Unknown,
        "❌ never `Known(0)`: a copy would be told to expect an empty file"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ranged_read_that_ends_before_its_named_length_fails() {
    let server = FakeDav::start().await;
    let (volume, _) = server.connected().await;

    let mut stream = volume
        .open_read_stream_at_offset(&at(&volume, "cut.txt"), 2)
        .await
        .unwrap();
    let drained = drain(&mut stream).await;

    assert!(
        matches!(drained, Err(VolumeError::IoError { .. })),
        "8 bytes named, 3 sent: the end of the body is a cut, not the end of the file: {drained:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ranged_read_that_arrives_whole_ends_cleanly() {
    let server = FakeDav::start().await;
    let (volume, _) = server.connected().await;

    let mut stream = volume
        .open_read_stream_at_offset(&at(&volume, "nolength.txt"), 2)
        .await
        .unwrap();

    assert_eq!(drain(&mut stream).await.unwrap(), FILE_LEN - 2);
}
