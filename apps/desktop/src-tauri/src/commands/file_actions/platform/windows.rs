//! Windows: no reveal, Get Info, or editor launch yet (`SHOpenFolderAndSelectItems` and
//! `ShellExecuteW` are the ones to add), so each answers as before it had an arm. The Recycle
//! Bin opens in File Explorer (`open_trash_view`).

use cmdr_fs::app_path::AppPath;
use cmdr_operations::MutationError;

use super::TrashView;
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
        std::thread::spawn(open_recycle_bin)
            .join()
            .unwrap_or(Err(E_UNEXPECTED))
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
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, SEE_MASK_FLAG_NO_UI, SEE_MASK_IDLIST, SHELLEXECUTEINFOW,
        SHGetKnownFolderIDList, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // SAFETY: a fresh thread joins a single-threaded apartment; a success (`S_OK` or
    // `S_FALSE`) is balanced by the `CoUninitialize` below, on this same thread.
    let entered = unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned(),
        )
    };
    if entered < 0 {
        return Err(entered);
    }
    let mut folder = std::ptr::null_mut();
    // SAFETY: `folder` receives an ID list the Shell allocates, freed below after the one
    // call that reads it; the struct's other pointers are null (the default verb, no file),
    // and `info` outlives the call.
    let opened = unsafe {
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
    };
    // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
    unsafe { CoUninitialize() };
    opened
}

/// Not available yet.
pub(crate) fn reveal_in_file_manager(_path: String) -> Result<(), String> {
    Err("Show in file manager is not available on this platform".to_string())
}

/// No Get Info window; a no-op.
pub(crate) async fn get_info(_path: String) -> Result<(), GetInfoError> {
    Ok(())
}

/// Refused, except in the E2E build, which records the file like the other arms.
pub(crate) async fn open_in_editor(
    path: String,
    _app_choice: String,
    _ask_about_other_editors: bool,
) -> Result<EditorOpenReport, OpenInEditorError> {
    #[cfg(feature = "playwright-e2e")]
    {
        crate::open_mock::record(path);
        Ok(EditorOpenReport {
            outcome: crate::file_system::text_editor::EditorOpenOutcome::Opened,
            opened_in_name: None,
            other_editors_installed: None,
        })
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        let _ = path;
        Err(OpenInEditorError::LaunchRefused { errno: None })
    }
}
