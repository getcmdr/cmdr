//! `open_with_default_app`: the volume's say comes before any launch, and the
//! opener's refusal comes back typed.

use super::*;
use crate::file_system::volume::Volume;
use crate::file_system::volume::manager::test_support::TestVolumeRegistration;
use crate::test_support::CapabilityStub;
use std::sync::Arc;

/// A registered volume answering `paths_are_os_visible`, under a per-test id so
/// the process-wide registry can't make two tests collide.
fn register(id: &str, paths_are_os_visible: bool) -> TestVolumeRegistration {
    TestVolumeRegistration::install(
        id,
        Arc::new(CapabilityStub {
            supports_local_fs_access: false,
            paths_are_os_visible,
        }) as Arc<dyn Volume>,
    )
}

/// A path nothing will ever create, so a real opener can only refuse it.
const MISSING: &str = "/nonexistent-cmdr-default-app-test/never-here.txt";

/// The reported bug's shape: a direct SMB share whose macOS mount is gone. Its
/// rows still read `/Volumes/<share>/…`, and handing one to `open` did nothing,
/// silently. Refused before the opener runs.
#[tokio::test]
async fn a_volume_whose_paths_other_apps_cant_open_refuses_before_launching() {
    let _volume = register("default-app-test-stranded-smb", false);
    let answer = open_with_default_app(MISSING.to_string(), Some("default-app-test-stranded-smb".to_string())).await;
    assert_eq!(answer, Err(OpenPathError::NotOnThisMac));
}

/// The opener's own refusal used to be dropped (`spawn` and walk away). macOS's
/// `open` exits 1 for a missing file and launches nothing, which makes it safe to
/// run here.
#[cfg(all(target_os = "macos", not(feature = "playwright-e2e")))]
#[tokio::test]
async fn the_openers_refusal_comes_back_typed() {
    let _volume = register("default-app-test-visible", true);
    let answer = open_with_default_app(MISSING.to_string(), Some("default-app-test-visible".to_string())).await;
    assert!(
        matches!(answer, Err(OpenPathError::NotOpened { exit_code: Some(code) }) if code != 0),
        "got {answer:?}"
    );
}

/// No volume to ask (the search-results snapshot's id, a favorite): the opener
/// answers for the path itself.
#[cfg(all(target_os = "macos", not(feature = "playwright-e2e")))]
#[tokio::test]
async fn an_unregistered_volume_leaves_the_answer_to_the_opener() {
    let answer = open_with_default_app(MISSING.to_string(), Some("default-app-test-nobody-here".to_string())).await;
    assert!(matches!(answer, Err(OpenPathError::NotOpened { .. })), "got {answer:?}");
}

/// Windows: a path whose plain spelling would name another file, or that runs past what
/// apps take, is refused before the Shell sees it.
#[cfg(all(target_os = "windows", not(feature = "playwright-e2e")))]
#[tokio::test]
async fn a_path_with_no_plain_form_is_refused_before_launching() {
    let long = format!("C:/{}/never-here.txt", "a".repeat(300));
    assert_eq!(open_with_default_app(long, None).await, Err(OpenPathError::NoPlainPath));
    let device = "C:/nonexistent-cmdr-default-app-test/con.txt".to_string();
    assert_eq!(
        open_with_default_app(device, None).await,
        Err(OpenPathError::NoPlainPath)
    );
}

/// Windows: the Shell's refusal of a missing file comes back typed, its Win32 code in place
/// of an exit code. `SEE_MASK_FLAG_NO_UI` keeps it from showing a box, so it's safe on CI.
#[cfg(all(target_os = "windows", not(feature = "playwright-e2e")))]
#[tokio::test]
async fn the_shells_refusal_of_a_missing_file_comes_back_typed() {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PATH_NOT_FOUND: i32 = 3;
    let missing = "C:/nonexistent-cmdr-default-app-test/never-here.txt".to_string();
    let answer = open_with_default_app(missing, None).await;
    assert!(
        matches!(
            answer,
            Err(OpenPathError::NotOpened {
                exit_code: Some(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
            })
        ),
        "got {answer:?}"
    );
}
