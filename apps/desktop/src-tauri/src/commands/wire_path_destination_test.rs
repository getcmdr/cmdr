//! The transfer dialog's destination box: a typed `..` resolves at the command edge, the way
//! `ls` reads it, so the three probes (`path_exists`, `destination_exists`,
//! `destination_write_access`) and the write all name one place (`docs/specs/app-path-plan.md`
//! § "Typed and pasted `..`"). The write's own cell is `write_ops_wire_path_test.rs`.

#![cfg(unix)] // POSIX symlinks.

use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::typed_path::TypedPathRefusal;
use cmdr_fs::volume::{Volume, VolumeError, WriteAccess};
use cmdr_local::LocalVolume;
use cmdr_operations::WriteOperationError;

use super::{DestinationRefusal, destination_over, transfer_destination};
use crate::commands::file_system::{destination_exists, destination_write_access, path_exists};
use crate::file_system::volume::manager::get_volume_manager;
use crate::go_to_path::AppHost;
use crate::test_support::{TestDir, WedgedVolume};

/// `x/link -> ../real/inner`, with `real/dst/name` where the OS's `..` lands and an empty
/// `x` where a lexical `..` would (`x/dst` isn't there).
fn link_tree(name: &str) -> TestDir {
    let dir = TestDir::new(name);
    std::fs::create_dir_all(dir.join("real/inner")).unwrap();
    std::fs::create_dir_all(dir.join("real/dst")).unwrap();
    std::fs::write(dir.join("real/dst/name"), b"there").unwrap();
    std::fs::create_dir_all(dir.join("x")).unwrap();
    std::os::unix::fs::symlink("../real/inner", dir.join("x/link")).unwrap();
    dir
}

fn wire(dir: &TestDir, rel: &str) -> String {
    format!("{}/{rel}", dir.to_string_lossy())
}

/// The boot volume the probes and the resolver ask, as production registers it.
fn ensure_root_volume() {
    get_volume_manager().register_if_absent(
        "root",
        Arc::new(LocalVolume::local_folder("Test root", "/", crate::volume_host::host())),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_probe_reads_a_dot_dot_after_a_folder_link_where_ls_does() {
    ensure_root_volume();
    let dir = link_tree("dest_dot_dot_probes");
    let typed = wire(&dir, "x/link/../dst/name");

    let exists = path_exists(Some("root".into()), typed.clone()).await;
    assert!(exists.data && !exists.timed_out, "`real/dst/name` is there: {exists:?}");
    let exists = destination_exists(Some("root".into()), typed).await;
    assert!(exists.data && !exists.timed_out, "{exists:?}");
    assert_eq!(
        destination_write_access("root".into(), wire(&dir, "x/link/../dst")).await,
        WriteAccess::Writable
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transfer_start_hands_the_engine_the_resolved_place() {
    ensure_root_volume();
    let dir = link_tree("dest_dot_dot_start");
    let real = std::fs::canonicalize(dir.join("real/dst")).unwrap();
    let resolved = transfer_destination("root", wire(&dir, "x/link/../dst")).await.unwrap();
    assert_eq!(
        std::fs::canonicalize(&resolved).unwrap(),
        real,
        "lands on `real/dst`, ❌ not `x/dst`: {resolved}"
    );
    assert!(!resolved.split('/').any(|name| name == ".."), "{resolved}");
}

/// A `..` inside an archive lands inside it, and the probes ask the ARCHIVE about that place,
/// as they do for the same destination typed without the `..`: the disk holding the `.zip`
/// can't see `a.zip/inner/name` and would answer "not there".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dot_dot_inside_an_archive_is_probed_inside_the_archive() {
    use std::io::Write as _;

    ensure_root_volume();
    let dir = TestDir::new("dest_dot_dot_in_archive");
    {
        let file = std::fs::File::create(dir.join("a.zip")).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("sub/", options).unwrap();
        zip.start_file("inner/name", options).unwrap();
        zip.write_all(b"there").unwrap();
        zip.finish().unwrap();
    }

    let plain = path_exists(Some("root".into()), wire(&dir, "a.zip/inner/name")).await;
    assert!(plain.data && !plain.timed_out, "the no-`..` spelling: {plain:?}");
    let typed = path_exists(Some("root".into()), wire(&dir, "a.zip/sub/../inner/name")).await;
    assert!(
        typed.data && !typed.timed_out,
        "the `..` spelling names the same entry: {typed:?}"
    );
}

/// A destination with no `..` crosses byte for byte, as it always did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_destination_with_no_dot_dot_answers_as_before() {
    ensure_root_volume();
    let dir = link_tree("dest_no_dot_dot");
    for sent in [wire(&dir, "x/dst/"), "/photos".to_string(), "~/Downloads".to_string()] {
        assert_eq!(transfer_destination("root", sent.clone()).await.unwrap(), sent);
    }
    let lexical = path_exists(Some("root".into()), wire(&dir, "x/dst")).await;
    assert!(!lexical.data && !lexical.timed_out, "{lexical:?}");
}

/// A hung volume refuses inside the resolver's deadline, ❌ never a lexical guess, and the
/// write refuses as the interrupted connection it is.
#[tokio::test]
async fn a_hung_volume_refuses_a_dot_dot_in_time() {
    let wedged: Arc<dyn Volume> = Arc::new(WedgedVolume::new("Hung share"));
    let asked = Arc::clone(&wedged);
    let host = AppHost::over(move |_| Some(Arc::clone(&asked)), Duration::from_millis(50));
    let started = std::time::Instant::now();
    let refused = destination_over(wedged.as_ref(), "/x/../y", &host).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    let Err(refusal) = refused else {
        panic!("a hung volume answered: {refused:?}");
    };
    assert!(
        matches!(
            refusal,
            DestinationRefusal::Unresolved(TypedPathRefusal::Unanswered(VolumeError::ConnectionTimeout(_)))
        ),
        "{refusal:?}"
    );
    assert!(matches!(
        refusal.into_write_error("/x/../y".into()),
        WriteOperationError::ConnectionInterrupted { .. }
    ));
}
