//! The Recycle Bin arm against the real Shell, on NTFS. Each test that recycles
//! something holds an [`InBin`] guard, which deletes exactly that `$R…` item and its
//! `$I…` record on drop, so a run never leaves anything in the user's Recycle Bin.

use std::fs;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use cmdr_fs::testing::{SKIP_MARKER, TestDir};

use super::shell::refusal_kind_of;
use super::*;

/// One item this test put in the Recycle Bin, taken back out on drop.
struct InBin(PathBuf);

impl Drop for InBin {
    fn drop(&mut self) {
        let item = &self.0;
        if let Ok(meta) = fs::symlink_metadata(item) {
            let mut permissions = meta.permissions();
            #[expect(
                clippy::permissions_set_readonly_false,
                reason = "Windows: clears the read-only attribute so the test's own item can go"
            )]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(item, permissions);
            let _ = if meta.is_dir() {
                fs::remove_dir_all(item)
            } else {
                fs::remove_file(item)
            };
        }
        if let Some(record) = recycle_record_of(item) {
            let _ = fs::remove_file(record);
        }
    }
}

fn recycled(source: &Path) -> InBin {
    match move_to_trash_sync(source) {
        Ok(Some(landed)) => InBin(landed),
        other => panic!("expected the item in the Recycle Bin, got {other:?}"),
    }
}

#[test]
fn a_file_lands_in_the_recycle_bin_with_its_bytes() {
    let dir = TestDir::new("recycle_file");
    let file = dir.join("report.txt");
    fs::write(&file, "the bytes").unwrap();

    let landed = recycled(&file);

    assert!(fs::symlink_metadata(&file).is_err(), "the original is gone");
    let name = landed.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("$R") && name.ends_with(".txt"),
        "a Recycle Bin item: {}",
        landed.0.display()
    );
    assert!(
        landed.0.to_string_lossy().to_ascii_lowercase().contains("$recycle.bin"),
        "under the drive's Recycle Bin: {}",
        landed.0.display()
    );
    assert_eq!(fs::read_to_string(&landed.0).unwrap(), "the bytes");
    assert!(
        recycle_record_of(&landed.0).unwrap().exists(),
        "with its `$I` record beside it"
    );
}

#[test]
fn a_folder_lands_whole() {
    let dir = TestDir::new("recycle_folder");
    let folder = dir.join("photos");
    fs::create_dir_all(folder.join("2026")).unwrap();
    fs::write(folder.join("2026").join("a.txt"), "a").unwrap();

    let landed = recycled(&folder);

    assert!(fs::symlink_metadata(&folder).is_err(), "the original is gone");
    assert_eq!(fs::read_to_string(landed.0.join("2026").join("a.txt")).unwrap(), "a");
}

/// Windows moves a read-only file like any other; only a permanent delete refuses one.
#[test]
fn a_read_only_file_recycles() {
    let dir = TestDir::new("recycle_read_only");
    let file = dir.join("frozen.txt");
    fs::write(&file, "x").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();

    let _landed = recycled(&file);

    assert!(fs::symlink_metadata(&file).is_err());
}

#[test]
fn a_file_another_program_holds_open_refuses_as_in_use_and_stays() {
    let dir = TestDir::new("recycle_in_use");
    let file = dir.join("open.txt");
    fs::write(&file, "x").unwrap();
    // No sharing at all: the way a program that locks its document opens it.
    let _held = fs::OpenOptions::new().read(true).share_mode(0).open(&file).unwrap();

    let result = move_to_trash_sync(&file);

    assert!(
        matches!(
            result,
            Err(MutationError::TrashRefused {
                reason: TrashRefusalKind::InUse,
                ..
            })
        ),
        "{result:?}"
    );
    assert!(file.exists(), "a refusal leaves the item where it was");
}

/// A path through a share has no Recycle Bin, so the Shell would delete it for good;
/// the sink's veto keeps it. Reached through the admin share `\\localhost\C$`, which
/// needs an administrator's token: without one the share refuses the path itself,
/// and this cell skips, saying so.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "a skip says why, the way `cmdr_fs::testing::skip_without_symlinks` does"
)]
fn a_path_with_no_recycle_bin_is_refused_and_kept() {
    let dir = TestDir::new("recycle_no_bin");
    let file = dir.join("keep-me.txt");
    fs::write(&file, "keep").unwrap();
    let local = file.to_string_lossy().replace('/', "\\");
    let Some(rest) = local.strip_prefix("C:\\").or_else(|| local.strip_prefix("c:\\")) else {
        eprintln!("{SKIP_MARKER}the temp folder isn't on C: ({local})");
        return;
    };
    let through_share = PathBuf::from(format!("\\\\localhost\\C$\\{rest}"));
    if fs::metadata(&through_share).is_err() {
        eprintln!("{SKIP_MARKER}\\\\localhost\\C$ isn't reachable here (no administrator token)");
        return;
    }

    let result = move_to_trash_sync(&through_share);

    assert!(
        matches!(
            result,
            Err(MutationError::TrashRefused {
                reason: TrashRefusalKind::NoTrashForVolume,
                ..
            })
        ),
        "{result:?}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep", "never deleted for good");
}

#[test]
fn a_restore_renames_the_item_back_and_forgets_its_record() {
    let dir = TestDir::new("recycle_restore");
    let file = dir.join("back.txt");
    fs::write(&file, "back").unwrap();
    let landed = recycled(&file);
    let record = recycle_record_of(&landed.0).unwrap();

    fs::rename(&landed.0, &file).unwrap();
    forget_restored(&landed.0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "back");
    assert!(!record.exists(), "the Recycle Bin no longer lists it");
}

#[test]
fn only_a_recycle_bin_item_has_a_record() {
    assert_eq!(
        recycle_record_of(Path::new(r"C:\$Recycle.Bin\S-1-5-21-1\$RAB12CD.txt")),
        Some(PathBuf::from(r"C:\$Recycle.Bin\S-1-5-21-1\$IAB12CD.txt"))
    );
    assert_eq!(
        recycle_record_of(Path::new("D:/$RECYCLE.BIN/S-1-5-21-1/$RXY")),
        Some(PathBuf::from("D:/$RECYCLE.BIN/S-1-5-21-1").join("$IXY")),
        "either spelling, any case"
    );
    assert_eq!(
        recycle_record_of(Path::new(r"C:\Users\test_user_1\$Rnot-a-bin.txt")),
        None
    );
    assert_eq!(
        recycle_record_of(Path::new(r"C:\$Recycle.Bin\S-1-5-21-1\$IAB12CD.txt")),
        None
    );
}

#[test]
fn the_shells_codes_read_as_reasons() {
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL};
    use windows::core::HRESULT;

    assert_eq!(refusal_kind_of(HRESULT::from_win32(32)), TrashRefusalKind::InUse);
    assert_eq!(refusal_kind_of(HRESULT::from_win32(33)), TrashRefusalKind::InUse);
    assert_eq!(refusal_kind_of(E_ACCESSDENIED), TrashRefusalKind::NotPermitted);
    assert_eq!(refusal_kind_of(E_FAIL), TrashRefusalKind::Other);
}
