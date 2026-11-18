//! `name_rules_at`: the destination's format decides, by backend or by probing
//! the path the host reaches.

use std::path::Path;
use std::sync::Arc;

use cmdr_fs::testing::TestDir;
use cmdr_fs::validation::NameRules;
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
        ("share", BackendKind::Smb, NameRules::SmbShare),
        ("phone", BackendKind::Mtp, NameRules::Posix),
        ("server", BackendKind::Sftp, NameRules::Posix),
        ("bucket", BackendKind::S3, NameRules::Posix),
        ("zip", BackendKind::Archive, NameRules::Posix),
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
        NameRules::Posix
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
    assert_eq!(name_rules_at(engine.host(), "root", &dir).await, NameRules::Posix);
}
