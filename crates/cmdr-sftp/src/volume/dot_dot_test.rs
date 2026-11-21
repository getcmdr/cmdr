//! What `<path>/..` names on an SFTP server: the link target's parent when the
//! last name is a symlink, as the server's own `..` reads it
//! (`docs/specs/app-path-plan.md` § "Typed and pasted `..`"). The Docker cells run
//! against stock OpenSSH; the offline one needs no server.

use std::path::Path;

use cmdr_fs::volume::{Volume, VolumeError};

use super::SftpVolume;
use super::test_support::make_test_volume;
use super::testing::*;

const FIXTURE: &str = "sftp-servers/start.sh (sftp-fixture)";

/// The stock server, a scratch directory, and in it `real/dir`, `x/plain`, and
/// `x/link -> ../real/dir` (relative, stored verbatim).
async fn linked_scratch(what: &str) -> (SftpVolume, String) {
    let params = fixture_params("OPENSSH", 12480);
    let host = fixture_host(&params, Some(FIXTURE_PASSWORD));
    let volume = connect_fixture(&host, params).await;
    let dir = scratch_dir(what);
    clean_scratch_deep(&volume, &dir).await;
    for folder in ["", "/real", "/real/dir", "/x", "/x/plain"] {
        volume
            .create_directory(&app_at(&volume, &format!("{dir}{folder}")))
            .await
            .expect(FIXTURE);
    }
    volume
        .create_symlink(&app_at(&volume, &format!("{dir}/x/link")), Path::new("../real/dir"))
        .await
        .expect(FIXTURE);
    (volume, dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the SFTP fixture stack: sftp-servers/start.sh (sftp-fixture)"]
async fn a_dot_dot_after_a_link_asks_the_server_and_lands_beside_the_target() {
    let (volume, dir) = linked_scratch("dot-dot-link").await;

    let up = volume
        .parent_for_dot_dot(&app_at(&volume, &format!("{dir}/x/link")))
        .await;
    assert_eq!(up.expect(FIXTURE), app_at(&volume, &format!("{dir}/real")));

    let plain = volume
        .parent_for_dot_dot(&app_at(&volume, &format!("{dir}/x/plain")))
        .await;
    assert_eq!(plain.expect(FIXTURE), app_at(&volume, &format!("{dir}/x")));

    // A missing name is no link, so a typed path still reaches its nearest ancestor.
    let missing = volume
        .parent_for_dot_dot(&app_at(&volume, &format!("{dir}/x/nope")))
        .await;
    assert_eq!(missing.expect(FIXTURE), app_at(&volume, &format!("{dir}/x")));

    volume
        .delete(&app_at(&volume, &format!("{dir}/x/link")))
        .await
        .expect(FIXTURE);
    clean_scratch_deep(&volume, &dir).await;
}

/// With no session there's nobody to ask: refused, ❌ never the lexical guess.
#[tokio::test]
async fn a_volume_with_no_session_refuses_rather_than_guessing() {
    let volume = make_test_volume();
    let answer = volume.parent_for_dot_dot(&app_at(&volume, "x/link")).await;
    assert!(
        matches!(
            answer,
            Err(VolumeError::DeviceDisconnected(_) | VolumeError::NotConnected(_))
        ),
        "got {answer:?}"
    );
}
