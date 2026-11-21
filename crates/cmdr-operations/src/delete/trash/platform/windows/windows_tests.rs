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
            // A link (a junction too) goes as itself, ❌ never through to its target.
            let _ = if meta.file_type().is_symlink() {
                fs::remove_dir(item).or_else(|_| fs::remove_file(item))
            } else if meta.is_dir() {
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

/// `IFileOperation` works only in a single-threaded apartment, so a caller whose thread
/// already joined a multithreaded one still gets its recycle, on a thread of its own.
#[test]
fn a_thread_in_a_multithreaded_apartment_still_recycles() {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    let dir = TestDir::new("recycle_mta");
    let file = dir.join("mta.txt");
    fs::write(&file, "x").unwrap();
    let on_thread = file.clone();
    std::thread::spawn(move || {
        // SAFETY: this fresh thread enters a multithreaded apartment, balanced below.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        assert!(hr.is_ok(), "{hr:?}");
        let landed = recycled(&on_thread);
        drop(landed);
        // SAFETY: balances the `CoInitializeEx` above, on the same thread.
        unsafe { CoUninitialize() };
    })
    .join()
    .unwrap();

    assert!(fs::symlink_metadata(&file).is_err(), "it went to the Recycle Bin");
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

/// `file` (on `C:`) as the admin share `\\localhost\C$` reaches it, a path with no Recycle
/// Bin, or `None` (saying why) when the share isn't reachable: it needs an administrator's
/// token, and without one the share refuses the path itself.
#[expect(
    clippy::print_stderr,
    reason = "a skip says why, the way `cmdr_fs::testing::skip_without_symlinks` does"
)]
fn through_the_admin_share(file: &Path) -> Option<PathBuf> {
    let local = file.to_string_lossy().replace('/', "\\");
    let Some(rest) = local.strip_prefix("C:\\").or_else(|| local.strip_prefix("c:\\")) else {
        eprintln!("{SKIP_MARKER}the temp folder isn't on C: ({local})");
        return None;
    };
    let through_share = PathBuf::from(format!("\\\\localhost\\C$\\{rest}"));
    if fs::metadata(&through_share).is_err() {
        eprintln!("{SKIP_MARKER}\\\\localhost\\C$ isn't reachable here (no administrator token)");
        return None;
    }
    Some(through_share)
}

fn assert_no_trash_for_volume(result: &Result<Option<PathBuf>, MutationError>) {
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
}

/// A path through a share has no Recycle Bin, so the Shell would delete it for good: it
/// refuses before the Shell is asked (not a fixed disk) and the file stays.
#[test]
fn a_path_with_no_recycle_bin_is_refused_and_kept() {
    let dir = TestDir::new("recycle_no_bin");
    let file = dir.join("keep-me.txt");
    fs::write(&file, "keep").unwrap();
    let Some(through_share) = through_the_admin_share(&file) else {
        return;
    };

    assert_no_trash_for_volume(&move_to_trash_sync(&through_share));
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep", "never deleted for good");
}

/// The sink's veto on its own, past the drive-type wall: the Shell, handed a path with no
/// Recycle Bin, announces a delete it won't recycle, and the veto keeps the file.
#[test]
fn the_sink_vetoes_a_delete_the_shell_would_not_recycle() {
    let dir = TestDir::new("recycle_sink_veto");
    let file = dir.join("keep-me.txt");
    fs::write(&file, "keep").unwrap();
    let Some(through_share) = through_the_admin_share(&file) else {
        return;
    };

    let result = shell::recycle(&through_share);

    assert!(
        matches!(result, Err(shell::Refusal::NoRecycleBin)),
        "{:?}",
        result.as_ref().err()
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep", "never deleted for good");
}

/// Marks `file` sparse (`fsutil`) and stretches it to `len` bytes, which takes no disk
/// space; `false` when the volume can't (then the caller skips).
fn sparse_of_len(file: &Path, len: u64) -> bool {
    let marked = std::process::Command::new("fsutil")
        .args(["sparse", "setflag"])
        .arg(file)
        .output()
        .is_ok_and(|out| out.status.success());
    marked
        && fs::OpenOptions::new()
            .write(true)
            .open(file)
            .and_then(|f| f.set_len(len))
            .is_ok()
}

const MIB: u64 = 1 << 20;

/// A sparse file of `len` bytes at `dir\name` (no disk space taken), or `None` when the
/// volume can't hold one (then the caller skips).
fn sparse_file(dir: &Path, name: &str, len: u64) -> Option<PathBuf> {
    let file = dir.join(name);
    fs::write(&file, "head").unwrap();
    sparse_of_len(&file, len).then_some(file)
}

/// The Recycle Bin limit of the drive `dir` sits on, as `bin.rs` reads it.
fn bin_limit(dir: &Path) -> u64 {
    bin::limit_of(&PathBuf::from(dir.to_string_lossy().replace('/', "\\"))).expect("a fixed drive has a bin limit")
}

/// A file past the bin's limit is one the Shell announces as a recycle and then deletes
/// for good with no word under `FOF_NO_UI`, out of the sink's sight: `bin.rs` weighs it
/// first, and it stays. A sparse file stands in for a big one.
#[test]
#[expect(clippy::print_stderr, reason = "a skip says why")]
fn a_file_past_the_recycle_bins_limit_is_refused_and_kept() {
    let dir = TestDir::new("recycle_too_big");
    let len = bin_limit(&dir) + MIB;
    let Some(file) = sparse_file(&dir, "huge.bin", len) else {
        eprintln!("{SKIP_MARKER}this volume can't hold a sparse file");
        return;
    };

    let result = move_to_trash_sync(&file);
    // If the bin took it after all, take it back out before the assertion fails.
    let _landed = match &result {
        Ok(Some(landed)) => Some(InBin(landed.clone())),
        _ => None,
    };

    assert_no_trash_for_volume(&result);
    assert_eq!(fs::metadata(&file).unwrap().len(), len, "never deleted for good");
}

/// The other side of the line: a file right at the bin's limit still recycles.
#[test]
#[expect(clippy::print_stderr, reason = "a skip says why")]
fn a_file_at_the_recycle_bins_limit_recycles() {
    let dir = TestDir::new("recycle_at_limit");
    let Some(file) = sparse_file(&dir, "big.bin", bin_limit(&dir)) else {
        eprintln!("{SKIP_MARKER}this volume can't hold a sparse file");
        return;
    };

    let _landed = recycled(&file);

    assert!(fs::symlink_metadata(&file).is_err());
}

/// A folder whose files each fit the bin but together don't can't move whole, so the
/// Shell falls back to its children, announcing them for good: the sink vetoes the first,
/// and every file stays.
#[test]
#[expect(clippy::print_stderr, reason = "a skip says why")]
fn a_folder_past_the_recycle_bins_limit_is_refused_whole() {
    let dir = TestDir::new("recycle_too_big_folder");
    let folder = dir.join("archive");
    fs::create_dir(&folder).unwrap();
    let each = bin_limit(&dir) / 5 * 3;
    let (Some(a), Some(b)) = (sparse_file(&folder, "a.bin", each), sparse_file(&folder, "b.bin", each)) else {
        eprintln!("{SKIP_MARKER}this volume can't hold a sparse file");
        return;
    };

    let result = move_to_trash_sync(&folder);
    let _landed = match &result {
        Ok(Some(landed)) => Some(InBin(landed.clone())),
        _ => None,
    };

    assert_no_trash_for_volume(&result);
    assert_eq!(fs::metadata(&a).unwrap().len(), each);
    assert_eq!(fs::metadata(&b).unwrap().len(), each);
}

/// A folder with one file another program holds open can't move as one: the trash
/// refuses and every file inside stays, the free ones too.
#[test]
fn a_folder_with_a_file_held_open_inside_is_refused_whole() {
    let dir = TestDir::new("recycle_folder_in_use");
    let folder = dir.join("project");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("free.txt"), "free").unwrap();
    let held_path = folder.join("held.txt");
    fs::write(&held_path, "held").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held_path)
        .unwrap();

    let result = move_to_trash_sync(&folder);
    let _landed = match &result {
        Ok(Some(landed)) => Some(InBin(landed.clone())),
        _ => None,
    };
    drop(held);

    assert!(matches!(result, Err(MutationError::TrashRefused { .. })), "{result:?}");
    assert_eq!(fs::read_to_string(folder.join("free.txt")).unwrap(), "free");
    assert_eq!(fs::read_to_string(&held_path).unwrap(), "held");
}

/// A junction recycles as the link: its target's files stay where they are.
#[test]
#[expect(clippy::print_stderr, reason = "a skip says why")]
fn a_junction_recycles_as_the_link_and_keeps_its_target() {
    let dir = TestDir::new("recycle_junction");
    let target = dir.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), "keep").unwrap();
    let link = dir.join("link");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .is_ok_and(|out| out.status.success());
    if !made {
        eprintln!("{SKIP_MARKER}couldn't make a junction here");
        return;
    }

    let landed = recycled(&link);

    assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "keep",
        "its target stays"
    );
    drop(landed);
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "keep",
        "and the guard's cleanup took only the link"
    );
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
