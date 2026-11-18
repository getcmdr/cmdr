//! The `tauri-plugin-store` save that `settings.json`, `shortcuts.json`, and `license.json` go
//! through is a temp file renamed over the target, never a truncate-then-write, and it never
//! lands over a store file that didn't load. Cmdr patches the plugin for both
//! (`vendor/tauri-plugin-store/`), so these tests run the real plugin on a mock app: they fail the
//! moment the patch stops applying.

use std::fs;
#[cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan).
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use tauri::test::MockRuntime;
use tauri_plugin_store::{Store, StoreExt};

use crate::test_support::TestDir;

fn app() -> tauri::App<MockRuntime> {
    let app = tauri::test::mock_app();
    app.handle()
        .plugin(tauri_plugin_store::Builder::default().build())
        .expect("the store plugin registers on a mock app");
    app
}

fn store_at(app: &tauri::App<MockRuntime>, path: &Path) -> Arc<Store<MockRuntime>> {
    app.store_builder(path)
        .disable_auto_save()
        .build()
        .expect("a store at an absolute path")
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read the store's dir")
        .map(|entry| entry.expect("a dir entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_unstable();
    names
}

/// A rename swaps in a new file (a new inode) and leaves nothing beside it; a truncate-then-write
/// keeps the inode, which is exactly the window where a crash leaves a half-written file.
#[cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan).
#[test]
fn a_save_renames_a_finished_file_over_the_old_one() {
    let dir = TestDir::new("plugin_store_rename");
    let path = dir.join("settings.json");
    let app = app();
    let store = store_at(&app, &path);

    store.set("listing.showHiddenFiles", true);
    store.save().expect("the first save");
    let first_inode = fs::metadata(&path).expect("the file exists").ino();

    store.set("listing.showHiddenFiles", false);
    store.save().expect("the second save");

    assert_ne!(fs::metadata(&path).expect("the file exists").ino(), first_inode);
    assert_eq!(
        names_in(&dir),
        vec!["settings.json".to_string()],
        "no temp file left behind"
    );
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).expect("read back")).expect("valid JSON");
    assert_eq!(saved["listing.showHiddenFiles"], serde_json::json!(false));
}

/// The rename swaps in a new file, so it has to carry the old one's mode: a store someone locked
/// down to 0600 stays 0600.
#[cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan).
#[test]
fn a_save_keeps_the_store_files_permissions() {
    let dir = TestDir::new("plugin_store_mode");
    let path = dir.join("license.json");
    let app = app();
    let store = store_at(&app, &path);

    store.set("license_key_in_secret_store", true);
    store.save().expect("the first save");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("lock the file down");

    store.set("license_key_in_secret_store", false);
    store.save().expect("the second save");

    assert_eq!(
        fs::metadata(&path).expect("the file exists").permissions().mode() & 0o777,
        0o600
    );
}

/// A save that can't finish (here a read-only folder, the same shape as a full disk) reports
/// itself to the caller and leaves the old file byte for byte.
#[cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan).
#[test]
fn a_save_that_cant_finish_leaves_the_old_file_and_says_so() {
    // Root ignores the folder's permissions (the Linux Docker lane runs as root).
    // SAFETY: (test) `geteuid` takes no arguments, shares no memory, and can't fail — it just
    // returns the caller's effective uid. We compare the returned integer to 0 to detect root.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let dir = TestDir::new("plugin_store_readonly");
    let path = dir.join("settings.json");
    let app = app();
    let store = store_at(&app, &path);

    store.set("analytics.enabled", false);
    store.save().expect("the first save");
    let before = fs::read(&path).expect("read the saved file");

    fs::set_permissions(&*dir, fs::Permissions::from_mode(0o555)).expect("make the folder read-only");
    store.set("analytics.enabled", true);
    let outcome = store.save();
    fs::set_permissions(&*dir, fs::Permissions::from_mode(0o755)).expect("make the folder writable again");

    assert!(outcome.is_err(), "the caller hears that the save didn't land");
    assert_eq!(fs::read(&path).expect("read the file again"), before);
    assert_eq!(names_in(&dir), vec!["settings.json".to_string()]);
}

/// A rename that fails after the temp file was written (here the target is a folder) cleans the
/// temp file up, so a stray copy can't pile up beside the store.
#[test]
fn a_save_whose_rename_fails_removes_its_temp_file() {
    let dir = TestDir::new("plugin_store_rename_fails");
    let path = dir.join("settings.json");
    fs::create_dir(&path).expect("a folder where the file should be");
    fs::write(path.join("keep"), b"x").expect("a non-empty folder can't be renamed over");
    let app = app();
    let store = store_at(&app, &path);

    store.set("analytics.enabled", false);
    assert!(store.save().is_err());
    assert_eq!(names_in(&dir), vec!["settings.json".to_string()]);
}

#[cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan).
fn running_as_root() -> bool {
    // SAFETY: (test) `geteuid` takes no arguments, shares no memory, and can't fail — it just
    // returns the caller's effective uid. We compare the returned integer to 0 to detect root.
    unsafe { libc::geteuid() == 0 }
}

/// A store file that's there but can't be read (owned by root after a `sudo` run, a restore that
/// reset its mode) loads as an empty store. Every save of that store refuses, the exit save
/// included (it calls the same `Store::save`), so the empty store never lands over the real one.
#[cfg(unix)] // POSIX-only: `LocalPosixVolume`, modes, inodes, `libc`, or `std::os::unix` (windows-port plan).
#[test]
fn an_unreadable_store_file_is_never_saved_over() {
    // Root reads a mode-000 file anyway (the Linux Docker lane runs as root).
    if running_as_root() {
        return;
    }
    let dir = TestDir::new("plugin_store_unreadable");
    let path = dir.join("settings.json");
    fs::write(&path, b"{\"analytics.enabled\": false}").expect("write the real settings");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("make the file unreadable");
    let app = app();
    let store = store_at(&app, &path);

    store.set("listing.showHiddenFiles", true);
    let outcome = store.save();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("make the file readable again");

    assert!(
        matches!(outcome, Err(tauri_plugin_store::Error::LoadFailed { .. })),
        "the save refuses with the typed error: {outcome:?}"
    );
    assert_eq!(
        fs::read(&path).expect("read the file again"),
        b"{\"analytics.enabled\": false}"
    );
}

/// A store file that doesn't parse, and that `state_recovery` couldn't set aside, also loads as
/// empty, and is never saved over either.
#[test]
fn a_store_file_that_doesnt_parse_is_never_saved_over() {
    let dir = TestDir::new("plugin_store_unparsed");
    let path = dir.join("shortcuts.json");
    fs::write(&path, b"{\"trunc").expect("write a truncated file");
    let app = app();
    let store = store_at(&app, &path);

    store.set("file.copy", serde_json::json!(["F5"]));

    assert!(matches!(
        store.save(),
        Err(tauri_plugin_store::Error::LoadFailed { .. })
    ));
    assert_eq!(fs::read(&path).expect("read the file again"), b"{\"trunc");
}

/// The refusal lasts until a load succeeds: once the file reads again, saves go through.
#[test]
fn a_store_saves_again_once_a_reload_succeeds() {
    let dir = TestDir::new("plugin_store_reload");
    let path = dir.join("app-status.json");
    fs::write(&path, b"not json").expect("write a broken file");
    let app = app();
    let store = store_at(&app, &path);
    assert!(store.save().is_err());

    fs::write(&path, b"{\"leftPath\": \"/Users\"}").expect("the file is healthy again");
    store.reload().expect("the reload reads it");
    store.set("rightPath", "/tmp");
    store.save().expect("the save goes through");

    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).expect("read back")).expect("valid JSON");
    assert_eq!(saved, serde_json::json!({ "leftPath": "/Users", "rightPath": "/tmp" }));
}
