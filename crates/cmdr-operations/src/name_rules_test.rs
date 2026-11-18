//! `name_rules_at`: the destination's format decides, by backend or by probing
//! the path the host reaches.

use std::path::Path;
use std::sync::Arc;

use cmdr_fs::testing::TestDir;
use cmdr_fs::validation::{HOST_PATH_LIMIT, LengthLimit, NameFormat, NameRules};
use cmdr_fs::volume::{BackendKind, InMemoryVolume, Volume};

use super::name_rules_at;
use crate::testing::TestOperations;

fn register(engine: &TestOperations, id: &str, backend: BackendKind) {
    let volume = InMemoryVolume::new(id).with_backend_kind(backend);
    engine.volumes().register(id, Arc::new(volume) as Arc<dyn Volume>);
}

#[tokio::test]
async fn a_backend_that_serves_its_own_io_answers_by_its_kind() {
    let engine = TestOperations::new();
    for (id, backend, rules) in [
        (
            "share",
            BackendKind::Smb,
            NameRules::smb_share().with_path_limit(Some(LengthLimit::SMB_PATH)),
        ),
        ("phone", BackendKind::Mtp, NameRules::POSIX),
        ("server", BackendKind::Sftp, NameRules::POSIX),
        ("bucket", BackendKind::S3, NameRules::POSIX),
        ("zip", BackendKind::Archive, NameRules::POSIX),
    ] {
        register(&engine, id, backend);
        assert_eq!(
            name_rules_at(engine.host(), id, Path::new("/x")).await,
            rules,
            "{backend:?}"
        );
    }
}

#[tokio::test]
async fn an_unregistered_volume_takes_posix_names() {
    let engine = TestOperations::new();
    assert_eq!(
        name_rules_at(engine.host(), "gone", Path::new("/x")).await,
        NameRules::POSIX
    );
}

/// #1228: a backend that serves its own I/O isn't held to the host's
/// `PATH_MAX`. Only the path API that reaches a path limits it.
#[tokio::test]
async fn only_the_host_path_api_holds_paths_to_the_host_limit() {
    let engine = TestOperations::new();
    register(&engine, "phone", BackendKind::Mtp);
    assert_eq!(
        name_rules_at(engine.host(), "phone", Path::new("/x")).await.path_limit,
        None
    );
    engine.register_local_root();
    let dir = TestDir::new("name_rules_path_limit");
    assert_eq!(
        name_rules_at(engine.host(), "root", &dir).await.path_limit,
        Some(HOST_PATH_LIMIT)
    );
}

/// The local disk is probed at the folder: the scratch dir sits on the boot
/// disk (APFS on macOS, ext4 or similar on Linux), which takes POSIX names.
/// An exFAT stick answers `Windows` through the same probe; that half is
/// pinned by `FilesystemKind::name_rules` and the note's hand-run evidence,
/// since tests may not attach a FAT image (`cmdr_fs::testing::disk_images`).
#[tokio::test]
async fn the_local_disk_answers_for_the_folder_it_holds() {
    let engine = TestOperations::new();
    engine.register_local_root();
    let dir = TestDir::new("name_rules_local");
    let rules = name_rules_at(engine.host(), "root", &dir).await;
    assert_eq!(rules.format, NameFormat::Posix);
}
