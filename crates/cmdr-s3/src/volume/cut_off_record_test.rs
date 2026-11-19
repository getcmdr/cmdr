//! A cut-off PUT left on record (#522), against a fake S3 on a provider not
//! trusted to refuse a short body: what the record clears, and ❌ what it never
//! touches. The Docker twin against VersityGW, with the real outage, is
//! `connection_drop_test.rs::a_put_cut_off_by_an_outage_leaves_nothing_for_the_retry_to_trip_over`.

use cmdr_fs::app_path::AppPath;
use std::ops::ControlFlow;
use std::time::Duration;

use cmdr_fs::volume::{Volume, VolumeReadStream, WriteMode};
use url::Url;

use super::S3Volume;
use super::fake_s3::FakeS3;
use super::testing::BytesSource;
use super::upload_ledger::UnsettledPut;
use crate::params::S3Provider;

const LEN: usize = 4_096;

/// A volume on the fake as an "Other" provider (`refuses_short_body` off),
/// under an account of its own: the ledger is process-wide.
fn other_provider(s3: &FakeS3, host: &str) -> S3Volume {
    s3.volume_for(S3Provider::Other {
        endpoint: Url::parse(&format!("http://{host}")).expect("a valid URL"),
        region: None,
        path_style: true,
    })
}

/// Puts what a cut-off PUT carrying `token` left at `key` (`kept` bytes of
/// `LEN`) on the fake, and the PUT on the volume's ledger, as a write whose
/// cleanup couldn't reach the server leaves them.
fn leave_cut_off(s3: &FakeS3, volume: &S3Volume, key: &str, token: &str, kept: usize) -> UnsettledPut {
    s3.seed_with_meta(key, kept, &[&format!("x-amz-meta-cmdr-write: {token}")]);
    let put = UnsettledPut {
        account: volume.inner.account(),
        bucket: super::fake_s3::BUCKET.to_string(),
        key: key.to_string(),
        token: token.to_string(),
        size: LEN as u64,
    };
    volume.inner.ledger.started(&put);
    volume.inner.ledger.abandoned(&put);
    put
}

async fn write_new(volume: &S3Volume, key: &str) -> Result<u64, cmdr_fs::volume::VolumeError> {
    let source = BytesSource::new(vec![7u8; LEN]);
    volume
        .write_from_stream(
            &AppPath::legacy_unchecked(&volume.root().as_std_legacy().join(key)),
            WriteMode::CreateNew,
            source.total_size(),
            Box::new(source),
            &|_| ControlFlow::Continue(()),
        )
        .await
        .map(|written| written.bytes)
}

/// The retry after an outage: the earlier PUT's short body is removed before
/// the write's own no-overwrite check, which would otherwise answer "already
/// exists" and leave the truncated file under the user's name.
#[tokio::test(flavor = "multi_thread")]
async fn the_next_write_to_the_key_clears_a_cut_off_body_and_lands() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    let volume = other_provider(&s3, "next-write.example");
    leave_cut_off(&s3, &volume, "cut.bin", "token-earlier", LEN / 3);

    let written = write_new(&volume, "cut.bin").await;

    assert!(matches!(written, Ok(n) if n == LEN as u64), "{written:?}");
    assert_eq!(s3.object("cut.bin").map(|stored| stored.len), Some(LEN));
}

/// The next connect's sweep clears it too, and settles the record.
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_clears_a_cut_off_body_and_forgets_it() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    let volume = other_provider(&s3, "sweep.example");
    let put = leave_cut_off(&s3, &volume, "cut.bin", "token-earlier", LEN / 3);

    volume.sweep_unfinished_uploads().await;

    assert!(s3.object("cut.bin").is_none(), "the truncated body is gone");
    assert!(
        volume.inner.ledger.unsettled_puts(&put.account).is_empty(),
        "and nothing is left on record"
    );
}

/// ❌ Never someone else's object, and ❌ never ours whole: a PUT whose
/// answer was lost after the server published it landed.
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_leaves_an_object_that_is_whole_or_not_ours() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    let volume = other_provider(&s3, "keeps.example");
    leave_cut_off(&s3, &volume, "whole.bin", "token-whole", LEN);
    let theirs = leave_cut_off(&s3, &volume, "theirs.bin", "token-ours", LEN / 3);
    s3.seed_with_meta("theirs.bin", LEN / 3, &["x-amz-meta-cmdr-write: token-another-writer"]);

    volume.sweep_unfinished_uploads().await;

    assert_eq!(s3.object("whole.bin").map(|stored| stored.len), Some(LEN));
    assert_eq!(s3.object("theirs.bin").map(|stored| stored.len), Some(LEN / 3));
    assert!(volume.inner.ledger.unsettled_puts(&theirs.account).is_empty());
}

/// ❗ **Only a body PROVABLY short is ours to delete** (#522/#545): a HEAD with
/// no length can't show it's short, and one LONGER than the PUT sent is a
/// later write by a client that kept our `x-amz-meta-*` token on overwrite.
/// Either may be a whole user file, so both stay, and the record is let go.
#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_leaves_an_object_longer_than_the_put_sent() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    let volume = other_provider(&s3, "unproven.example");
    let longer = leave_cut_off(&s3, &volume, "longer.bin", "token-kept", LEN * 2);

    volume.sweep_unfinished_uploads().await;

    assert_eq!(s3.object("longer.bin").map(|stored| stored.len), Some(LEN * 2));
    assert!(volume.inner.ledger.unsettled_puts(&longer.account).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sweep_leaves_an_object_whose_head_has_no_length() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    let volume = other_provider(&s3, "lengthless.example");
    let lengthless = leave_cut_off(&s3, &volume, "lengthless.bin", "token-lengthless", LEN / 3);
    s3.answer_heads_without_length();

    volume.sweep_unfinished_uploads().await;

    assert_eq!(s3.object("lengthless.bin").map(|stored| stored.len), Some(LEN / 3));
    assert!(volume.inner.ledger.unsettled_puts(&lengthless.account).is_empty());
}
