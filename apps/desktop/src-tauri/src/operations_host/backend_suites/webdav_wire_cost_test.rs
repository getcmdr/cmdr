//! What one upload costs a WebDAV server, counted on the wire.
//!
//! The transfer engine stages every write itself: the bytes go to a
//! `.cmdr-tmp-*` sibling and a rename (`MOVE`) gives them their name. The
//! backend's `write_from_stream` is a plain `PUT` to whatever path it's handed,
//! so a file costs ONE `PUT` and ONE `MOVE`. A backend that staged again under
//! the engine's temp would cost a second `MOVE` per file and put a second temp
//! in the user's folder; these cells count the requests so that can't come
//! back unnoticed. `crates/cmdr-webdav/DETAILS.md` § "Write staging" has the
//! decision.
//!
//! The counts come from the proxy in `webdav_refusing_proxy_test_support.rs`,
//! refusing nothing here: it sees every request, one per connection.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::volume::Volume;
use cmdr_webdav::volume::testing::scratch_dir;

use super::network_semantics_test_support::{local_volume, seed, try_read};
use super::network_transfer_test_support::{assert_no_staging_litter, clean_deep};
use super::webdav_refusing_proxy_test_support::{Refusal, RefusingProxy, through_a_refusing_proxy};
use super::webdav_test_support::{WebdavFixture, connect};
use crate::operations_host::app_engine;
use cmdr_operations::testing::new_operation_state;
use cmdr_operations::testing::{CollectorEventSink, copy_volumes_with_progress};
use cmdr_operations::{ConflictResolution, VolumeCopyConfig};

/// A live volume behind a proxy that only counts, and a scratch directory on it.
async fn counted() -> (RefusingProxy, Arc<dyn Volume>, PathBuf) {
    let (proxy, volume) = through_a_refusing_proxy(Refusal::NOTHING).await;
    let dir = scratch_dir(&volume).await;
    (proxy, Arc::new(volume), dir)
}

/// Copies `sources` from `local` into `dir` on the server under `policy`, and
/// insists it succeeded.
async fn copy_up(
    local: Arc<dyn Volume>,
    sources: &[&str],
    remote: &Arc<dyn Volume>,
    dir: &Path,
    policy: ConflictResolution,
    label: &str,
) {
    let result = copy_volumes_with_progress(
        Arc::new(CollectorEventSink::new()),
        label,
        &Arc::new(new_operation_state(app_engine(), Duration::from_millis(200))),
        local,
        &sources.iter().map(PathBuf::from).collect::<Vec<_>>(),
        Arc::clone(remote),
        dir,
        &VolumeCopyConfig {
            conflict_resolution: policy,
            ..VolumeCopyConfig::default()
        },
    )
    .await;
    assert!(result.is_ok(), "{label}: {:?}", result.err().map(|f| f.error));
}

/// Removes a cell's scratch directory through a direct connection, found again
/// by name (the proxied volume's paths carry the proxy's port).
async fn clean_up_past_the_proxy(dir: &Path) {
    let direct = connect(WebdavFixture::Stock).await;
    let name = dir.file_name().expect("a scratch dir has a name");
    clean_deep(&direct, &direct.root().as_std_legacy().join(name)).await;
}

/// Three new files in a folder: one `PUT` and one `MOVE` each.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn webdav_integration_a_new_file_costs_one_put_and_one_move() {
    let (proxy, remote, dir) = counted().await;
    let files: [(&str, &[u8]); 3] = [("a.txt", b"SRC-a"), ("b.txt", b"SRC-b"), ("c.txt", b"SRC-c")];
    let (_local_dir, local) = local_volume("wire-cost-new");
    seed(local.as_ref(), Path::new("album"), &files).await;

    copy_up(
        local,
        &["album"],
        &remote,
        &dir,
        ConflictResolution::Skip,
        "wire-cost-new",
    )
    .await;

    for (name, bytes) in files {
        assert_eq!(
            try_read(remote.as_ref(), &dir.join("album").join(name))
                .await
                .as_deref(),
            Some(bytes),
            "the copy of {name} must land whole"
        );
    }
    assert_no_staging_litter(remote.as_ref(), &dir.join("album"), "a finished upload").await;
    assert_eq!(proxy.requests("PUT"), 3, "one PUT per file");
    assert_eq!(
        proxy.requests("MOVE"),
        3,
        "❗ one MOVE per file: the engine's landing. A second means the backend staged under the engine's temp again"
    );
    clean_up_past_the_proxy(&dir).await;
}

/// An Overwrite: the upload rides the conflict layer's safe-replace temp, then
/// the original moves aside, the temp takes its name, and the aside goes
/// (WebDAV has no one-step replace, so the swap keeps the original whole until
/// the new bytes are at its name). One `PUT`, two `MOVE`s, and one `DELETE`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the WebDAV fixture stack: apps/desktop/test/webdav-servers/start.sh (webdav-fixture)"]
async fn webdav_integration_an_overwrite_costs_one_put_two_moves_and_one_delete() {
    let (proxy, remote, dir) = counted().await;
    seed(remote.as_ref(), &dir, &[("report.txt", b"DEST-old-report")]).await;
    let (_local_dir, local) = local_volume("wire-cost-overwrite");
    seed(
        local.as_ref(),
        Path::new(""),
        &[("report.txt", b"SRC-new-report, longer")],
    )
    .await;
    // The seed went through the proxy too; count from here.
    let (puts, moves, deletes) = (proxy.requests("PUT"), proxy.requests("MOVE"), proxy.requests("DELETE"));

    copy_up(
        local,
        &["report.txt"],
        &remote,
        &dir,
        ConflictResolution::Overwrite,
        "wire-cost-overwrite",
    )
    .await;

    assert_eq!(
        try_read(remote.as_ref(), &dir.join("report.txt")).await.as_deref(),
        Some(b"SRC-new-report, longer".as_slice())
    );
    assert_no_staging_litter(remote.as_ref(), &dir, "a finished overwrite").await;
    assert_eq!(proxy.requests("PUT") - puts, 1, "one PUT");
    assert_eq!(
        proxy.requests("MOVE") - moves,
        2,
        "❗ two MOVEs: the original aside, the temp onto its name. A third means the backend staged under the engine's temp again"
    );
    assert_eq!(
        proxy.requests("DELETE") - deletes,
        1,
        "one DELETE: the original's aside, once the new bytes are at its name"
    );
    clean_up_past_the_proxy(&dir).await;
}
