//! A multipart upload another writer's PUT ended, whose part answers are lost
//! to a reset: Garage answers `NoSuchUpload` before reading the body and drops
//! the connection, so under load the client sees a broken pipe, ❌ never the
//! 404 (`apps/desktop/test/s3-servers/README.md`). Regression anchor: CI run
//! 39495674734 (2026-10-20) failed `…catches_a_writer_mid_upload_on_garage`
//! with `DeviceDisconnected` after all four attempts of a part, and runs
//! 39638912868 and 39684718106 hit the same race. A `CreateNew` must still
//! come out as the name being taken, with the other writer's object kept and
//! no upload left.

use std::ops::ControlFlow;
use std::time::Duration;

use cmdr_fs::volume::{Volume, VolumeError, VolumeReadStream, WriteMode};

use super::fake_s3::FakeS3;
use super::test_support::at;
use super::testing::BytesSource;

const MIB: usize = 1024 * 1024;

#[tokio::test(flavor = "multi_thread")]
async fn a_create_new_whose_upload_another_writer_ended_is_refused_as_taken() {
    let s3 = FakeS3::start(Duration::ZERO).await;
    let volume = s3.volume();
    volume.set_part_floor(5 * MIB as u64);
    s3.taken_before_next_part("raced.bin");
    let source = BytesSource::new(vec![7u8; 12 * MIB]);
    let length = source.total_size();
    let outcome = volume
        .write_from_stream(
            &at(&volume, "raced.bin"),
            WriteMode::CreateNew,
            length,
            Box::new(source),
            &|_| ControlFlow::Continue(()),
        )
        .await;
    assert!(matches!(outcome, Err(VolumeError::AlreadyExists(_))), "got {outcome:?}");
    assert_eq!(
        s3.object("raced.bin").map(|stored| stored.len),
        Some(6),
        "the other writer's object stays"
    );
    assert_eq!(s3.open_uploads(), 0, "nothing of ours is left on the server");
}
