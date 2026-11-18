//! The read side against `fake_nfs.rs`: listings over both READDIR shapes, the
//! handle cache's two rules, the read window under reordering and short
//! answers, links, and a silent server on a paused clock.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::path::Path;

use std::sync::Mutex;
use std::time::Duration;

use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{EntryKind, ListingProgress, SignInShape, SpaceInfo, Volume, VolumeError, WriteAccess};
use tokio_util::sync::CancellationToken;

use super::fake_nfs::{FAKE_EXPORT, FakeNfs, proc};
use super::{Dialer, NfsVolume, connect_with};
use crate::dial::DialOptions;

fn host() -> VolumeHost {
    VolumeHost::builder().runtime(tokio::runtime::Handle::current()).build()
}

/// A volume on `fake` through the real dial over loopback TCP.
async fn over_tcp(fake: &FakeNfs) -> NfsVolume {
    connect_with(
        "fake",
        fake.params(),
        host(),
        Dialer::over_tcp(DialOptions::PRODUCTION),
        CancellationToken::new(),
    )
    .await
    .expect("the fake mounts")
}

/// A volume on `fake` over an in-memory pipe: exact on a paused clock.
async fn over_pipe(fake: &FakeNfs) -> NfsVolume {
    connect_with(
        "fake",
        fake.params(),
        host(),
        fake.duplex_dialer(DialOptions::PRODUCTION),
        CancellationToken::new(),
    )
    .await
    .expect("the fake mounts")
}

/// Every 16-byte line holds its own number, so any offset says where it
/// belongs (the fixture's `large.bin` shape).
fn numbered(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut line = 0u64;
    while out.len() < len {
        out.extend_from_slice(format!("{line:015}\n").as_bytes());
        line += 1;
    }
    out.truncate(len);
    out
}

/// `remote` as the app addresses it on `volume`: a bare `/photos` is refused,
/// the way a pane never sends one.
fn at(volume: &NfsVolume, remote: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}{remote}", volume.root().as_std_legacy().display()))
}

fn names(entries: &[cmdr_fs::entry::FileEntry]) -> Vec<String> {
    let mut names: Vec<String> = entries.iter().map(|entry| entry.name.clone()).collect();
    names.sort();
    names
}

fn seeded() -> FakeNfs {
    let fake = FakeNfs::new();
    fake.add_dir("/photos");
    fake.add_dir("/photos/2024");
    fake.add_file("/photos/2024/beach.txt", b"sand");
    for index in 0..10 {
        fake.add_file(&format!("/file-{index:02}.txt"), &[b'x'; 10]);
    }
    fake.add_file_named("/", b"caf\xE9.txt", b"latin-1");
    fake.add_link("/link-inside", "photos/2024/beach.txt");
    fake.add_link("/link-outside", "/etc/hostname");
    fake.add_link("/photo-dir", "photos");
    fake
}

async fn read_all(volume: &NfsVolume, path: &str) -> Vec<u8> {
    let mut stream = volume
        .open_read_stream(&AppPath::legacy_unchecked(&at(volume, path)))
        .await
        .expect("the file opens");
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        out.extend_from_slice(&chunk.expect("a chunk"));
    }
    out
}

/// The shared dates claim, in the default lane: `fattr3` has no creation date,
/// and the default `supported_dates` claims none.
#[tokio::test]
async fn listing_dates_honor_the_shared_claim_contract() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;

    cmdr_fs::volume::conformance::assert_listing_dates_are_claimed(&volume, &at(&volume, "/photos")).await;
    cmdr_fs::volume::conformance::assert_listing_dates_are_claimed(&volume, &at(&volume, "/photos/2024")).await;
}

#[tokio::test]
async fn a_listing_spans_pages_reports_each_batch_and_leaves_out_what_it_cant_name() {
    let fake = FakeNfs::start_with(seeded()).await;
    fake.set_page_entries(4);
    let volume = over_tcp(&fake).await;
    let reports = Mutex::new(Vec::<ListingProgress>::new());
    let on_progress = |progress: ListingProgress| reports.lock().unwrap().push(progress);

    let entries = volume
        .list_directory(app_path!("/"), Some(&on_progress))
        .await
        .expect("the root lists");

    let mut expected: Vec<String> = (0..10).map(|index| format!("file-{index:02}.txt")).collect();
    expected.extend(["link-inside", "link-outside", "photo-dir", "photos"].map(str::to_string));
    expected.sort();
    assert_eq!(names(&entries), expected, "no `.`/`..`, no non-UTF-8 name");
    let reports = reports.into_inner().unwrap();
    assert!(
        reports.len() >= 3,
        "one report per page, ❌ not one at the end: {reports:?}"
    );
    assert_eq!(reports.last().unwrap().entries(), 14);
    let photos = entries.iter().find(|entry| entry.name == "photos").unwrap();
    assert!(photos.is_directory);
    assert_eq!(photos.path, format!("{}/photos", volume.params().app_root()));
    assert!(
        entries
            .iter()
            .find(|entry| entry.name == "link-inside")
            .unwrap()
            .is_symlink
    );
}

#[tokio::test]
async fn a_server_without_readdirplus_lists_the_same_through_readdir_and_lookup() {
    let fake = FakeNfs::start_with(seeded()).await;
    fake.refuse_readdirplus(true);
    let volume = over_tcp(&fake).await;

    let entries = volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos")), None)
        .await
        .unwrap();

    assert_eq!(names(&entries), vec!["2024"]);
    assert!(fake.calls(proc::READDIR) >= 1);
    assert!(fake.calls(proc::LOOKUP) >= 2, "a LOOKUP per entry on the fallback");
}

#[tokio::test]
async fn a_deep_folder_costs_no_lookup_once_its_parent_was_listed() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos")), None)
        .await
        .unwrap();
    let lookups = fake.calls(proc::LOOKUP);

    volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos/2024")), None)
        .await
        .unwrap();
    volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos/2024")), None)
        .await
        .unwrap();

    assert_eq!(fake.calls(proc::LOOKUP), lookups, "both levels came from the cache");
}

#[tokio::test]
async fn a_stale_cached_handle_is_looked_up_once_more_and_the_listing_lands() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos/2024")), None)
        .await
        .unwrap();
    let lookups = fake.calls(proc::LOOKUP);

    fake.make_stale("/photos/2024");
    let entries = volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos/2024")), None)
        .await
        .unwrap();

    assert_eq!(names(&entries), vec!["beach.txt"]);
    assert_eq!(
        fake.calls(proc::LOOKUP),
        lookups + 1,
        "one fresh LOOKUP of the stale level, no more"
    );
}

#[tokio::test]
async fn a_file_streams_byte_exact_through_reordered_and_short_answers() {
    let fake = FakeNfs::new();
    let data = numbered(300 * 1024 + 7);
    fake.add_file("/big.bin", &data);
    fake.set_transfer_size(16 * 1024);
    let fake = FakeNfs::start_with(fake).await;
    let volume = over_tcp(&fake).await;

    fake.reorder_reads(true);
    assert!(
        read_all(&volume, "/big.bin").await == data,
        "reordered READs reassemble in file order"
    );

    fake.reorder_reads(false);
    fake.answer_reads_short(5000);
    assert!(
        read_all(&volume, "/big.bin").await == data,
        "a short READ mid-file is re-asked, ❌ not the end"
    );
}

#[tokio::test]
async fn a_stream_declares_its_length_date_and_mode_before_the_first_byte() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    let stream = volume
        .open_read_stream(&AppPath::legacy_unchecked(&at(&volume, "/photos/2024/beach.txt")))
        .await
        .unwrap();
    assert_eq!(stream.total_size(), cmdr_fs::volume::StreamLength::Known(4));
    assert!(stream.modified_at().is_some());

    let empty = FakeNfs::new();
    empty.add_file("/zero", b"");
    let empty = FakeNfs::start_with(empty).await;
    let volume = over_tcp(&empty).await;
    assert_eq!(read_all(&volume, "/zero").await, Vec::<u8>::new());
    assert!(matches!(
        volume.open_read_stream(app_path!("/")).await.err(),
        Some(VolumeError::IsADirectory(_))
    ));
}

#[tokio::test]
async fn read_range_answers_exactly_the_window_and_short_only_at_the_end() {
    let fake = FakeNfs::new();
    let data = numbered(200_000);
    fake.add_file("/big.bin", &data);
    fake.set_transfer_size(8 * 1024);
    let fake = FakeNfs::start_with(fake).await;
    let volume = over_tcp(&fake).await;

    for (offset, len) in [(0usize, 100usize), (65_531, 70_000), (199_990, 100)] {
        let read = volume
            .read_range(&AppPath::legacy_unchecked(&at(&volume, "/big.bin")), offset as u64, len)
            .await
            .unwrap();
        let end = (offset + len).min(data.len());
        assert!(read == data[offset..end], "read_range({offset}, {len})");
    }
    assert!(
        volume
            .read_range(&AppPath::legacy_unchecked(&at(&volume, "/big.bin")), 300_000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_link_inside_the_export_is_followed_and_one_outside_it_is_only_a_link() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;

    assert_eq!(read_all(&volume, "/link-inside").await, b"sand");
    let inside = volume
        .get_metadata(&AppPath::legacy_unchecked(&at(&volume, "/link-inside")))
        .await
        .unwrap();
    assert_eq!(
        inside.size,
        Some(4),
        "`get_metadata` reads through the link, like `stat`"
    );
    let through = volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photo-dir/2024")), None)
        .await
        .unwrap();
    assert_eq!(
        names(&through),
        vec!["beach.txt"],
        "a folder link in the middle of a path"
    );

    let outside = volume
        .get_metadata(&AppPath::legacy_unchecked(&at(&volume, "/link-outside")))
        .await
        .unwrap();
    assert!(outside.is_symlink, "a link we may not follow answers as itself");
    assert!(matches!(
        volume
            .open_read_stream(&AppPath::legacy_unchecked(&at(&volume, "/link-outside")))
            .await
            .err(),
        Some(VolumeError::NotSupported)
    ));
    assert_eq!(
        volume
            .entry_kind(&AppPath::legacy_unchecked(&at(&volume, "/photo-dir")))
            .await
            .unwrap(),
        EntryKind::Symlink
    );
    assert_eq!(
        volume
            .entry_kind(&AppPath::legacy_unchecked(&at(&volume, "/photos")))
            .await
            .unwrap(),
        EntryKind::Directory
    );
}

#[tokio::test]
async fn a_missing_path_is_not_found_and_carries_the_path() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    match volume
        .get_metadata(&AppPath::legacy_unchecked(&at(&volume, "/photos/nope.txt")))
        .await
    {
        Err(VolumeError::NotFound(path)) => assert_eq!(path, "/photos/nope.txt"),
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert!(!volume.exists(&AppPath::legacy_unchecked(&at(&volume, "/nope"))).await);
    assert!(
        volume
            .exists(&AppPath::legacy_unchecked(&at(&volume, "/link-outside")))
            .await
    );
}

#[tokio::test]
async fn space_comes_from_fsstat_and_write_access_from_access() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    assert_eq!(
        volume.get_space_info().await.unwrap(),
        SpaceInfo::Bounded {
            total_bytes: 1_000_000,
            available_bytes: 300_000,
            used_bytes: 600_000
        }
    );
    assert_eq!(
        volume
            .write_access_at(&AppPath::legacy_unchecked(&at(&volume, "/photos")))
            .await,
        WriteAccess::Writable
    );
}

#[tokio::test]
async fn a_typed_path_under_the_export_resolves_to_the_export_and_a_start_folder() {
    let fake = FakeNfs::start_with(seeded()).await;
    let mut typed = fake.params();
    typed = crate::params::NfsConnectionParams::new(typed.server, "/export/photos/2024", typed.identity);

    let volume = connect_with(
        "fake",
        typed,
        host(),
        Dialer::over_tcp(DialOptions::PRODUCTION),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    let params = volume.params();
    assert_eq!(params.export(), FAKE_EXPORT);
    assert_eq!(params.start_folder.as_deref(), Some("/photos/2024"));
    assert_eq!(volume.volume_id(), params.volume_id());
    assert_eq!(*volume.root().as_std_legacy(), Path::new(&params.app_root()));
    assert_eq!(volume.sign_in_prompt(), SignInShape::Nothing);
}

#[tokio::test]
async fn a_dropped_connection_comes_back_unattended_with_nothing_to_offer() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    let generation = volume.connection_generation();

    volume.simulate_session_loss().await;
    volume.attempt_reconnect().await.expect("no secret needed to redial");

    assert_ne!(volume.connection_generation(), generation);
    assert_eq!(
        volume.connection_state(),
        Some(cmdr_fs::volume::ConnectionState::Direct)
    );
    volume
        .list_directory(&AppPath::legacy_unchecked(&at(&volume, "/photos")), None)
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_silent_server_is_disconnected_inside_the_ladder_and_every_waiting_call_answers() {
    let fake = seeded();
    let volume = over_pipe(&fake).await;
    volume.list_directory(app_path!("/"), None).await.unwrap();
    // Long enough for any watch the warm-up started to see nothing waiting and
    // end, so the ladder below is the one the SESSION starts.
    // allowed-test-sleep: virtual time on a paused clock, so the warm-up's watch ends.
    tokio::time::sleep(Duration::from_secs(30)).await;

    fake.go_silent(true);
    let started = tokio::time::Instant::now();
    let (photos, beach) = (at(&volume, "/photos"), at(&volume, "/photos/2024/beach.txt"));
    let (listing, stat) = tokio::join!(
        volume.list_directory(&AppPath::legacy_unchecked(&photos), None),
        volume.get_metadata(&AppPath::legacy_unchecked(&beach)),
    );

    assert!(
        matches!(listing, Err(VolumeError::DeviceDisconnected(_))),
        "{listing:?}"
    );
    assert!(matches!(stat, Err(VolumeError::DeviceDisconnected(_))), "{stat:?}");
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_secs(14) && waited <= Duration::from_secs(16),
        "5 s quiet, then two 5 s probes: {waited:?}"
    );
    assert_eq!(
        volume.connection_state(),
        Some(cmdr_fs::volume::ConnectionState::Disconnected)
    );
}

#[tokio::test]
async fn a_mutation_finds_a_folder_another_client_renamed_away() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    let client = volume.client().await.unwrap();
    let held = volume.dir_for_mutation(&client, "/photos/2024").await.unwrap();
    let lookups = fake.calls(proc::LOOKUP);

    // Nothing moved: the chain is proven by GETATTRs alone.
    let again = volume.dir_for_mutation(&client, "/photos/2024").await.unwrap();
    assert_eq!(again, held);
    assert_eq!(fake.calls(proc::LOOKUP), lookups, "no LOOKUP while the chain stands");

    // Another client moves the folder: the cached handle still WORKS, and now
    // names `/moved`. The mutation must not act on it.
    fake.add_dir("/elsewhere");
    fake.rename("/photos/2024", "/elsewhere/2024");
    match volume.dir_for_mutation(&client, "/photos/2024").await {
        Err(VolumeError::NotFound(path)) => assert_eq!(path, "/photos/2024"),
        other => panic!("the moved folder must not be acted on: {other:?}"),
    }
}

#[tokio::test]
async fn our_own_change_in_a_folder_keeps_its_children_vouched_for() {
    let fake = FakeNfs::start_with(seeded()).await;
    let volume = over_tcp(&fake).await;
    let client = volume.client().await.unwrap();
    volume.dir_for_mutation(&client, "/photos/2024").await.unwrap();
    let lookups = fake.calls(proc::LOOKUP);

    // A change in `/photos` that we made, reported by its own `wcc_data`.
    let photos = volume
        .resolve(&client, "/photos", super::handles::Follow::Always, false)
        .await
        .unwrap();
    let wcc = client
        .nfs
        .mkdir(&photos.handle, b"new", &crate::proto::nfs3::Sattr3::default())
        .await
        .unwrap()
        .dir_wcc;
    volume.inner.handles.note_dir_changed("/photos", &wcc);

    volume.dir_for_mutation(&client, "/photos/2024").await.unwrap();
    assert_eq!(
        fake.calls(proc::LOOKUP),
        lookups,
        "our own mkdir didn't cost a re-LOOKUP"
    );
}
