//! Tests for `encrypted_file.rs`, in a file of their own so CodeQL's `**/*_test.rs` ignore covers
//! them: the module is Linux-only outside tests, so the macOS-target analysis can't tell its inline
//! `#[cfg(test)]` module from shipped code and reads each test key as a hard-coded password.

use super::*;

#[test]
fn test_encryption_roundtrip() {
    let password = b"a-machine-id".to_vec();
    let data = b"test data for encryption";
    let mut cocoon = Cocoon::new(&password);
    let encrypted = cocoon.wrap(data).expect("wrap should succeed");
    let decrypted = Cocoon::new(&password)
        .unwrap(&encrypted)
        .expect("unwrap should succeed");
    assert_eq!(decrypted, data);
}

/// ❗ **A `credentials.enc` written by cocoon 0.5 must keep opening** after any `cocoon` bump: a
/// format change would make every Linux user's saved passwords unreadable, and the next save
/// would set the file aside as if the machine had changed (#537).
///
/// `testdata/credentials-cocoon-0.5.enc` is a real store file, written once by
/// `EncryptedFileStore::set` on cocoon 0.5.0 (2026-11-17) under the test-only key
/// `test-machine-id-1`. Its header is cocoon's container format version 1 (magic `7f c0 0a`,
/// version `01`, ChaCha20-Poly1305, PBKDF2 at "strong" iterations). ❌ Never regenerate it with a
/// newer cocoon: that would test the new version against itself. If a future cocoon writes a new
/// format version, add a fixture for it beside this one and keep this one.
#[test]
fn a_store_file_from_cocoon_0_5_format_v1_still_opens() {
    let fixture = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/secrets/testdata/credentials-cocoon-0.5.enc"
    ));
    let raw = std::fs::read(&fixture).expect("the fixture is committed");
    assert_eq!(
        &raw[..7],
        &[0x7f, 0xc0, 0x0a, 0x01, 0x01, 0x01, 0x01],
        "the fixture is cocoon container format v1, ChaCha20-Poly1305, PBKDF2 strong"
    );

    let contents = read_store(&fixture, b"test-machine-id-1").expect("cocoon 0.5's file decrypts and parses");

    assert_eq!(contents.0.len(), 1);
    assert_eq!(
        contents.0.get("smb://testnas/photos").map(Vec::as_slice),
        Some(&b"test_user_1\0test_secret_1"[..])
    );
}

#[test]
fn test_store_contents_serde_roundtrip() {
    let mut contents = StoreContents::default();
    contents.0.insert("key1".to_string(), b"hello".to_vec());
    contents.0.insert("key2".to_string(), b"world".to_vec());

    let json = serde_json::to_vec(&contents).unwrap();
    let parsed: StoreContents = serde_json::from_slice(&json).unwrap();

    assert_eq!(parsed.0.get("key1").unwrap(), b"hello");
    assert_eq!(parsed.0.get("key2").unwrap(), b"world");
}

#[test]
fn test_read_store_missing_file() {
    let contents = read_store(&PathBuf::from("/tmp/nonexistent-cmdr-test-file.enc"), b"any").unwrap();
    assert!(contents.0.is_empty());
}

// ── A file this machine can't read (#537) ────────────────────────────

use crate::state_recovery::{StateFile, StateRecovery, testing as recovery};
use crate::test_support::TestDir;

/// `credentials.enc` written under one machine's key, then opened under another's: a home
/// folder restored onto a new install, or a cloned VM.
fn moved_to_a_new_machine(dir: &TestDir) -> EncryptedFileStore {
    EncryptedFileStore::with_password(dir.to_path_buf(), b"old-machine-id".to_vec())
        .set("smb://testnas/photos", b"test\0secret")
        .expect("the old machine saves");
    EncryptedFileStore::with_password(dir.to_path_buf(), b"new-machine-id".to_vec())
}

#[test]
fn a_round_trip_under_this_machines_key() {
    let dir = TestDir::new("encrypted_file_round_trip");
    let store = EncryptedFileStore::new(dir.to_path_buf());
    store.set("ai.apiKey.openai", b"sk-test").expect("set");
    assert_eq!(store.get("ai.apiKey.openai").expect("get"), b"sk-test");
}

#[test]
fn saving_on_a_new_machine_sets_the_old_file_aside_and_starts_a_new_store() {
    let notices = recovery::capture_recoveries();
    let dir = TestDir::new("encrypted_file_new_machine_set");
    let store = moved_to_a_new_machine(&dir);

    store
        .set("smb://testnas/photos", b"test\0typed-again")
        .expect("a save works again");

    assert_eq!(store.get("smb://testnas/photos").expect("get"), b"test\0typed-again");
    let kept = recovery::kept_copies(&store.path);
    assert_eq!(kept.len(), 1, "the unreadable file is kept, not deleted");
    assert_eq!(
        notices.recoveries(),
        vec![StateRecovery {
            file: StateFile::Credentials,
            kept_copy: Some(kept[0].to_string_lossy().into_owned()),
        }]
    );
}

#[test]
fn forgetting_on_a_new_machine_sets_the_old_file_aside_too() {
    let notices = recovery::capture_recoveries();
    let dir = TestDir::new("encrypted_file_new_machine_delete");
    let store = moved_to_a_new_machine(&dir);

    assert!(matches!(
        store.delete("smb://testnas/photos"),
        Err(SecretStoreError::NotFound(_))
    ));

    assert_eq!(recovery::kept_copies(&store.path).len(), 1);
    assert_eq!(notices.recoveries().len(), 1);
    store
        .set("smb://testnas/photos", b"test\0new")
        .expect("and the next save works");
    assert_eq!(notices.recoveries().len(), 1, "told once");
}

#[test]
fn a_file_that_decrypts_but_doesnt_parse_is_set_aside_on_save() {
    let _notices = recovery::capture_recoveries();
    let dir = TestDir::new("encrypted_file_unparseable");
    let store = EncryptedFileStore::with_password(dir.to_path_buf(), b"this-machine".to_vec());
    let garbage = Cocoon::new(b"this-machine").wrap(b"{\"trunc").expect("wrap");
    std::fs::write(&store.path, garbage).expect("write");

    store.set("license.key", b"{}").expect("a save works again");

    assert_eq!(recovery::kept_copies(&store.path).len(), 1);
}

/// ❗ **A machine id that can't be read RIGHT NOW is not a new machine** (#537): falling back
/// to the fixed key made this machine's own file undecryptable, and the first save set it
/// aside, every saved password with it. The save fails instead, and the file stays.
#[test]
fn a_machine_id_that_cant_be_read_right_now_never_sets_the_file_aside() {
    let notices = recovery::capture_recoveries();
    let dir = TestDir::new("encrypted_file_machine_id_unreadable");
    let machine_id = dir.join("machine-id");
    std::fs::write(&machine_id, "this-machine\n").expect("machine id");
    EncryptedFileStore::with_machine_id(dir.to_path_buf(), machine_id.clone())
        .set("smb://testnas/photos", b"test\0secret")
        .expect("saved under this machine's id");

    // A read that fails for a reason other than "there's no such file": EISDIR stands in for
    // the transient EIO or EACCES.
    std::fs::remove_file(&machine_id).expect("remove");
    std::fs::create_dir(&machine_id).expect("a directory where the id was");
    let store = EncryptedFileStore::with_machine_id(dir.to_path_buf(), machine_id.clone());

    assert!(store.set("license.key", b"{}").is_err(), "the save says it didn't save");
    assert!(store.delete("smb://testnas/photos").is_err());
    assert!(recovery::kept_copies(&store.path).is_empty(), "nothing set aside");
    assert!(notices.recoveries().is_empty());

    // Once the id reads again, everything is where it was.
    std::fs::remove_dir(&machine_id).expect("remove");
    std::fs::write(&machine_id, "this-machine\n").expect("machine id");
    assert_eq!(store.get("smb://testnas/photos").expect("get"), b"test\0secret");
}

/// A machine with no machine id at all keys by the fixed fallback, for good: that IS this
/// machine's key, so its file round-trips.
#[test]
fn a_machine_without_a_machine_id_keys_by_the_fallback() {
    let dir = TestDir::new("encrypted_file_machine_id_absent");
    let store = EncryptedFileStore::with_machine_id(dir.to_path_buf(), dir.join("no-machine-id"));
    store.set("ai.apiKey.openai", b"sk-test").expect("set");
    assert_eq!(store.get("ai.apiKey.openai").expect("get"), b"sk-test");
}

#[test]
fn reading_on_a_new_machine_answers_an_error_and_leaves_the_file_alone() {
    // A read decides nothing: it doesn't destroy what it can't read, and it stays quiet.
    let notices = recovery::capture_recoveries();
    let dir = TestDir::new("encrypted_file_new_machine_get");
    let store = moved_to_a_new_machine(&dir);

    assert!(matches!(
        store.get("smb://testnas/photos"),
        Err(SecretStoreError::Other(_))
    ));
    assert!(store.keys().is_err());

    assert!(store.path.exists());
    assert!(recovery::kept_copies(&store.path).is_empty());
    assert!(notices.recoveries().is_empty());
}
