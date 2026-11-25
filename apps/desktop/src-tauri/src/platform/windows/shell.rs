//! The Windows Shell calls behind handing a file to other apps: open it in its default app
//! (`ShellExecuteExW`), show it selected in File Explorer (`SHOpenFolderAndSelectItems`), and the
//! thread each runs on, in the COM apartment `apartment.rs` enters.
//!
//! Each takes the NUL-terminated plain paths `platform::windows_shell_path` builds, and pins the
//! exact item ONCE as an ID list (`SHParseDisplayName`) before acting on it: a path handed on as
//! text gets searched (`report` that's gone can resolve to `report.exe` beside it).
//! ❌ Never `explorer.exe /select,<path>`: a comma in the path splits its argument, and it opens a
//! new window rather than reusing the one already showing that folder.

use std::fmt::Debug;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_CANCELLED, ERROR_NO_ASSOCIATION, GetLastError};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_INVOKEIDLIST, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SHOpenFolderAndSelectItems,
    SHParseDisplayName, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::apartment::in_apartment;
use crate::file_system::default_app::OpenPathError;
use crate::platform::windows_shell_path::{ShellLaunchPaths, shell_launch_paths, win32_code};

/// A thread for one Shell call. The Shell loads third-party extensions into the caller, so it
/// gets the same 8 MB stack a macOS framework call does (`file_system/CLAUDE.md`).
pub(crate) const SHELL_THREAD_STACK: usize = 8 * 1024 * 1024;

/// Runs `call` in an apartment on a thread of its own, named `name`, and blocks until it answers:
/// for a caller that is already off the main thread and bounded by its own deadline (a command's
/// blocking task). `None` when the thread couldn't start, panicked, or the apartment refused.
pub(crate) fn blocking_on_shell_thread<T: Send + 'static>(
    name: &'static str,
    call: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let joined = std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(SHELL_THREAD_STACK)
        .spawn(move || in_apartment(call))
        .map(std::thread::JoinHandle::join);
    match joined {
        Ok(Ok(Ok(answer))) => Some(answer),
        Ok(Ok(Err(hresult))) => {
            // allowed-log-private: `name` is our own fixed thread name, and `hresult` a typed code
            log::warn!(target: "file_actions", "{name}: no COM apartment ({hresult:#010x})");
            None
        }
        Ok(Err(_panicked)) => {
            // allowed-log-private: `name` is our own fixed thread name
            log::warn!(target: "file_actions", "{name}: the Shell thread panicked");
            None
        }
        Err(e) => {
            // allowed-log-private: `name` is our own fixed thread name, and `e` the OS's spawn refusal
            log::warn!(target: "file_actions", "{name} couldn't start a thread: {e}");
            None
        }
    }
}

/// How a Shell call run by [`on_shell_thread`] ended for its caller.
#[derive(Debug)]
pub(crate) enum ShellWait<T> {
    /// The call ran and answered in time.
    Answered(T),
    /// The apartment couldn't be entered (its `HRESULT`), so the call didn't run.
    NoApartment(i32),
    /// No answer inside the deadline. The thread runs on; its late answer is logged.
    TimedOut,
    /// The thread couldn't start, or ended without answering (a panic).
    NoThread,
}

/// How long a thread that handed files over through `IAssocHandler::Invoke` or the "open with"
/// dialog keeps its apartment serving messages before it leaves (`apartment::pump_messages_for`):
/// long enough for a DDE or asynchronous handler to take the files, short enough that a thread
/// never lingers.
pub(crate) const HANDOFF_LINGER: Duration = Duration::from_secs(3);

/// Runs `call` in an apartment on a thread of its own, named `name`, and waits at most `deadline`
/// for its answer. A Shell call can't be cut, so a thread past the deadline is left to finish,
/// and what it answers then is logged rather than lost.
pub(crate) async fn on_shell_thread<T: Debug + Send + 'static>(
    name: &'static str,
    deadline: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> ShellWait<T> {
    on_shell_thread_lingering(name, deadline, Duration::ZERO, call).await
}

/// [`on_shell_thread`], with the thread staying in its apartment, serving messages, for `linger`
/// after it answers ([`HANDOFF_LINGER`] after an `Invoke`). The answer goes out first, so the
/// caller doesn't wait out the linger.
pub(crate) async fn on_shell_thread_lingering<T: Debug + Send + 'static>(
    name: &'static str,
    deadline: Duration,
    linger: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> ShellWait<T> {
    let (answer_tx, answer_rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(SHELL_THREAD_STACK)
        .spawn(move || {
            // Held outside the apartment call, so a refused apartment still answers with its code.
            let sender = std::cell::Cell::new(Some(answer_tx));
            let entered = in_apartment(|| {
                let answer = call();
                if let Some(tx) = sender.take()
                    && let Err(late) = tx.send(Ok(answer))
                {
                    // allowed-log-private: `name` is our own fixed thread name, and `late` a typed code
                    log::info!(target: "file_actions", "{name} answered after its deadline: {late:?}");
                }
                if !linger.is_zero() {
                    super::apartment::pump_messages_for(linger);
                }
            });
            if let (Err(hresult), Some(tx)) = (entered, sender.take()) {
                let _ = tx.send(Err(hresult));
            }
        });
    if let Err(e) = spawned {
        // allowed-log-private: `name` is our own fixed thread name, and `e` the OS's spawn refusal
        log::warn!(target: "file_actions", "{name} couldn't start a thread: {e}");
        return ShellWait::NoThread;
    }
    // Dropping the receiver at the deadline cancels nothing: the thread owns the Shell call.
    match tokio::time::timeout(deadline, answer_rx).await {
        Err(_elapsed) => ShellWait::TimedOut,
        Ok(Err(_thread_gone)) => ShellWait::NoThread,
        Ok(Ok(Err(hresult))) => ShellWait::NoApartment(hresult),
        Ok(Ok(Ok(answer))) => ShellWait::Answered(answer),
    }
}

/// Why the Shell didn't take a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellRefusal {
    /// A Win32 error code: `ERROR_FILE_NOT_FOUND`, `ERROR_ACCESS_DENIED`, `ERROR_CANCELLED`, …
    /// ❗ Read as a number only, ❌ never as its message text.
    Win32(u32),
    /// Any other `HRESULT`.
    Other(i32),
    /// No app by the asked handler's name offers to open this type any more
    /// (`assoc::invoke`): it was uninstalled since the list was read.
    NoHandler,
}

impl ShellRefusal {
    pub(super) fn of_hresult(hresult: i32) -> Self {
        win32_code(hresult).map_or(Self::Other(hresult), Self::Win32)
    }
}

/// Enter's default-app open (`file_system::default_app`) for the wire text `path`: the Shell's
/// paths first ([`OpenPathError::NoPlainPath`], nothing launched, for a path with no faithful
/// plain form), then [`open_with_default_app`] on a Shell thread, waited on for at most
/// `deadline`. A cancelled UAC prompt or a dismissed picker is [`OpenPathError::Cancelled`].
pub(crate) async fn launch_default_app(path: String, deadline: Duration) -> Result<(), OpenPathError> {
    let paths = shell_paths_of(&path)?;
    let wait = on_shell_thread("open-default-app", deadline, move || open_with_default_app(&paths)).await;
    let answer = match wait {
        ShellWait::Answered(Ok(())) => Ok(()),
        ShellWait::Answered(Err(ShellRefusal::Win32(ERROR_CANCELLED))) => Err(OpenPathError::Cancelled),
        ShellWait::Answered(Err(ShellRefusal::Win32(code))) => Err(OpenPathError::NotOpened {
            exit_code: i32::try_from(code).ok(),
        }),
        ShellWait::Answered(Err(ShellRefusal::Other(_) | ShellRefusal::NoHandler)) => {
            Err(OpenPathError::NotOpened { exit_code: None })
        }
        // No errno to carry: neither an apartment's `HRESULT` nor a gone thread is one.
        ShellWait::NoApartment(_) | ShellWait::NoThread => Err(OpenPathError::LaunchRefused { errno: None }),
        ShellWait::TimedOut => Err(OpenPathError::TimedOut),
    };
    if answer.is_err() {
        log::info!(
            target: "file_actions",
            "The Shell didn't open {}: {wait:?}",
            cmdr_fs::log_private::LogPrivate::path(&path)
        );
    }
    answer
}

/// The Shell's paths for the wire text `path`: [`OpenPathError::NoPlainPath`] for a path whose
/// plain form would name another file or runs too long, [`OpenPathError::NotOnThisMac`] for text
/// that names no Windows file at all (a scheme path).
fn shell_paths_of(path: &str) -> Result<ShellLaunchPaths, OpenPathError> {
    use cmdr_fs::app_path::NativeRefusal;

    let app_path = crate::commands::wire_path::app_path(path)
        .ok()
        .flatten()
        .ok_or(OpenPathError::NotOnThisMac)?;
    shell_launch_paths(&app_path).map_err(|why| match why {
        NativeRefusal::NotPlainSafe(_) => OpenPathError::NoPlainPath,
        _ => OpenPathError::NotOnThisMac,
    })
}

/// An ID list the Shell allocated for one item, freed on drop.
pub(super) struct ItemIdList(*mut ITEMIDLIST);

impl ItemIdList {
    /// The list, for a call that reads it while `self` lives.
    pub(super) fn as_ptr(&self) -> *const core::ffi::c_void {
        self.0.cast_const().cast()
    }

    /// The item at `path` (NUL-terminated), pinned: the Shell finds it now, by this exact name.
    pub(super) fn parse(path: &[u16]) -> Result<Self, ShellRefusal> {
        let mut item = std::ptr::null_mut();
        // SAFETY: `path` is NUL-terminated and outlives the call; no bind context, no attribute
        // query; `item` receives an ID list the Shell allocates, owned by `Self` from here.
        let parsed =
            unsafe { SHParseDisplayName(path.as_ptr(), std::ptr::null_mut(), &mut item, 0, std::ptr::null_mut()) };
        if parsed < 0 {
            return Err(ShellRefusal::of_hresult(parsed));
        }
        Ok(Self(item))
    }
}

impl Drop for ItemIdList {
    fn drop(&mut self) {
        // SAFETY: the Shell allocated this list with the COM allocator; it's freed once, here.
        unsafe { CoTaskMemFree(self.0.cast_const().cast()) };
    }
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

/// Opens the item at `paths.item` with its default app, as a double-click in File Explorer does:
/// pinned as an ID list, started in its own folder (`paths.folder`), and, for a file no app is set
/// to open, with Windows' own "How do you want to open this file?" picker. Call it inside an
/// apartment ([`in_apartment`]).
///
/// # Errors
///
/// [`ShellRefusal`]: the item is gone or unreachable (`SHParseDisplayName`), or the launch was
/// refused or cancelled (`ShellExecuteExW`).
pub(crate) fn open_with_default_app(paths: &ShellLaunchPaths) -> Result<(), ShellRefusal> {
    let item = ItemIdList::parse(&paths.item)?;
    let folder = paths.folder.as_deref();
    match execute(&item, folder, None) {
        Err(ERROR_NO_ASSOCIATION) => execute(&item, folder, Some(OPEN_AS)),
        answer => answer,
    }
    .map_err(ShellRefusal::Win32)
}

/// `ShellExecuteExW` on `item` with `verb` (`None`: the item's default), started in `folder`,
/// waiting until the Shell has handed it over (`SEE_MASK_NOASYNC`, so the thread can end right
/// after) and showing no error box of its own: the caller words the refusal.
fn execute(item: &ItemIdList, folder: Option<&[u16]>, verb: Option<&[u16]>) -> Result<(), u32> {
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.map_or(std::ptr::null(), <[u16]>::as_ptr),
        lpDirectory: folder.map_or(std::ptr::null(), <[u16]>::as_ptr),
        lpIDList: item.0.cast(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: `folder` and `verb` are NUL-terminated (the callers' contract), and they, `item`'s
    // ID list, and `info` all outlive the call; every other pointer in `info` is null.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: reads this thread's last error, which the failed call above just set.
        return Err(unsafe { GetLastError() });
    }
    Ok(())
}

/// Shows `path` (NUL-terminated) selected in File Explorer. A window already showing its folder
/// is reused; a drive root shows selected in "This PC". Call it inside an apartment.
///
/// # Errors
///
/// [`ShellRefusal`]: the item is gone or unreachable (`SHParseDisplayName`), or Explorer said no
/// (`SHOpenFolderAndSelectItems`).
pub(crate) fn reveal(path: &[u16]) -> Result<(), ShellRefusal> {
    let item = ItemIdList::parse(path)?;
    // SAFETY: `item` holds the fully qualified ID list parsed above, alive across the call; with
    // no children (`cidl` 0) the call opens the item's folder with the item selected.
    let shown = unsafe { SHOpenFolderAndSelectItems(item.0, 0, std::ptr::null(), 0) };
    if shown < 0 {
        return Err(ShellRefusal::of_hresult(shown));
    }
    Ok(())
}
