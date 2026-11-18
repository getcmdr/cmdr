//! AppleDouble sidecars in the index: a Finder-made zip's `__MACOSX` and a macOS
//! tar's `._name` siblings pair with their items, never show, and hand their
//! xattrs to whoever copies the item. Hostile shapes stay hidden or stay plain
//! files, and never escape the sanitizer.

use std::ffi::OsString;
use std::sync::Arc;

use cmdr_fs::metadata::Xattr;
use cmdr_fs::rt::Runtime;

use super::*;
use crate::apple_double;
use crate::test_fixtures::{APPLE_DOUBLE_BSDTAR_TAR_GZ, APPLE_DOUBLE_DITTO_ZIP, build_zip, deflated, stored};

const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";

fn parse(bytes: &[u8], format: ArchiveFormat) -> (ArchiveIndex, Arc<dyn ArchiveByteSource>) {
    let source: Arc<dyn ArchiveByteSource> = Arc::new(BytesSource::new(bytes.to_vec()));
    let index = ArchiveIndex::parse(Arc::clone(&source), format, None).expect("the fixture parses");
    (index, source)
}

fn listed(index: &ArchiveIndex, dir: &str) -> Vec<String> {
    index
        .list(dir)
        .expect("a listable folder")
        .into_iter()
        .map(|node| node.name)
        .collect()
}

async fn xattr_names(index: &ArchiveIndex, inner: &str, source: &Arc<dyn ArchiveByteSource>) -> Vec<String> {
    index
        .item_xattrs(inner, Arc::clone(source), None, &Runtime::current())
        .await
        .into_iter()
        .map(|xattr| xattr.name.to_string_lossy().into_owned())
        .collect()
}

fn sidecar(name: &str, value: &[u8]) -> Vec<u8> {
    apple_double::build(&[Xattr {
        name: OsString::from(name),
        value: value.to_vec(),
    }])
}

#[tokio::test]
async fn a_finder_made_zip_hides_macosx_and_hands_each_item_its_tags() {
    let (index, source) = parse(APPLE_DOUBLE_DITTO_ZIP, ArchiveFormat::Zip);

    assert_eq!(listed(&index, ""), vec!["tagged-dir", "plain.txt", "tagged.txt"]);
    assert_eq!(listed(&index, "tagged-dir"), vec!["inner.txt"]);
    assert!(!index.exists("__MACOSX"));

    assert_eq!(xattr_names(&index, "tagged.txt", &source).await, vec![TAGS]);
    assert_eq!(xattr_names(&index, "tagged-dir", &source).await, vec![TAGS]);
    // Their sidecars hold only provenance, which nobody carries.
    assert!(xattr_names(&index, "plain.txt", &source).await.is_empty());
    assert!(xattr_names(&index, "tagged-dir/inner.txt", &source).await.is_empty());
    assert!(index.has_carried_xattrs("tagged.txt"));
    assert!(!index.has_carried_xattrs("missing.txt"));
}

#[tokio::test]
async fn a_macos_tar_hides_its_dot_underscore_siblings_and_hands_each_item_its_tags() {
    let (index, source) = parse(APPLE_DOUBLE_BSDTAR_TAR_GZ, ArchiveFormat::Tar(TarCodec::Gzip));

    assert_eq!(listed(&index, ""), vec!["tagged-dir", "plain.txt", "tagged.txt"]);
    assert_eq!(listed(&index, "tagged-dir"), vec!["inner.txt"]);
    // The sidecar and the pax record carry the same tags: one copy lands.
    assert_eq!(xattr_names(&index, "tagged.txt", &source).await, vec![TAGS]);
    assert_eq!(xattr_names(&index, "tagged-dir", &source).await, vec![TAGS]);
    assert!(xattr_names(&index, "plain.txt", &source).await.is_empty());
}

#[tokio::test]
async fn the_one_pass_extract_hands_each_tar_member_its_xattrs() {
    let (index, source) = parse(APPLE_DOUBLE_BSDTAR_TAR_GZ, ArchiveFormat::Tar(TarCodec::Gzip));
    let mut reader = index.open_subtree_extract("", source, None, &Runtime::current());
    let mut seen = Vec::new();
    while let Some(member) = reader.next_member().await.expect("next member") {
        let names: Vec<String> = member
            .xattrs
            .iter()
            .map(|xattr| xattr.name.to_string_lossy().into_owned())
            .collect();
        seen.push((member.inner_path, names));
    }
    seen.sort();
    assert_eq!(
        seen,
        vec![
            ("plain.txt".to_string(), vec![]),
            ("tagged-dir/inner.txt".to_string(), vec![]),
            ("tagged.txt".to_string(), vec![TAGS.to_string()]),
        ],
        "the `._` siblings are never members of their own"
    );
}

#[tokio::test]
async fn a_dot_underscore_file_with_no_sibling_is_an_ordinary_file() {
    // In a tar: no `notes` beside `._notes`, so nothing reads it as xattrs and
    // it's the only copy of whatever it holds.
    let mut builder = ::tar::Builder::new(Vec::new());
    for (name, data) in [
        ("._notes", sidecar("user.a", b"1")),
        ("._paired", sidecar("user.b", b"2")),
    ] {
        let mut header = ::tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, data.as_slice()).expect("append");
    }
    let mut header = ::tar::Header::new_ustar();
    header.set_size(1);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "paired", b"x".as_slice())
        .expect("append");
    let (index, source) = parse(
        &builder.into_inner().expect("finish"),
        ArchiveFormat::Tar(TarCodec::Plain),
    );
    assert_eq!(listed(&index, ""), vec!["._notes", "paired"]);
    assert_eq!(xattr_names(&index, "paired", &source).await, vec!["user.b"]);

    // In a zip, only `__MACOSX` holds sidecars: a `._` sibling is a plain file.
    let zip = build_zip(&[stored("a.txt", "a"), stored("._a.txt", sidecar("user.c", b"3"))]);
    let (index, source) = parse(&zip, ArchiveFormat::Zip);
    assert_eq!(listed(&index, ""), vec!["._a.txt", "a.txt"]);
    assert!(xattr_names(&index, "a.txt", &source).await.is_empty());
}

#[tokio::test]
async fn hostile_sidecar_names_never_pair_and_never_escape() {
    let zip = build_zip(&[
        stored("a.txt", "a"),
        stored("__MACOSX/._..", sidecar("user.dotdot", b"1")),
        stored("__MACOSX/._.", sidecar("user.dot", b"1")),
        stored("__MACOSX/._", sidecar("user.empty", b"1")),
        stored("__MACOSX/../._a.txt", sidecar("user.escape", b"1")),
        stored("__MACOSX/nested/not-a-sidecar", "x"),
        stored("__MACOSX/._missing.txt", sidecar("user.orphan", b"1")),
    ]);
    let (index, source) = parse(&zip, ArchiveFormat::Zip);
    assert_eq!(listed(&index, ""), vec!["a.txt"], "__MACOSX shows nothing");
    // `__MACOSX/../._a.txt` never reaches the pairing: the sanitizer
    // quarantines its `..`.
    assert!(xattr_names(&index, "a.txt", &source).await.is_empty());
    assert_eq!(index.quarantined().len(), 1);
    for key in ["..", ".", "", "missing.txt", "__MACOSX/nested/not-a-sidecar"] {
        assert!(xattr_names(&index, key, &source).await.is_empty(), "{key:?}");
    }
}

#[tokio::test]
async fn a_corrupt_or_oversized_sidecar_carries_nothing_and_fails_nothing() {
    let oversized = vec![0u8; usize::try_from(apple_double::MAX_SIDECAR_BYTES).expect("fits") + 1];
    let zip = build_zip(&[
        stored("garbage.txt", "g"),
        stored("__MACOSX/._garbage.txt", "not an AppleDouble at all"),
        stored("big.txt", "b"),
        deflated("__MACOSX/._big.txt", oversized),
    ]);
    let (index, source) = parse(&zip, ArchiveFormat::Zip);
    assert!(xattr_names(&index, "garbage.txt", &source).await.is_empty());
    assert!(xattr_names(&index, "big.txt", &source).await.is_empty());
    assert_eq!(listed(&index, ""), vec!["big.txt", "garbage.txt"]);
}
