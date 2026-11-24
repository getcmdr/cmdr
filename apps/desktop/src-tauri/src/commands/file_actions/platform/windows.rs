//! Windows: reveals select the item in File Explorer (`SHOpenFolderAndSelectItems`, through
//! `platform::shell`), and the Recycle Bin opens there too (`open_trash_view`). No Get Info or
//! editor launch yet (task 44b of #942), so those answer as before they had an arm.

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
    crate::platform::shell::in_apartment(open_recycle_bin_in_apartment).and_then(|opened| opened)
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

/// Shows `path` selected in File Explorer, reusing a window already on its folder.
///
/// Refused up front, with nothing launched, for a path the Shell can't take
/// (`platform::windows_shell_path`: past 259 units, a name ending in a dot or space, a device
/// name, a scheme path). Otherwise the Shell calls run on a thread of their own and this answers
/// at once, as the macOS arm does: `SHParseDisplayName` touches the file, which on a dead share
/// can take a long time, and a sync command must not wait on it. A Shell refusal there (the item
/// is gone) is logged. The E2E build records the path instead: Explorer would take the front.
pub(crate) fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let app_path = crate::commands::wire_path::app_path(&path)
        .ok()
        .flatten()
        .ok_or_else(|| "no file to show in File Explorer: not an app path".to_string())?;
    let wide = crate::platform::windows_shell_path::shell_path(&app_path)
        .map_err(|why| format!("no path File Explorer can show: {why}"))?;
    #[cfg(feature = "playwright-e2e")]
    {
        let _ = wide;
        crate::open_mock::record(path);
        Ok(())
    }
    #[cfg(not(feature = "playwright-e2e"))]
    {
        std::thread::Builder::new()
            .name("reveal-in-explorer".to_string())
            .stack_size(crate::platform::shell::SHELL_THREAD_STACK)
            .spawn(move || {
                let shown = crate::platform::shell::in_apartment(|| crate::platform::shell::reveal(&wide))
                    .and_then(|shown| shown);
                match shown {
                    Ok(()) => log::debug!(
                        target: "reveal",
                        "Showed {} in File Explorer",
                        cmdr_fs::log_private::LogPrivate::path(&path)
                    ),
                    Err(hr) => log::warn!(
                        target: "reveal",
                        "File Explorer didn't show {} [code {:#010x}]",
                        cmdr_fs::log_private::LogPrivate::path(&path),
                        hr.cast_unsigned()
                    ),
                }
            })
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
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
