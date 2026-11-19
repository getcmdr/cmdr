//! The shared `Volume` conformance promises, against both fixtures.
//!
//! ❗ **Both servers run check-then-write here**: the "Other S3-compatible"
//! profile trusts no conditional header (`profile.rs`), so the no-clobber cells
//! prove the HEAD-then-write path, the one every provider but AWS and R2 takes.
//! The header path has its own cells in `write_test.rs`, against VersityGW.

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::conformance;

use super::test_support::at;
use super::testing::*;
use cmdr_fs::app_path::AppPath;
use std::path::PathBuf;

/// `rel` under the volume's root, as the `std::path` value `conformance`'s assertions still
/// take: the file's one bridge, which goes once `conformance` takes app paths.
fn conformance_at(volume: &dyn Volume, rel: &str) -> PathBuf {
    conformance_path(&at(volume, rel))
}

fn conformance_path(path: &AppPath) -> PathBuf {
    path.as_std_legacy().into_owned()
}

/// Seeds `files` under `prefix` in the fixture bucket.
async fn seeded(service: FixtureService, prefix: &str, files: &[(&str, &[u8])]) {
    let keys: Vec<String> = files.iter().map(|(name, _)| format!("{prefix}{name}")).collect();
    let seeds: Vec<Seed<'_>> = keys
        .iter()
        .zip(files)
        .map(|(key, (_, bytes))| object(key, bytes))
        .collect();
    seed(service, FIXTURE_BUCKET, &seeds).await;
}

async fn a_place_keeps_the_write_promises(service: FixtureService) {
    let volume = connect_fixture(service, Some(FIXTURE_BUCKET)).await;
    let prefix = scratch_prefix("conformance-writes");
    let at = |name: &str| conformance_at(&volume, &format!("{prefix}{name}"));
    seeded(
        service,
        &prefix,
        &[
            ("notes.txt", b"the user's notes"),
            ("source.txt", b"source"),
            ("target.txt", b"the user's target file"),
            ("album/keep.txt", b"content"),
            ("blocker", b"a file where a folder is asked for"),
            ("level/a.txt", b"doomed"),
            ("level/b.txt", b"doomed"),
            ("level/kept.txt", b"kept"),
        ],
    )
    .await;

    conformance::assert_writability_matches_the_mutations_offered(&volume, &at("unborn")).await;
    conformance::assert_create_file_refuses_to_clobber(&volume, &at("notes.txt"), b"new").await;
    conformance::assert_write_from_stream_create_new_refuses_to_clobber(
        &volume,
        &at("notes.txt"),
        &at("fresh.txt"),
        b"new",
    )
    .await;
    conformance::assert_rename_refuses_an_existing_destination(&volume, &at("source.txt"), &at("target.txt")).await;
    conformance::assert_delete_leaves_a_non_empty_dir_intact(&volume, &at("album"), "keep.txt").await;
    conformance::assert_create_directory_all_reports_an_existing_dir_honestly(&volume, &at("album")).await;
    conformance::assert_create_directory_all_refuses_a_file_in_the_way(&volume, &at("blocker")).await;
    conformance::assert_conflict_scan_reads_a_missing_destination_as_empty(&volume, &at("not-created-yet")).await;
    conformance::assert_delete_files_removes_exactly_what_it_names(
        &volume,
        [&at("level/a.txt"), &at("level/b.txt")],
        &at("level/kept.txt"),
    )
    .await;
    conformance::assert_write_from_stream_keeps_the_source_date(&volume, &at("dated.txt"), std::time::Duration::ZERO)
        .await;
    conformance::assert_read_stream_reports_the_listed_date(&volume, &at("dated.txt")).await;
    conformance::assert_landed_time_agrees_with_get_metadata(&volume, &at("landed.txt")).await;
    conformance::assert_write_from_stream_keeps_metadata(&volume, &at("born.txt")).await;
    conformance::assert_read_stream_reports_the_listed_metadata(&volume, &at("born.txt")).await;
    conformance::assert_apply_metadata_keeps(&volume, &at("born")).await;
}

async fn a_place_keeps_the_read_promises(service: FixtureService) {
    let volume = connect_fixture(service, Some(FIXTURE_BUCKET)).await;
    let prefix = scratch_prefix("conformance");
    let content: &[u8] = b"the bytes the export assertion compares against";
    seeded(service, &prefix, &[("held.txt", content)]).await;

    conformance::assert_export_matches_the_bytes_offered(
        &volume,
        &conformance_at(&volume, &format!("{prefix}held.txt")),
        content,
    )
    .await;
    conformance::assert_not_found_carries_the_path(&volume, &conformance_at(&volume, &format!("{prefix}missing.txt")))
        .await;
    // A folder holding the object: its listing and the object's HEAD may
    // carry a creation date (`supported_dates` claims it), never an added one.
    let folder = conformance_at(&volume, prefix.trim_end_matches('/'));
    conformance::assert_listing_dates_are_claimed(&volume, &folder).await;
}

async fn a_copy_scan_stops_when_told_and_asks_inside_the_walk(service: FixtureService) {
    let volume = connect_fixture(service, Some(FIXTURE_BUCKET)).await;
    let prefix = scratch_prefix("conformance-scan");
    let names = ["a.txt", "b.txt", "inner/c.txt", "inner/deeper/d.txt", "other/e.txt"];
    let files: Vec<(String, &[u8])> = names
        .iter()
        .map(|name| (format!("tree/{name}"), b"scan me".as_slice()))
        .collect();
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (name.as_str(), *bytes)).collect();
    seeded(service, &prefix, &refs).await;
    let tree = conformance_at(&volume, &format!("{prefix}tree"));

    conformance::assert_batch_scan_stops_when_told(&volume, &tree).await;
    // Five files and three folders under the top.
    conformance::assert_batch_scan_asks_inside_the_walk(&volume, &tree, 8).await;
}

/// One `#[tokio::test]` per fixture for each cell, `#[ignore]`d for the shared
/// fixture lane.
macro_rules! on_both_fixtures {
    ($($cell:ident => $versitygw:ident, $garage:ident;)*) => {$(
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "needs the S3 fixture stack: apps/desktop/test/s3-servers/start.sh (s3-fixture)"]
        async fn $versitygw() {
            $cell(VERSITYGW).await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "needs the S3 fixture stack: apps/desktop/test/s3-servers/start.sh (s3-fixture)"]
        async fn $garage() {
            $cell(GARAGE).await;
        }
    )*};
}

on_both_fixtures! {
    a_place_keeps_the_write_promises
        => a_place_keeps_the_write_promises_on_versitygw, a_place_keeps_the_write_promises_on_garage;
    a_place_keeps_the_read_promises
        => a_place_keeps_the_read_promises_on_versitygw, a_place_keeps_the_read_promises_on_garage;
    a_copy_scan_stops_when_told_and_asks_inside_the_walk
        => a_copy_scan_stops_when_told_and_asks_inside_the_walk_on_versitygw,
           a_copy_scan_stops_when_told_and_asks_inside_the_walk_on_garage;
}
