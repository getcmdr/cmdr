//! Names carrying the characters SMB2 forbids on the wire, against a real server.
//!
//! `"`, `*`, `:`, `<`, `>`, `?`, `\`, `|`, and a trailing space or period can't
//! cross the wire as themselves, so every client that carries POSIX names
//! substitutes them into the private-use area at U+F000, and smb2 maps them both
//! ways. The `unicode` fixture container's `public` share holds a set of them in
//! exactly the bytes macOS smbfs writes (its `populate.sh`), so a user who saved
//! `who?.txt` through Finder must see `who?.txt` in a Cmdr pane and be able to
//! open it, and a name Cmdr writes must land in the same bytes Finder would read.
//!
//! The fixture names are seeded by the image, so these cells only read them. The
//! write cell works in a unique directory on the same share and removes it.
//!
//! Every test here is `#[ignore]`d so default runs skip it. Start the containers
//! with `apps/desktop/test/smb-servers/start.sh`, then run
//! `cargo nextest run -E 'package(cmdr-smb)' --run-ignored only`.

use super::test_support::*;
use super::*;
use cmdr_fs::app_path::AppPath;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::{StreamLength, StreamWriteProgress};

/// Each fixture name as a pane shows it, and the file's content.
const FIXTURE_FILES: &[(&str, &[u8])] = &[
    ("who?.txt", b"question mark\n"),
    ("\"quoted\".txt", b"double quote\n"),
    ("wild*card.txt", b"star\n"),
    ("12:30.txt", b"colon\n"),
    ("<tag>.txt", b"angle brackets\n"),
    ("back\\slash.txt", b"backslash\n"),
    ("a|b.txt", b"pipe\n"),
    ("trailing space ", b"trailing space\n"),
    ("trailing period.", b"trailing period\n"),
    ("\"how_are_you_feeling?\"_emojis.json", b"all at once\n"),
];

/// A directory whose own name carries a forbidden character, and the file in it
/// whose name carries another: a path has to be mapped per component.
const FIXTURE_DIR: &str = "we?ird dir";
const FIXTURE_NESTED: (&str, &[u8]) = ("in*ner.txt", b"nested\n");

/// Connects to the `unicode` fixture container's `public` share, which is where
/// its `populate.sh` puts the forbidden-character names.
async fn make_unicode_volume() -> SmbVolume {
    let port = smb2::testing::unicode_port();
    let params = SmbConnectionParams::new("127.0.0.1", "public", port, None, None);
    let volume_id = cmdr_fs::volume::smb_volume_id("127.0.0.1", port, "public");
    connect_smb_volume(
        "public",
        MountAnchor::at_share_root(TEST_MOUNT_ROOT),
        &volume_id,
        params,
        VolumeHost::detached(),
    )
    .await
    .unwrap_or_else(|e| {
        panic!("Failed to connect to the unicode SMB container at 127.0.0.1:{port}. Is it running? ({e:?})")
    })
}

/// Reads a whole file through `open_read_stream`.
async fn read_all(vol: &SmbVolume, path: &AppPath) -> Vec<u8> {
    let stream = vol
        .open_read_stream(path)
        .await
        .unwrap_or_else(|e| panic!("opening {path} for reading: {e:?}"));
    drain(stream).await
}

/// The entry named exactly `name` in `dir`'s listing.
async fn listed(vol: &SmbVolume, dir: &AppPath, name: &str) -> FileEntry {
    let entries = vol
        .list_directory_impl(dir)
        .await
        .unwrap_or_else(|e| panic!("listing {dir}: {e:?}"));
    entries.iter().find(|e| e.name == name).cloned().unwrap_or_else(|| {
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        panic!("{name:?} must be listed in {dir}; got {names:?}")
    })
}

/// What Finder saved as `who?.txt` lists as `who?.txt`, sized, with a path the
/// pane can hand straight back: no private-use code point reaches the UI.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn forbidden_characters_list_as_the_characters_themselves() {
    let vol = make_unicode_volume().await;
    let root = PathBuf::from(TEST_MOUNT_ROOT);
    let root_app = app_at(vol.root(), TEST_MOUNT_ROOT);

    for (name, content) in FIXTURE_FILES {
        let entry = listed(&vol, &root_app, name).await;
        assert!(!entry.is_directory, "{name:?} is a file");
        assert_eq!(entry.size, Some(content.len() as u64), "size of {name:?}");
        assert_eq!(entry.path, root.join(name).to_string_lossy(), "path of {name:?}");
    }
    let dir = listed(&vol, &root_app, FIXTURE_DIR).await;
    assert!(dir.is_directory, "{FIXTURE_DIR:?} is a directory");
    listed(&vol, &app_at(vol.root(), &dir.path), FIXTURE_NESTED.0).await;

    let pua: Vec<String> = vol
        .list_directory_impl(&root_app)
        .await
        .expect("listing the share root")
        .into_iter()
        .map(|e| e.name)
        .filter(|n| n.chars().any(|c| ('\u{F000}'..='\u{F0FF}').contains(&c)))
        .collect();
    assert!(pua.is_empty(), "private-use code points leaked into names: {pua:?}");
}

/// Every listed name opens: metadata and a read both go out mapped, including
/// through a directory whose own name carries one.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn forbidden_character_names_open_and_read() {
    let vol = make_unicode_volume().await;
    let root = app_at(vol.root(), TEST_MOUNT_ROOT);

    for (name, content) in FIXTURE_FILES {
        let path = app_at(vol.root(), &listed(&vol, &root, name).await.path);
        let metadata = vol
            .get_metadata(&path)
            .await
            .unwrap_or_else(|e| panic!("metadata of {name:?}: {e:?}"));
        assert_eq!(metadata.size, Some(content.len() as u64), "metadata size of {name:?}");
        assert_eq!(read_all(&vol, &path).await, *content, "content of {name:?}");
    }

    let dir = app_at(vol.root(), &listed(&vol, &root, FIXTURE_DIR).await.path);
    let nested = app_at(vol.root(), &listed(&vol, &dir, FIXTURE_NESTED.0).await.path);
    assert_eq!(read_all(&vol, &nested).await, FIXTURE_NESTED.1);
}

/// A name Cmdr creates with a forbidden character lists back as typed and reads.
/// The listing cell above pins that the decode matches the bytes Finder writes,
/// so a write that round-trips through it lands in Finder's spelling too.
#[tokio::test]
#[ignore = "Requires Docker SMB containers (./apps/desktop/test/smb-servers/start.sh)"]
async fn a_forbidden_character_name_cmdr_writes_round_trips() {
    const NAME: &str = "why?.txt";
    const PAYLOAD: &[u8] = b"written by Cmdr";
    let vol = make_unicode_volume().await;
    let top = test_dir_name();
    ensure_clean(&vol, &top).await;
    vol.create_directory(&app_at(vol.root(), &top)).await.unwrap();

    let dest = app_at(vol.root(), &format!("{}/{NAME}", share_path(&top)));
    let no_progress = &|_: StreamWriteProgress| std::ops::ControlFlow::Continue(());
    let written = vol
        .write_from_stream(
            &dest,
            cmdr_fs::volume::WriteMode::CreateOrReplace,
            StreamLength::Known(PAYLOAD.len() as u64),
            inline_read_stream(PAYLOAD.to_vec()),
            no_progress,
        )
        .await;
    let listed_back = vol.list_directory_impl(&app_at(vol.root(), &share_path(&top))).await;
    let read_back = match &written {
        Ok(_) => Some(read_all(&vol, &dest).await),
        Err(_) => None,
    };
    ensure_clean(&vol, &top).await;

    written.expect("writing a name with `?` must succeed");
    let names: Vec<String> = listed_back.expect("listing").into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec![NAME.to_string()]);
    assert_eq!(read_back.as_deref(), Some(PAYLOAD));
}
