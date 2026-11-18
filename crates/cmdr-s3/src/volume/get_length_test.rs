//! A GET whose answer names no length, or ends before the length it named,
//! against a fake S3 (#538).
//!
//! A proxy can strip `content-length` and send the body close-delimited. The
//! stream then declares what the CALLER knows (the listing's size, its hint),
//! never "exactly zero bytes", and a body that ends before the length the
//! server did name (a `206`'s `content-range`) fails instead of ending
//! cleanly, so nothing short is ever handed on as the whole file.

use cmdr_fs::app_path::AppPath;
use std::time::Duration;

use cmdr_fs::volume::{StreamLength, Volume, VolumeError, VolumeReadStream};

use super::fake_s3::FakeS3;

const LEN: usize = 10;

async fn serving(cut_after: Option<usize>) -> FakeS3 {
    let s3 = FakeS3::start(Duration::ZERO).await;
    s3.seed("ten.bin", LEN);
    s3.answer_gets_without_length(cut_after);
    s3
}

/// Every chunk until the end, or the first failure.
async fn drain(stream: &mut Box<dyn VolumeReadStream>) -> Result<usize, VolumeError> {
    let mut read = 0;
    while let Some(chunk) = stream.next_chunk().await {
        read += chunk?.len();
    }
    Ok(read)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_without_a_length_declares_the_callers_hint() {
    let s3 = serving(None).await;
    let volume = s3.volume();
    let path = volume.root().as_std_legacy().join("ten.bin");

    let mut stream = volume
        .open_read_stream_with_hint(&AppPath::legacy_unchecked(&path), Some(LEN as u64))
        .await
        .unwrap();

    assert_eq!(stream.total_size(), StreamLength::Known(LEN as u64));
    assert_eq!(drain(&mut stream).await.unwrap(), LEN);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_without_a_length_or_a_hint_says_it_doesnt_know() {
    let s3 = serving(None).await;
    let volume = s3.volume();

    let stream = volume
        .open_read_stream(&AppPath::legacy_unchecked(
            &volume.root().as_std_legacy().join("ten.bin"),
        ))
        .await
        .unwrap();

    assert_eq!(
        stream.total_size(),
        StreamLength::Unknown,
        "❌ never `Known(0)`: a copy would be told to expect an empty file"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ranged_read_that_ends_before_its_named_length_fails() {
    let s3 = serving(Some(3)).await;
    let volume = s3.volume();

    let mut stream = volume
        .open_read_stream_at_offset(
            &AppPath::legacy_unchecked(&volume.root().as_std_legacy().join("ten.bin")),
            2,
        )
        .await
        .unwrap();
    let drained = drain(&mut stream).await;

    assert!(
        matches!(drained, Err(VolumeError::IoError { .. })),
        "8 bytes named, 3 sent: the end of the body is a cut, not the end of the file: {drained:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ranged_read_that_arrives_whole_ends_cleanly() {
    let s3 = serving(None).await;
    let volume = s3.volume();

    let mut stream = volume
        .open_read_stream_at_offset(
            &AppPath::legacy_unchecked(&volume.root().as_std_legacy().join("ten.bin")),
            2,
        )
        .await
        .unwrap();

    assert_eq!(drain(&mut stream).await.unwrap(), LEN - 2);
}
