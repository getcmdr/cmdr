//! The Windows Shell calls behind handing a file to other apps: open it in its default app
//! (`ShellExecuteExW`), show it selected in File Explorer (`SHOpenFolderAndSelectItems`), and the
//! single-threaded COM apartment both want.
//!
//! Each takes the NUL-terminated plain path `platform::windows_shell_path::shell_path` builds.
//! ❌ Never `explorer.exe /select,<path>`: a comma in the path splits its argument, and it opens a
//! new window rather than reusing the one already showing that folder.

use cmdr_fs::app_path::NativeRefusal;
use windows_sys::Win32::Foundation::{ERROR_NO_ASSOCIATION, GetLastError};

use crate::file_system::default_app::OpenPathError;
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SHOpenFolderAndSelectItems, SHParseDisplayName,
    ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// A thread for one Shell call. The Shell loads third-party extensions into the caller, so it
/// gets the same 8 MB stack a macOS framework call does (`file_system/CLAUDE.md`).
pub(crate) const SHELL_THREAD_STACK: usize = 8 * 1024 * 1024;

/// Runs `call` inside a single-threaded apartment entered and left on THIS thread, the apartment
/// the Shell's calls ask for. Call it on a fresh thread: one that already joined the
/// multithreaded apartment can't enter this one.
///
/// # Errors
///
/// The `CoInitializeEx` `HRESULT` when the apartment can't be entered; `call` didn't run.
pub(crate) fn in_apartment<T>(call: impl FnOnce() -> T) -> Result<T, i32> {
    // SAFETY: no reserved pointer; a success (`S_OK` or `S_FALSE`) is balanced by the
    // `CoUninitialize` below, on this same thread.
    let entered = unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).cast_unsigned(),
        )
    };
    if entered < 0 {
        return Err(entered);
    }
    let answer = call();
    // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
    unsafe { CoUninitialize() };
    Ok(answer)
}

/// The `openas` verb: Windows' "How do you want to open this file?" picker.
const OPEN_AS: &[u16] = &[
    b'o' as u16,
    b'p' as u16,
    b'e' as u16,
    b'n' as u16,
    b'a' as u16,
    b's' as u16,
    0,
];

/// Enter's default-app open (`file_system::default_app`) for the wire text `path`: the Shell's
/// path first ([`OpenPathError::NoPlainPath`], nothing launched, for a path with no faithful plain
/// form), then [`open_with_default_app`] on a thread of its own in an apartment, waited on for at
/// most `deadline`. A thread past the deadline is left to finish: the Shell call can't be cut.
pub(crate) async fn launch_default_app(path: String, deadline: std::time::Duration) -> Result<(), OpenPathError> {
    let wide = shell_path_of(&path)?;
    let (answer_tx, answer_rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("open-default-app".to_string())
        .stack_size(SHELL_THREAD_STACK)
        .spawn(move || {
            // A closed receiver is a caller past its deadline, with no one left to tell.
            let _ = answer_tx.send(in_apartment(|| open_with_default_app(&wide)));
        })
        .map_err(|e| OpenPathError::LaunchRefused {
            errno: e.raw_os_error(),
        })?;
    // Dropping the receiver at the deadline cancels nothing: the thread owns the Shell call.
    let answer = match tokio::time::timeout(deadline, answer_rx).await {
        Err(_elapsed) => Err(OpenPathError::TimedOut),
        Ok(Err(_thread_gone)) => Err(OpenPathError::LaunchRefused { errno: None }),
        Ok(Ok(Err(hresult))) => Err(OpenPathError::LaunchRefused { errno: Some(hresult) }),
        Ok(Ok(Ok(Err(code)))) => Err(OpenPathError::NotOpened {
            exit_code: i32::try_from(code).ok(),
        }),
        Ok(Ok(Ok(Ok(())))) => Ok(()),
    };
    if let Err(why) = &answer {
        log::info!(
            target: "file_actions",
            "The Shell didn't open {}: {why:?}",
            cmdr_fs::log_private::LogPrivate::path(&path)
        );
    }
    answer
}

/// The Shell's path for the wire text `path`: [`OpenPathError::NoPlainPath`] for a path whose
/// plain form would name another file or runs too long, [`OpenPathError::NotOnThisMac`] for text
/// that names no Windows file at all (a scheme path).
fn shell_path_of(path: &str) -> Result<Vec<u16>, OpenPathError> {
    let app_path = crate::commands::wire_path::app_path(path)
        .ok()
        .flatten()
        .ok_or(OpenPathError::NotOnThisMac)?;
    crate::platform::windows_shell_path::shell_path(&app_path).map_err(|why| match why {
        NativeRefusal::NotPlainSafe(_) => OpenPathError::NoPlainPath,
        _ => OpenPathError::NotOnThisMac,
    })
}

/// Opens `path` (NUL-terminated) with its default app, as a double-click in File Explorer does,
/// inside an apartment ([`in_apartment`]). A file no app is set to open gets Windows' own
/// "How do you want to open this file?" picker, as it does in File Explorer.
///
/// # Errors
///
/// The Win32 error code `ShellExecuteExW` set: `ERROR_FILE_NOT_FOUND`, `ERROR_ACCESS_DENIED`, and
/// so on. ❗ Read as a number only, ❌ never as its message text.
pub(crate) fn open_with_default_app(path: &[u16]) -> Result<(), u32> {
    match execute(path, None) {
        Err(ERROR_NO_ASSOCIATION) => execute(path, Some(OPEN_AS)),
        answer => answer,
    }
}

/// `ShellExecuteExW` with `verb` (`None`: the file type's default), waiting until the Shell has
/// handed the file over (`SEE_MASK_NOASYNC`, so the thread can end right after) and showing no
/// error box of its own: the caller words the refusal.
fn execute(path: &[u16], verb: Option<&[u16]>) -> Result<(), u32> {
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.map_or(std::ptr::null(), <[u16]>::as_ptr),
        lpFile: path.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: `path` and `verb` are NUL-terminated (the callers' contract) and outlive the call;
    // every other pointer in `info` is null, and `info` outlives the call too.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: reads this thread's last error, which the failed call above just set.
        return Err(unsafe { GetLastError() });
    }
    Ok(())
}

/// Shows `path` (NUL-terminated) selected in File Explorer, inside an apartment
/// ([`in_apartment`]). A window already showing its folder is reused; a drive root shows
/// selected in "This PC".
///
/// # Errors
///
/// The failing call's `HRESULT`: `SHParseDisplayName`'s for an item the Shell can't find (it's
/// gone, or the share is unreachable), else `SHOpenFolderAndSelectItems`'s.
pub(crate) fn reveal(path: &[u16]) -> Result<(), i32> {
    let mut item = std::ptr::null_mut();
    // SAFETY: `path` is NUL-terminated and outlives the call; no bind context, no attribute
    // query; `item` receives an ID list the Shell allocates, freed below.
    let parsed = unsafe { SHParseDisplayName(path.as_ptr(), std::ptr::null_mut(), &mut item, 0, std::ptr::null_mut()) };
    if parsed < 0 {
        return Err(parsed);
    }
    // SAFETY: `item` is the fully qualified ID list parsed above; with no children (`cidl` 0) the
    // call opens the item's folder with the item selected.
    let shown = unsafe { SHOpenFolderAndSelectItems(item, 0, std::ptr::null(), 0) };
    // SAFETY: frees the ID list `SHParseDisplayName` allocated, once, after its last use.
    unsafe { CoTaskMemFree(item.cast_const().cast()) };
    if shown < 0 { Err(shown) } else { Ok(()) }
}
