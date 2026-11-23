use std::io::Write;

use super::*;

#[test]
fn a_created_file_is_private_to_its_creator() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("secret");
    create_new(&path).expect("create").write_all(b"s3cret").expect("write");
    assert!(is_private_to_current_user(&path).expect("read access"));
    assert_eq!(std::fs::read(&path).expect("read back"), b"s3cret");
}

/// The checker has to be able to say no, or every assertion built on it passes for free.
#[test]
fn a_file_made_the_ordinary_way_is_not_private() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("ordinary");
    std::fs::write(&path, "x").expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("loosen it");
    }
    assert!(!is_private_to_current_user(&path).expect("read access"));
}

#[test]
fn create_new_refuses_a_file_already_there_and_leaves_it_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("taken");
    std::fs::write(&path, "theirs").expect("seed");
    let err = create_new(&path).expect_err("must not reuse a file that's there");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "theirs");
}

#[cfg(unix)] // Making a symlink on Windows needs a privilege the test runner may not hold.
#[test]
fn create_new_refuses_a_dangling_link_rather_than_creating_its_target() {
    let dir = tempfile::tempdir().expect("temp dir");
    let target = dir.path().join("where-the-link-points");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).expect("plant a link");
    let err = create_new(&link).expect_err("must not follow the link");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert!(!target.exists(), "the link's target must not be created");
}

#[test]
fn write_atomically_replaces_a_loose_file_with_a_private_one_and_leaves_no_temp() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("mcp.token");
    std::fs::write(&path, "stale").expect("seed a leftover");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("loosen it");
    }

    write_atomically(&path, b"fresh").expect("write");

    assert_eq!(std::fs::read_to_string(&path).expect("read"), "fresh");
    assert!(is_private_to_current_user(&path).expect("read access"));
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("mcp.token")], "the temp is gone");
}

#[test]
fn write_atomically_into_a_missing_folder_fails_and_leaves_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("missing").join("mcp.token");
    assert!(write_atomically(&path, b"x").is_err());
    assert!(!dir.path().join("missing").exists());
}

#[cfg(unix)]
#[test]
fn on_unix_the_mode_is_exactly_0600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("secret");
    write_atomically(&path, b"x").expect("write");
    let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use crate::private_file::platform::{AccessEntry, current_user_sid, file_security};

    /// `FILE_ALL_ACCESS`.
    const FULL_ACCESS: u32 = 0x001F_01FF;

    /// Every entry, read back: owned by this user, protected from the folder, and one allow entry
    /// for this user with full access and no inheritance flags. Nothing for `SYSTEM`,
    /// `Administrators`, or anyone else.
    fn assert_exactly_owner_only(path: &Path) {
        let me = current_user_sid().expect("this user's SID");
        let security = file_security(path).expect("read the descriptor");
        assert_eq!(security.owner, me, "owned by this user");
        assert!(security.dacl_protected, "nothing inherits from the folder");
        assert_eq!(
            security.entries,
            Some(vec![AccessEntry {
                kind: 0,
                flags: 0,
                mask: FULL_ACCESS,
                sid: Some(me),
            }]),
        );
    }

    #[test]
    fn a_created_file_carries_one_entry_for_this_user_and_nothing_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("secret");
        drop(create_new(&path).expect("create"));
        assert_exactly_owner_only(&path);
    }

    /// A folder that hands everyone full access to whatever is made in it (what a relocated
    /// `CMDR_DATA_DIR` or a redirected `AppData` could do) must not reach the file.
    #[test]
    fn a_folder_open_to_everyone_lends_the_file_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let status = std::process::Command::new("icacls")
            .arg(dir.path())
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"]) // Everyone, inherited by files and folders
            .stdout(std::process::Stdio::null())
            .status()
            .expect("run icacls");
        assert!(status.success(), "open the folder to everyone");
        let ordinary = dir.path().join("ordinary");
        std::fs::write(&ordinary, "x").expect("write");
        assert!(
            file_security(&ordinary)
                .expect("read")
                .entries
                .expect("a DACL")
                .iter()
                .any(|entry| entry.sid.as_deref() == Some("S-1-1-0")),
            "the folder's grant reaches an ordinary file, so the setup holds"
        );

        let path = dir.path().join("mcp.token");
        write_atomically(&path, b"token").expect("write");
        assert_exactly_owner_only(&path);
    }
}
