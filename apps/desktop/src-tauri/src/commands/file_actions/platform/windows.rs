//! Windows: reveals select the item in File Explorer (`SHOpenFolderAndSelectItems`, through
//! `platform::shell`), the Recycle Bin opens there too (`open_trash_view`), and F4 hands the file
//! to the chosen plain-text handler (`file_system::text_editor`). No Get Info window.

use cmdr_fs::app_path::AppPath;
use cmdr_operations::MutationError;

use super::{RevealError, TrashView};
use crate::file_system::get_info::GetInfoError;
use crate::file_system::text_editor::{EditorOpenReport, OpenInEditorError};

/// Opens the Recycle Bin in File Explorer when `path`'s drive keeps one (the answer the
/// volume's `has_trash`, the switcher row, and the trash's refusal read too,
/// `cmdr_local::recycle_bin`), else [`TrashView::NoView`].
///
/// Decision (#1332): Explorer's window, ❌ not a pane. The bin is one Shell folder
/// (`FOLDERID_RecycleBinFolder`) gathering every drive's `$Recycle.Bin`, whose `$R…` names
/// and original places only the `$I…` records hold, so no folder a pane lists is it, and a
/// listing of the records would be a second Recycle Bin to keep in step with the Shell's.
/// The E2E build records the open instead, as for a reveal.
pub(crate) fn open_trash_view(path: &AppPath) -> Result<TrashView, MutationError> {
    // The Shell's calls take the plain spelling, never the verbatim `\\?\` one.
    let has_bin = path
        .to_native_plain()
        .is_ok_and(|plain| cmdr_local::recycle_bin::drive_bin(&plain).is_some());
    if !has_bin {
        return Ok(TrashView::NoView);
    }
    #[cfg(feature = "playwright-e2e")]
    {
        crate::open_mock::record("shell:RecycleBinFolder".to_string());
        Ok(TrashView::Opened)
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        // A thread of its own, so the Shell gets the single-threaded apartment it asks for
        // whatever the caller's thread already joined.
        std::thread::Builder::new()
            .name("open-recycle-bin".to_string())
            .stack_size(crate::platform::shell::SHELL_THREAD_STACK)
            .spawn(open_recycle_bin)
            .map_err(|_| E_UNEXPECTED)
            .and_then(|thread| thread.join().unwrap_or(Err(E_UNEXPECTED)))
            .map(|()| TrashView::Opened)
            .map_err(|hr| MutationError::Unexpected {
                detail: format!(
                    "the Shell couldn't open the Recycle Bin [code {:#010x}]",
                    hr.cast_unsigned()
                )
                .into(),
            })
    }
}

/// `E_UNEXPECTED`, for a thread that didn't come back.
#[cfg(not(feature = "playwright-e2e"))]
const E_UNEXPECTED: i32 = 0x8000_FFFF_u32.cast_signed();

/// `ShellExecuteExW` with the Recycle Bin's known-folder ID list, in a single-threaded
/// apartment it enters and leaves on this thread. Returns once Explorer has the request.
#[cfg(not(feature = "playwright-e2e"))]
fn open_recycle_bin() -> Result<(), i32> {
    crate::platform::apartment::in_apartment(open_recycle_bin_in_apartment).and_then(|opened| opened)
}

/// [`open_recycle_bin`]'s Shell calls, inside the apartment.
#[cfg(not(feature = "playwright-e2e"))]
fn open_recycle_bin_in_apartment() -> Result<(), i32> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, SEE_MASK_FLAG_NO_UI, SEE_MASK_IDLIST, SHELLEXECUTEINFOW,
        SHGetKnownFolderIDList, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let mut folder = std::ptr::null_mut();
    // SAFETY: `folder` receives an ID list the Shell allocates, freed below after the one
    // call that reads it; the struct's other pointers are null (the default verb, no file),
    // and `info` outlives the call.
    unsafe {
        let found = SHGetKnownFolderIDList(
            &FOLDERID_RecycleBinFolder,
            KF_FLAG_DEFAULT.cast_unsigned(),
            std::ptr::null_mut(),
            &mut folder,
        );
        if found < 0 {
            Err(found)
        } else {
            let mut info = SHELLEXECUTEINFOW {
                cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
                fMask: SEE_MASK_IDLIST | SEE_MASK_FLAG_NO_UI,
                lpIDList: folder.cast(),
                nShow: SW_SHOWNORMAL,
                ..Default::default()
            };
            let result = if ShellExecuteExW(&mut info) == 0 {
                Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(E_UNEXPECTED))
            } else {
                Ok(())
            };
            CoTaskMemFree(folder.cast_const().cast());
            result
        }
    }
}

/// How long a reveal waits for the Shell's answer. Finding the item is one parse, fast on a
/// healthy disk; a dead share holds it far longer, and then the wait ends quietly (logged): the
/// window may still open.
#[cfg(not(feature = "playwright-e2e"))]
const REVEAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Shows `path` selected in File Explorer, reusing a window already on its folder, and says why
/// not when it didn't, typed, for the frontend's toast.
///
/// [`RevealError::NoPlainPath`] up front, with nothing asked, for a path the Shell can't take
/// (`platform::windows_shell_path`: past 259 units, a name ending in a dot or space, a device
/// name). Otherwise the Shell calls run on a thread of their own (`platform::shell`), waited on
/// for at most [`REVEAL_TIMEOUT`]; a refusal there (the item is gone) is
/// [`RevealError::NotShown`]. The E2E build records the path instead: Explorer would take the
/// front.
pub(crate) async fn reveal_in_file_manager(path: String) -> Result<(), RevealError> {
    use cmdr_fs::app_path::NativeRefusal;

    let app_path = crate::commands::wire_path::app_path(&path)
        .ok()
        .flatten()
        .ok_or(RevealError::NoPath)?;
    let wide = crate::platform::windows_shell_path::shell_path(&app_path).map_err(|why| match why {
        NativeRefusal::NotPlainSafe(_) => RevealError::NoPlainPath,
        _ => RevealError::NoPath,
    })?;
    #[cfg(feature = "playwright-e2e")]
    {
        let _ = wide;
        crate::open_mock::record(path);
        Ok(())
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        use crate::platform::shell::{ShellWait, on_shell_thread, reveal};

        let wait = on_shell_thread("reveal-in-explorer", REVEAL_TIMEOUT, move || reveal(&wide)).await;
        let shown = match wait {
            ShellWait::Answered(Ok(())) => Ok(()),
            // Explorer may still show it; no answer is no refusal to report.
            ShellWait::TimedOut => Ok(()),
            ShellWait::Answered(Err(_)) | ShellWait::NoApartment(_) | ShellWait::NoThread => Err(RevealError::NotShown),
        };
        match &shown {
            Ok(()) => log::debug!(
                target: "reveal",
                "Showed {} in File Explorer ({wait:?})",
                cmdr_fs::log_private::LogPrivate::path(&path)
            ),
            Err(_) => log::warn!(
                target: "reveal",
                "File Explorer didn't show {}: {wait:?}",
                cmdr_fs::log_private::LogPrivate::path(&path)
            ),
        }
        shown
    }
}

/// No Get Info window; a no-op.
pub(crate) async fn get_info(_path: String) -> Result<(), GetInfoError> {
    Ok(())
}

/// The chosen plain-text editor (`file_system::text_editor`), except in the E2E build, which
/// records the file like the other arms.
pub(crate) async fn open_in_editor(
    path: String,
    app_choice: String,
    ask_about_other_editors: bool,
) -> Result<EditorOpenReport, OpenInEditorError> {
    #[cfg(feature = "playwright-e2e")]
    {
        let _ = (app_choice, ask_about_other_editors);
        crate::open_mock::record(path);
        Ok(EditorOpenReport {
            outcome: crate::file_system::text_editor::EditorOpenOutcome::Opened,
            opened_in_name: None,
            other_editors_installed: None,
        })
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        crate::file_system::text_editor::open_in_editor(path, app_choice, ask_about_other_editors).await
    }
}
