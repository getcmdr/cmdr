//! The mutator keeps AppleDouble sidecars in step with their items: an added
//! tagged file or folder brings one (and a plain one doesn't), and a delete or a
//! rename takes the item's sidecar along. macOS only: the sources' xattrs are
//! real, and `ditto -x -k` reads the result back the way Finder would.

use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use super::*;
use crate::apple_double::FINDER_INFO_XATTR;
use crate::test_fixtures::{APPLE_DOUBLE_DITTO_ZIP, build_zip, stored};
use crate::{ArchiveByteSource, ArchiveFormat, ArchiveIndex, BytesSource};

const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";
const TAGS_VALUE: &[u8] = b"bplist00 a Finder tag";
const QUARANTINE: &str = "com.apple.quarantine";

fn names_in(path: &Path) -> Vec<String> {
    let archive = ZipArchive::new(File::open(path).expect("open the zip")).expect("the zip parses");
    let mut names: Vec<String> = archive
        .file_names()
        .map(|name| name.expect("a decodable name").into_owned())
        .collect();
    names.sort();
    names
}

fn macosx_names(path: &Path) -> Vec<String> {
    names_in(path)
        .into_iter()
        .filter(|name| name.starts_with("__MACOSX/"))
        .collect()
}

/// The xattr names the archive at `path` carries for `inner`, read the way an
/// extract reads them.
fn carried_names(path: &Path, inner: &str) -> Vec<String> {
    let source: Arc<dyn ArchiveByteSource> = Arc::new(BytesSource::new(std::fs::read(path).expect("read the zip")));
    let index = ArchiveIndex::parse(Arc::clone(&source), ArchiveFormat::Zip, None).expect("the zip parses");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    runtime
        .block_on(index.item_xattrs(inner, source, None, runtime.handle()))
        .into_iter()
        .map(|xattr| xattr.name.to_string_lossy().into_owned())
        .collect()
}

fn tagged_file(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"tagged").expect("write the source");
    xattr::set(&path, TAGS, TAGS_VALUE).expect("tag the source");
    path
}

fn empty_zip(dir: &Path) -> PathBuf {
    let path = dir.join("out.zip");
    std::fs::write(&path, build_zip(&[stored("keep.txt", "k")])).expect("write the zip");
    path
}

#[derive(Default)]
struct LastProgress(Mutex<Option<MutationProgress>>);

impl MutationHooks for LastProgress {
    fn on_progress(&self, progress: MutationProgress) {
        *self.0.lock().expect("not poisoned") = Some(progress);
    }
}

#[test]
fn a_tagged_file_and_folder_added_bring_sidecars_ditto_reads_back() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tagged_file(tmp.path(), "src.txt");
    let folder = tmp.path().join("srcdir");
    std::fs::create_dir(&folder).expect("make the folder");
    xattr::set(&folder, TAGS, TAGS_VALUE).expect("tag the folder");
    let zip = empty_zip(tmp.path());

    let changeset = Changeset {
        adds: vec![AddEntry {
            inner_path: "docs/tagged.txt".to_string(),
            source: AddSource::LocalPath(file),
        }],
        mkdirs: vec!["docs".to_string()],
        mkdir_sources: HashMap::from([("docs".to_string(), folder)]),
        ..Default::default()
    };
    apply(&zip, &changeset, &NoHooks).expect("the edit applies");

    assert_eq!(
        macosx_names(&zip),
        vec!["__MACOSX/._docs", "__MACOSX/docs/._tagged.txt"]
    );
    assert_eq!(carried_names(&zip, "docs/tagged.txt"), vec![TAGS]);
    assert_eq!(carried_names(&zip, "docs"), vec![TAGS]);

    // What Finder's own extract makes of it.
    let out = tmp.path().join("ditto-out");
    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(&out)
        .status()
        .expect("run ditto");
    assert!(status.success());
    assert_eq!(
        xattr::get(out.join("docs/tagged.txt"), TAGS).expect("read").as_deref(),
        Some(TAGS_VALUE)
    );
    assert_eq!(
        xattr::get(out.join("docs"), TAGS).expect("read").as_deref(),
        Some(TAGS_VALUE)
    );
    assert!(!out.join("__MACOSX").exists());
}

#[test]
fn a_plain_quarantined_or_merely_opened_source_adds_no_sidecar() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plain = tmp.path().join("plain.txt");
    std::fs::write(&plain, b"plain").expect("write");
    let quarantined = tmp.path().join("quarantined.txt");
    std::fs::write(&quarantined, b"q").expect("write");
    xattr::set(&quarantined, QUARANTINE, b"0083;5f000000;Safari;").expect("quarantine it");
    xattr::set(&quarantined, "com.apple.lastuseddate#PS", &[0; 16]).expect("mark it opened");
    let blank_info = tmp.path().join("blank-info.txt");
    std::fs::write(&blank_info, b"b").expect("write");
    xattr::set(&blank_info, FINDER_INFO_XATTR, &[0u8; 32]).expect("set an empty FinderInfo");
    let zip = empty_zip(tmp.path());

    let changeset = Changeset {
        adds: [plain, quarantined, blank_info]
            .into_iter()
            .map(|path| AddEntry {
                inner_path: path.file_name().expect("a name").to_string_lossy().into_owned(),
                source: AddSource::LocalPath(path),
            })
            .collect(),
        ..Default::default()
    };
    apply(&zip, &changeset, &NoHooks).expect("the edit applies");

    assert!(
        macosx_names(&zip).is_empty(),
        "an ordinary zip stays free of `__MACOSX`"
    );
}

#[test]
fn a_sidecar_carries_quarantine_and_last_used_date_along_when_something_else_is_worth_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tagged_file(tmp.path(), "both.txt");
    xattr::set(&file, QUARANTINE, b"0083;5f000000;Safari;").expect("quarantine it");
    xattr::set(&file, "com.apple.lastuseddate#PS", &[0; 16]).expect("mark it opened");
    let zip = empty_zip(tmp.path());
    let changeset = Changeset {
        adds: vec![AddEntry {
            inner_path: "both.txt".to_string(),
            source: AddSource::LocalPath(file),
        }],
        ..Default::default()
    };
    apply(&zip, &changeset, &NoHooks).expect("the edit applies");
    let mut carried = carried_names(&zip, "both.txt");
    carried.sort();
    assert_eq!(carried, vec!["com.apple.lastuseddate#PS", TAGS, QUARANTINE]);
}

#[test]
fn deleting_items_drops_their_sidecars_and_counts_only_the_items() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let zip = tmp.path().join("tagged.zip");
    std::fs::write(&zip, APPLE_DOUBLE_DITTO_ZIP).expect("write the zip");
    let hooks = LastProgress::default();

    let changeset = Changeset {
        deletes: vec!["tagged.txt".to_string(), "tagged-dir".to_string()],
        ..Default::default()
    };
    apply(&zip, &changeset, &hooks).expect("the edit applies");

    assert_eq!(
        macosx_names(&zip),
        vec!["__MACOSX/", "__MACOSX/._plain.txt"],
        "the deleted items' sidecars go with them; the rest stay"
    );
    let progress = hooks.0.lock().expect("not poisoned").expect("progress reported");
    assert_eq!(
        progress.entries_changed, 3,
        "`tagged.txt`, `tagged-dir/`, and `tagged-dir/inner.txt`: never their sidecars"
    );
}

#[test]
fn renaming_items_carries_their_sidecars() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let zip = tmp.path().join("tagged.zip");
    std::fs::write(&zip, APPLE_DOUBLE_DITTO_ZIP).expect("write the zip");

    let changeset = Changeset {
        renames: vec![
            ("tagged.txt".to_string(), "renamed.txt".to_string()),
            ("tagged-dir".to_string(), "moved-dir".to_string()),
        ],
        ..Default::default()
    };
    apply(&zip, &changeset, &NoHooks).expect("the edit applies");

    assert_eq!(carried_names(&zip, "renamed.txt"), vec![TAGS]);
    assert_eq!(carried_names(&zip, "moved-dir"), vec![TAGS]);
    let names = macosx_names(&zip);
    assert!(
        names.contains(&"__MACOSX/moved-dir/._inner.txt".to_string()),
        "{names:?}"
    );
    assert!(!names.iter().any(|name| name.contains("tagged")), "{names:?}");
}

#[test]
fn adding_where_an_orphan_sidecar_waits_replaces_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let zip = tmp.path().join("orphan.zip");
    let stale = crate::test_fixtures::apple_double_sidecar(&[("user.stale", b"1")]);
    std::fs::write(&zip, build_zip(&[stored("__MACOSX/._new.txt", stale)])).expect("write the zip");
    let file = tagged_file(tmp.path(), "new.txt");

    let changeset = Changeset {
        adds: vec![AddEntry {
            inner_path: "new.txt".to_string(),
            source: AddSource::LocalPath(file),
        }],
        ..Default::default()
    };
    apply(&zip, &changeset, &NoHooks).expect("no duplicate entry name");
    assert_eq!(carried_names(&zip, "new.txt"), vec![TAGS]);
}
