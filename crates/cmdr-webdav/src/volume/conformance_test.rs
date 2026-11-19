//! The shared `Volume` conformance promises, against a real Apache `mod_dav`.
//!
//! ❗ **The server here is `webdav-fixture-apache`, and that is the point.** A
//! WebDAV `MOVE` defaults to `Overwrite: T`, and a `DELETE` on a collection
//! removes the whole tree, so wiring `Volume::rename` or `Volume::delete`
//! straight to the verb would silently clobber here and hand every conflict
//! prompt in the app a destroyed file instead of a question.
//!
//! These live apart from the rest of the Docker suite because they assert
//! something different in kind: not what this backend does, but that it keeps the
//! contracts `volume::conformance` holds every backend to. WebDAV has no
//! in-process double, and the answers that matter are the server's.

use cmdr_fs::app_path::{AppPath, AppPathBuf};

use cmdr_fs::volume::Volume;
use cmdr_fs::volume::conformance;

use super::WebdavVolume;
use super::testing::*;

/// `path` as the `std::path` value `cmdr_fs::volume::conformance`'s assertions still take
/// (also `nextcloud_test.rs`'s). The crate's one bridge: it goes when
/// `cmdr_fs::volume::conformance` takes app paths.
pub(super) fn conformance_path(path: &AppPath) -> std::path::PathBuf {
    path.as_std_legacy().into_owned()
}

const FIXTURE: &str = "webdav-servers/start.sh (webdav-fixture)";

/// The stock server, plus a scratch directory of this cell's own.
///
/// Every cell in this binary shares one export, so a fixed directory name would
/// have two of them renaming each other's files.
async fn stock_server_with_scratch() -> (WebdavVolume, AppPathBuf) {
    let volume = connect_fixture("APACHE", 13480).await;
    let dir = scratch_dir_at(&volume).await;
    (volume, dir)
}

/// Removes everything a cell built, deepest first, and the scratch dir itself.
async fn clean(volume: &WebdavVolume, dir: &AppPath) {
    remove_recursively(volume, dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn a_forceless_rename_refuses_an_existing_destination() {
    // ❗ THE data-safety cell of the write path. `MOVE` without `Overwrite: F`
    // replaces the destination on every server that speaks the protocol.
    let (volume, dir) = stock_server_with_scratch().await;
    let source = child(&dir, "source.txt");
    let target = child(&dir, "target.txt");
    volume.create_file(&source, b"source").await.expect(FIXTURE);
    volume
        .create_file(&target, b"the user's target file")
        .await
        .expect(FIXTURE);

    conformance::assert_rename_refuses_an_existing_destination(
        &volume,
        &conformance_path(&source),
        &conformance_path(&target),
    )
    .await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn a_forced_rename_keeps_a_non_empty_folder() {
    // ❗ `MOVE` with `Overwrite: T` deletes a destination collection, tree and
    // all, before it moves anything (RFC 4918 § 9.9.3).
    let (volume, dir) = stock_server_with_scratch().await;
    let source = child(&dir, "source.txt");
    let source_dir = child(&dir, "source-dir");
    let album = child(&dir, "album");
    volume.create_file(&source, b"source").await.expect(FIXTURE);
    volume.create_directory(&source_dir).await.expect(FIXTURE);
    volume.create_directory(&album).await.expect(FIXTURE);
    volume
        .create_file(&child(&album, "source.txt"), b"the user's photo")
        .await
        .expect(FIXTURE);

    for from in [&source, &source_dir] {
        conformance::assert_forced_rename_keeps_a_non_empty_dir(
            &volume,
            &conformance_path(from),
            &conformance_path(&album),
        )
        .await;
    }

    clean(&volume, &dir).await;
}

/// The shared dates claim: Apache answers a `creationdate`, which
/// `supported_dates` claims, and WebDAV has no added date.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn listing_dates_honor_the_shared_claim_contract() {
    let (volume, dir) = stock_server_with_scratch().await;
    volume
        .create_file(&child(&dir, "dated.txt"), b"bytes")
        .await
        .expect(FIXTURE);
    volume.create_directory(&child(&dir, "folder")).await.expect(FIXTURE);

    conformance::assert_listing_dates_are_claimed(&volume, &conformance_path(&dir)).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn create_file_refuses_to_clobber() {
    // `PUT` overwrites by default; the refusal has to come from `If-None-Match: *`
    // or a probe, so this is the cell that notices either going missing.
    let (volume, dir) = stock_server_with_scratch().await;
    let notes = child(&dir, "notes.txt");
    volume.create_file(&notes, b"the user's notes").await.expect(FIXTURE);

    conformance::assert_create_file_refuses_to_clobber(&volume, &conformance_path(&notes), b"new").await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn write_from_stream_create_new_refuses_to_clobber() {
    // The upload lands by MOVE, so the refusal is `Overwrite: F`'s 412.
    let (volume, dir) = stock_server_with_scratch().await;
    let notes = child(&dir, "notes.txt");
    let fresh = child(&dir, "fresh.txt");
    volume.create_file(&notes, b"the user's notes").await.expect(FIXTURE);

    conformance::assert_write_from_stream_create_new_refuses_to_clobber(
        &volume,
        &conformance_path(&notes),
        &conformance_path(&fresh),
        b"new",
    )
    .await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn unknown_write_is_refused_before_io() {
    let (volume, dir) = stock_server_with_scratch().await;
    let notes = child(&dir, "notes.txt");
    let original = b"the user's notes";
    volume.create_file(&notes, original).await.expect(FIXTURE);

    conformance::assert_unknown_write_is_refused_before_io(&volume, &conformance_path(&notes), original).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn create_directory_all_reports_an_existing_directory_honestly() {
    // ❗ `Created` is a promise the transfer driver SPENDS: on it, it skips the
    // per-file destination conflict probe for everything it writes inside.
    // `MKCOL` on an existing collection answers 405, and the honesty here rests
    // on the mapper reading that as "already there" rather than as a refusal.
    let (volume, dir) = stock_server_with_scratch().await;
    let album = child(&dir, "album");
    volume.create_directory(&album).await.expect(FIXTURE);

    conformance::assert_create_directory_all_reports_an_existing_dir_honestly(&volume, &conformance_path(&album)).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn create_directory_all_refuses_a_file_in_the_way() {
    // `MKCOL` answers 405 for ANY occupied name, file or collection, and 409
    // for a path under a file. Read as "already there" and "parent missing",
    // those walk straight past the file and report the folder the user asked
    // Cmdr to create as not found.
    let (volume, dir) = stock_server_with_scratch().await;
    let notes = child(&dir, "notes");
    volume.create_file(&notes, b"the user's notes").await.expect(FIXTURE);

    conformance::assert_create_directory_all_refuses_a_file_in_the_way(&volume, &conformance_path(&notes)).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn delete_leaves_a_non_empty_directory_intact() {
    // ❗ `DELETE` on a collection is recursive by protocol (`Depth: infinity` is
    // the only depth it accepts), so this refusal is entirely the backend's.
    let (volume, dir) = stock_server_with_scratch().await;
    let album = child(&dir, "album");
    volume.create_directory(&album).await.expect(FIXTURE);
    volume
        .create_file(&child(&album, "keep.txt"), b"content")
        .await
        .expect(FIXTURE);

    conformance::assert_delete_leaves_a_non_empty_dir_intact(&volume, &conformance_path(&album), "keep.txt").await;

    clean(&volume, &dir).await;
}

/// The source half of the date contract, on the file `seed.sh` dated to 2021.
///
/// ❗ Only the source half runs here. Apache `mod_dav` can't STORE a date (it
/// ignores `X-OC-Mtime`, and `getlastmodified` is a protected property no
/// PROPPATCH may set), so the destination half is pinned on the server that
/// can: `nextcloud_test.rs`'s `nextcloud_a_copy_keeps_the_source_date`. The cell
/// below keeps this server's limit on record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn a_read_stream_reports_the_listed_date() {
    if not_for_your_own_server("the seeded `dated.txt`") {
        return;
    }
    let volume = connect_fixture("APACHE", 13480).await;

    conformance::assert_read_stream_reports_the_listed_date(
        &volume,
        &conformance_path(&at(&volume, FIXTURE_DATED_FILE)),
    )
    .await;
}

/// Apache has no `nc:creation_time`, so its connect probe says it keeps no
/// dates: the caps claim nothing, a copy sends no `X-OC-CTime`, and a folder
/// gets no PROPPATCH (`mod_dav` would store one as dead properties).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn apache_keeps_no_creation_dates_and_says_so() {
    if not_for_your_own_server("plain Apache `mod_dav`") {
        return;
    }
    let (volume, dir) = stock_server_with_scratch().await;
    assert_eq!(volume.metadata_caps(), cmdr_fs::metadata::MetadataCaps::NONE);

    let folder = child(&dir, "born");
    volume.create_directory(&folder).await.expect(FIXTURE);
    let carried = cmdr_fs::metadata::CarriedMetadata {
        created: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(conformance::BIRTH_SECS)),
        ..cmdr_fs::metadata::CarriedMetadata::default()
    };
    let report = volume
        .apply_metadata(&folder, &carried, Some(std::time::SystemTime::now()))
        .await;
    assert!(
        report.holds_none(),
        "no PROPPATCH on a server that keeps no dates; got {report:?}"
    );

    clean(&volume, &dir).await;
}

/// Apache stores no date, so a copy onto it carries the server's own: the
/// documented limit of a plain `mod_dav` destination, ❗ asserted so it stays a
/// fact rather than an assumption.
///
/// If this ever fails, Apache (or the backend) learned to keep a date: switch
/// this server to `conformance::assert_write_from_stream_keeps_the_source_date`
/// and update `crates/cmdr-webdav/DETAILS.md` § "Dates".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn apache_stores_no_date_so_a_copy_onto_it_carries_its_own() {
    if not_for_your_own_server("the seeded `dated.txt` on plain Apache `mod_dav`") {
        return;
    }
    let (volume, dir) = stock_server_with_scratch().await;
    let seeded = at(&volume, FIXTURE_DATED_FILE);
    let copy = child(&dir, "copy.txt");

    let stream = volume.open_read_stream(&seeded).await.expect(FIXTURE);
    assert!(
        stream.modified_at().is_some(),
        "the seed's stream must carry its 2021 date, or this cell proves nothing about the destination"
    );
    let length = stream.total_size();
    volume
        .write_from_stream(&copy, cmdr_fs::volume::WriteMode::CreateNew, length, stream, &|_| {
            std::ops::ControlFlow::Continue(())
        })
        .await
        .expect("a date the server ignores must never fail the copy");

    let listed = volume
        .get_metadata(&copy)
        .await
        .expect(FIXTURE)
        .modified_at
        .expect("Apache lists `getlastmodified` on every file");
    assert_ne!(
        listed,
        conformance::SOURCE_DATE_SECS,
        "Apache kept the source's date: the limit this cell records is gone, so pin the destination half here instead"
    );

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn writability_matches_the_mutations_offered() {
    let (volume, dir) = stock_server_with_scratch().await;
    // ❗ A name that is NOT there yet: the helper creates the directory itself
    // (or watches the create be refused) and removes it again, so handing it the
    // scratch dir trips its own precondition.
    let unborn = child(&dir, "unborn");

    conformance::assert_writability_matches_the_mutations_offered(&volume, &conformance_path(&unborn)).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn export_matches_the_bytes_offered() {
    // ❗ The cell that says this backend can be COPIED FROM at all:
    // `copy_between_volumes` refuses a source answering `supports_export() ==
    // false` before it reads a byte, and logs nothing on the way out.
    let (volume, dir) = stock_server_with_scratch().await;
    let file = child(&dir, "exported.txt");
    let content = b"the bytes a copy would move";
    volume.create_file(&file, content).await.expect(FIXTURE);

    conformance::assert_export_matches_the_bytes_offered(&volume, &conformance_path(&file), content).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn copy_within_matches_what_is_offered() {
    let (volume, dir) = stock_server_with_scratch().await;
    let from = child(&dir, "original.txt");
    volume.create_file(&from, b"copied on the server").await.expect(FIXTURE);

    conformance::assert_copy_within_matches_what_is_offered(
        &volume,
        &conformance_path(&from),
        &conformance_path(&child(&dir, "duplicate.txt")),
    )
    .await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn not_found_carries_the_path() {
    // A 404 carries Apache's own HTML page and no path, and that is what the
    // frontend would render as the missing file's name unless the mapper puts
    // the path there.
    let (volume, dir) = stock_server_with_scratch().await;

    conformance::assert_not_found_carries_the_path(&volume, &conformance_path(&child(&dir, "no-such-file.txt"))).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn conflict_scan_reads_a_missing_destination_as_empty() {
    // A `PROPFIND` on a collection nobody has created yet answers 404, and the
    // walk's `scan_conflicts` is what turns it into "nothing clashes" rather
    // than into a copy preview that won't open.
    let (volume, dir) = stock_server_with_scratch().await;

    conformance::assert_conflict_scan_reads_a_missing_destination_as_empty(
        &volume,
        &conformance_path(&child(&dir, "not-created-yet")),
    )
    .await;

    clean(&volume, &dir).await;
}

/// The shared stop assertions, against a real Apache `mod_dav`.
///
/// ❗ Per entry: this backend's scan is `scan_walk`'s, so the boundary comes with
/// it. One `PROPFIND` per directory over a WAN link is the cost a Cancel has to
/// land inside of.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn a_batch_scan_stops_when_it_is_told_to() {
    let (volume, dir) = stock_server_with_scratch().await;
    volume.create_file(&child(&dir, "a.txt"), b"a").await.expect(FIXTURE);

    conformance::assert_batch_scan_stops_when_told(&volume, &conformance_path(&dir)).await;

    clean(&volume, &dir).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn a_batch_scan_asks_its_boundary_inside_the_walk() {
    let (volume, dir) = stock_server_with_scratch().await;
    volume.create_file(&child(&dir, "a.txt"), b"a").await.expect(FIXTURE);
    volume.create_file(&child(&dir, "b.txt"), b"bb").await.expect(FIXTURE);

    conformance::assert_batch_scan_asks_inside_the_walk(&volume, &conformance_path(&dir), 3).await;

    clean(&volume, &dir).await;
}
