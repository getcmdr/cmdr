//! A TOP-LEVEL source's name lands whole, on every cross-volume driver (#1457): the
//! drivers take app paths and split a source's last name typed, so a POSIX server's
//! `a\b.txt` is one name on Windows too, where the `std::path` split landed it as
//! `b.txt`. A drive folder takes no name holding `\`, so there the item is asked about,
//! per item, ❌ never the operation's failure. A deep child's twin is
//! `refused_class_tests.rs`.

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::ignore_poison::IgnorePoison;
use cmdr_fs::volume::{InMemoryVolume, Volume};

use super::super::conflict_responder_test_support::ConflictResponderSink;
use super::{copy_volumes_with_progress, move_volumes_with_progress};
use crate::testing::TestOperations;
use crate::types::{ConflictResolution, VolumeCopyConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Driver {
    SerialCopy,
    ConcurrentCopy,
    CrossVolumeMove,
}

const DRIVERS: [Driver; 3] = [Driver::SerialCopy, Driver::ConcurrentCopy, Driver::CrossVolumeMove];

fn at(text: &str) -> &AppPath {
    AppPath::from_normalized(text).unwrap_or_else(|why| panic!("{text}: {why}"))
}

fn drive(text: &str) -> &AppPath {
    AppPath::from_normalized_as(text, PathFlavor::Windows).unwrap_or_else(|why| panic!("{text}: {why}"))
}

/// A server holding `names` at its root, each file's bytes its own name. The concurrent
/// driver takes three sources or more, so it gets two plain ones beside.
async fn server_with(names: &[&str], driver: Driver) -> (Arc<InMemoryVolume>, Vec<String>) {
    let source = Arc::new(InMemoryVolume::new("Server").with_space_info(10_000_000, 10_000_000));
    let mut sources: Vec<String> = names.iter().map(|name| format!("/{name}")).collect();
    sources.push("/ok.txt".to_string());
    if driver == Driver::ConcurrentCopy {
        sources.push("/zz.txt".to_string());
    }
    for path in &sources {
        source.create_file(at(path), path.as_bytes()).await.unwrap();
    }
    (source, sources)
}

/// Runs `driver` from `source`'s `sources` into `dest_dir` on `dest`, answering every
/// question with Skip, and hands back the questions asked.
async fn transfer(
    driver: Driver,
    source: &Arc<InMemoryVolume>,
    sources: &[String],
    dest: &Arc<InMemoryVolume>,
    dest_dir: &AppPath,
) -> Vec<crate::types::WriteConflictEvent> {
    transfer_answering(driver, source, sources, dest, dest_dir, ConflictResolution::Skip).await
}

/// [`transfer`], answering every question with `answer`.
async fn transfer_answering(
    driver: Driver,
    source: &Arc<InMemoryVolume>,
    sources: &[String],
    dest: &Arc<InMemoryVolume>,
    dest_dir: &AppPath,
    answer: ConflictResolution,
) -> Vec<crate::types::WriteConflictEvent> {
    let engine = TestOperations::new();
    let state = Arc::new(engine.new_operation_state(Duration::from_millis(50)));
    let events = Arc::new(ConflictResponderSink::new(&state, answer, false));
    let config = VolumeCopyConfig::default();
    let (src, dst) = (
        Arc::clone(source) as Arc<dyn Volume>,
        Arc::clone(dest) as Arc<dyn Volume>,
    );
    let sources: Vec<AppPathBuf> = sources.iter().map(|source| at(source).to_owned()).collect();
    let result = match driver {
        Driver::SerialCopy | Driver::ConcurrentCopy => {
            copy_volumes_with_progress(
                Arc::clone(&events) as _,
                "op",
                &state,
                src,
                &sources,
                dst,
                dest_dir,
                &config,
            )
            .await
        }
        Driver::CrossVolumeMove => {
            move_volumes_with_progress(
                Arc::clone(&events) as _,
                "op",
                &state,
                src,
                &sources,
                dst,
                dest_dir,
                &config,
            )
            .await
        }
    };
    result.unwrap_or_else(|f| panic!("{driver:?}: one name must not end the transfer: {:?}", f.error));
    events.inner.conflicts.lock_ignore_poison().clone()
}

async fn read(volume: &InMemoryVolume, path: &AppPath) -> Option<Vec<u8>> {
    let mut stream = volume.open_read_stream(path).await.ok()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next_chunk().await {
        bytes.extend_from_slice(&chunk.ok()?);
    }
    Some(bytes)
}

/// Names a POSIX server lists that std's Windows split reads as more than one name, or
/// that a Windows volume would refuse: each lands whole on a remote destination.
fn hostile_names() -> Vec<&'static str> {
    let mut names = vec!["a:b", "trail.", "trail "];
    // The test double keys its store by `std::path` (`InMemoryVolume`), which on Windows
    // can't hold a source named with a `\`.
    if cfg!(unix) {
        names.extend(["a\\b.txt", "foo\\..", "..\\bar"]);
    }
    names
}

/// ❗ #1457: a top-level `a\b.txt` lands as `a\b.txt` on a POSIX destination, ❌ never as
/// `b.txt` (the `std::path` split on Windows), whatever else holds `b.txt`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_top_level_remote_name_lands_whole_on_a_remote_destination() {
    for driver in DRIVERS {
        let names = hostile_names();
        let (source, sources) = server_with(&names, driver).await;
        let dest = Arc::new(InMemoryVolume::new("Other server").with_space_info(10_000_000, 10_000_000));
        dest.create_directory(at("/inbox")).await.unwrap();

        let prompts = transfer(driver, &source, &sources, &dest, at("/inbox")).await;

        assert!(prompts.is_empty(), "{driver:?}: nothing to ask about: {prompts:?}");
        for source_path in &sources {
            let landed = format!("/inbox{source_path}");
            assert_eq!(
                read(&dest, at(&landed)).await.as_deref(),
                Some(source_path.as_bytes()),
                "{driver:?}: {landed}"
            );
            if driver == Driver::CrossVolumeMove {
                assert!(
                    !source.exists(at(source_path)).await,
                    "a move takes {source_path} along"
                );
            }
        }
        for split in ["/inbox/b.txt", "/inbox/bar", "/inbox/b"] {
            assert!(!dest.exists(at(split)).await, "{driver:?}: {split} is a split name");
        }
    }
}

/// ❗ A drive folder takes no name holding `\` (a POSIX server's `a\b.txt` copied onto
/// NTFS): a TOP-LEVEL source so named is asked about, per item, and its siblings land,
/// ❌ never the operation's failure. The destination is an in-memory volume spelled as a
/// drive, so every host runs it, with no name rules resolved: the typed join's own
/// refusal is what asks. A move keeps the skipped source.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_top_level_backslash_name_a_drive_folder_cant_hold_is_asked_about_per_item() {
    for driver in DRIVERS {
        let refused: &[&str] = if cfg!(unix) {
            &["a\\b.txt", "foo\\..", "..\\bar"]
        } else {
            &[]
        };
        let landing = ["a:b", "trail.", "trail "];
        let names: Vec<&str> = refused.iter().chain(landing.iter()).copied().collect();
        let (source, sources) = server_with(&names, driver).await;
        let dest = Arc::new(InMemoryVolume::new("Disk").with_space_info(10_000_000, 10_000_000));
        dest.create_directory(drive("C:/inbox")).await.unwrap();

        let prompts = transfer(driver, &source, &sources, &dest, drive("C:/inbox")).await;

        assert_eq!(
            prompts.len(),
            refused.len(),
            "{driver:?}: one prompt per refused name: {prompts:?}"
        );
        assert!(
            prompts.iter().all(|p| !p.source_is_directory),
            "{driver:?}: {prompts:?}"
        );
        for source_path in &sources {
            let name = &source_path[1..];
            if refused.contains(&name) {
                if driver == Driver::CrossVolumeMove {
                    assert!(
                        source.exists(at(source_path)).await,
                        "a move keeps a skipped {source_path}"
                    );
                }
                continue;
            }
            let landed = format!("C:/inbox/{name}");
            assert_eq!(
                read(&dest, drive(&landed)).await.as_deref(),
                Some(source_path.as_bytes()),
                "{driver:?}: {landed}"
            );
        }
        for split in ["C:/inbox/b.txt", "C:/inbox/bar"] {
            assert!(!dest.exists(drive(split)).await, "{driver:?}: {split} is a split name");
        }
    }
}

/// ❗ A source that is a server's ROOT lands in a folder named after it, as `main`'s
/// `Path::file_name` named it (`u@h:22`), ❌ never mixed into the destination folder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scheme_root_lands_in_a_folder_named_after_its_server() {
    for driver in [Driver::SerialCopy, Driver::CrossVolumeMove] {
        let source = Arc::new(InMemoryVolume::new("Server").with_space_info(10_000_000, 10_000_000));
        source.create_file(at("sftp://u@h:22/f.txt"), b"f").await.unwrap();
        let dest = Arc::new(InMemoryVolume::new("Other server").with_space_info(10_000_000, 10_000_000));
        dest.create_directory(at("/inbox")).await.unwrap();

        let prompts = transfer(driver, &source, &["sftp://u@h:22/".to_string()], &dest, at("/inbox")).await;

        assert!(prompts.is_empty(), "{driver:?}: {prompts:?}");
        assert_eq!(
            read(&dest, at("/inbox/u@h:22/f.txt")).await.as_deref(),
            Some(&b"f"[..]),
            "{driver:?}"
        );
        assert!(
            !dest.exists(at("/inbox/f.txt")).await,
            "{driver:?}: mixed into the destination"
        );
    }
}

/// ❗ #1470: a Rename of a top-level `a\b.txt` clashing at a remote destination picks
/// `a\b (1).txt` BESIDE it, ❌ never `b (1).txt` inside a folder `a` (Windows' `Path`
/// split), and the user's original stays as it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_of_a_backslash_name_picks_its_new_name_beside_it() {
    for driver in [Driver::SerialCopy, Driver::CrossVolumeMove] {
        let (source, sources) = server_with(&["a\\b.txt"], driver).await;
        let dest = Arc::new(InMemoryVolume::new("Other server").with_space_info(10_000_000, 10_000_000));
        dest.create_directory(at("/inbox")).await.unwrap();
        dest.create_file(at("/inbox/a\\b.txt"), b"the user's own")
            .await
            .unwrap();

        transfer_answering(
            driver,
            &source,
            &sources,
            &dest,
            at("/inbox"),
            ConflictResolution::Rename,
        )
        .await;

        assert_eq!(
            read(&dest, at("/inbox/a\\b (1).txt")).await.as_deref(),
            Some("/a\\b.txt".as_bytes()),
            "{driver:?}"
        );
        assert_eq!(
            read(&dest, at("/inbox/a\\b.txt")).await.as_deref(),
            Some(&b"the user's own"[..]),
            "{driver:?}"
        );
        assert!(
            !dest.exists(at("/inbox/a/b (1).txt")).await,
            "{driver:?}: split into a folder"
        );
    }
}
