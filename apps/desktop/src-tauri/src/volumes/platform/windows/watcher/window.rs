//! The drive watcher's window: hears Windows and hands everything to the worker.
//!
//! Four signals, one hidden top-level window on its own thread (the pattern
//! `system_look/platform/windows/setting_changes.rs` set):
//!
//! - `WM_DEVICECHANGE` with a `DBT_DEVTYP_VOLUME` (broadcast): a letter arrived or left.
//! - `WM_DEVICECHANGE` with a `DBT_DEVTYP_HANDLE` (sent to the worker's registrations): a
//!   removal or lock query, its refusal, and the device going.
//! - `WM_SETTINGCHANGE` (broadcast): `DefineDosDevice` sends it for a `subst` letter.
//! - The Shell's change notice (`SHChangeNotifyRegister`, `WM_SHELL_DRIVES`): a drive or share
//!   mapped or unmapped through the Shell, media inserted or removed.
//!
//! An arrival or removal settles for [`SETTLE_MS`] into ONE re-read, since one plug brings a
//! burst (several broadcasts plus the Shell's notice). A handle event goes to the worker at
//! once. ❌ Nothing here reads a drive: a dead network drive would hang the procedure, and
//! with it Windows' own broadcast to every other window.

use std::cell::Cell;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::time::Duration;

use cmdr_fs::volume::windows_device_change::{self as change, DeviceChange, HandleEvent};
use log::{debug, warn};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
use windows_sys::Win32::UI::Shell::{
    CSIDL_DESKTOP, ILFree, SHCNE_DRIVEADD, SHCNE_DRIVEREMOVED, SHCNE_MEDIAINSERTED, SHCNE_MEDIAREMOVED,
    SHCNRF_InterruptLevel, SHCNRF_NewDelivery, SHCNRF_ShellLevel, SHChangeNotification_Lock,
    SHChangeNotification_Unlock, SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister,
    SHGetSpecialFolderLocation,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEV_BROADCAST_HDR, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetMessageW,
    GetWindowLongPtrW, KillTimer, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, SetWindowLongPtrW,
    WM_APP, WM_CLOSE, WM_DESTROY, WM_DEVICECHANGE, WM_NCDESTROY, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
};

use super::Job;

/// Posted by the Shell for a drive or media change.
const WM_SHELL_DRIVES: u32 = WM_APP + 1;

/// How long a burst of arrival and removal signals settles before its one re-read.
pub(super) const SETTLE_MS: u32 = 250;
/// The settle timer's id, the window's only timer.
const SETTLE_TIMER: usize = 1;

/// How long a removal or lock query waits for the worker to let go before the window answers
/// Windows anyway. The removal waits on this answer, and a handle still open then makes
/// Windows refuse it honestly ("the device is in use"), which comes back as a take-back.
const LET_GO_WAIT: Duration = Duration::from_secs(8);

/// `ERROR_CLASS_ALREADY_EXISTS`: a second watch in one process (a test) reuses the class.
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

/// The window's own state, owned by the window (`GWLP_USERDATA`) and only touched on its thread.
struct WindowState {
    jobs: Sender<Job>,
    settling: Cell<bool>,
}

/// UTF-16, NUL-terminated, for a `PCWSTR`.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> u32 {
    // SAFETY: reads this thread's last-error value, right after the call that set it.
    unsafe { GetLastError() }
}

/// Creates the window, subscribes to the Shell's drive notices, answers `ready` with the
/// window (or why there's none), and pumps messages until the window closes.
pub(super) fn run(jobs: Sender<Job>, ready: SyncSender<Result<isize, String>>) {
    // SAFETY: initializes COM on this thread, which the Shell's change registration may need;
    // balanced by `CoUninitialize` below, on this same thread. A failure leaves COM as it was.
    let com = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;
    let window = match create_window(jobs) {
        Ok(window) => window,
        Err(e) => {
            // the starter is waiting; if it isn't, nobody needs it
            let _ = ready.send(Err(e));
            if com {
                // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
                unsafe { CoUninitialize() };
            }
            return;
        }
    };
    let shell = ShellNotice::register(window);
    // the starter is waiting; if it isn't, the window still closes
    let _ = ready.send(Ok(window as isize));
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a live `MSG` this loop owns; a null window takes every message
        // posted or sent to this thread's windows.
        let got = unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) };
        // 0 is `WM_QUIT`, -1 a failure: either way the loop is over.
        if got == 0 || got == -1 {
            break;
        }
        // SAFETY: `message` was filled by `GetMessageW` just above.
        unsafe { DispatchMessageW(&raw const message) };
    }
    drop(shell);
    if com {
        // SAFETY: balances the successful `CoInitializeEx` above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

/// The hidden top-level window, its state attached.
fn create_window(jobs: Sender<Job>) -> Result<HWND, String> {
    let class_name = wide("CmdrDriveEvents");
    // SAFETY: a null name asks for the module that started this process, loaded for the
    // process's whole life; the handle needs no release.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..WNDCLASSW::default()
    };
    // SAFETY: `class` is a fully initialized `WNDCLASSW` (the rest zeroed, meaning "none"), and
    // its name points into `class_name`, which outlives the call; Windows copies the class.
    if unsafe { RegisterClassW(&raw const class) } == 0 {
        let code = last_error();
        if code != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!(
                "couldn't register the drive events window class (error {code})"
            ));
        }
    }
    // SAFETY: the class is registered above under this name. No parent and no `WS_VISIBLE`: a
    // hidden top-level window, which receives broadcasts. No creation parameter.
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if window.is_null() {
        return Err(format!(
            "couldn't create the drive events window (error {})",
            last_error()
        ));
    }
    let state = Box::into_raw(Box::new(WindowState {
        jobs,
        settling: Cell::new(false),
    }));
    // SAFETY: `window` is this thread's own window, just created. The pointer is a leaked box
    // that `WM_NCDESTROY` takes back, the last message the window gets.
    unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, state as isize) };
    Ok(window)
}

/// Closes the window from any thread: its thread destroys it and ends its loop.
pub(super) fn close(window: isize) {
    // SAFETY: `PostMessageW` may be called from any thread, and the message carries no
    // pointers. A window already gone fails the call, which is fine.
    unsafe { PostMessageW(window as HWND, WM_CLOSE, 0, 0) };
}

/// The Shell's drive and media notices for the window, deregistered on drop.
struct ShellNotice {
    id: u32,
    desktop: *mut ITEMIDLIST,
}

impl ShellNotice {
    fn register(window: HWND) -> Option<Self> {
        let mut desktop: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `desktop` is a live out-pointer; the Shell allocates the item list, which
        // `Drop` frees with `ILFree`.
        let found = unsafe { SHGetSpecialFolderLocation(std::ptr::null_mut(), CSIDL_DESKTOP as i32, &raw mut desktop) };
        if found < 0 || desktop.is_null() {
            warn!(target: "volumes::windows", "No Shell drive notices: the desktop's item list wouldn't come (HRESULT {found:#x})");
            return None;
        }
        let entry = SHChangeNotifyEntry {
            pidl: desktop,
            fRecursive: 1,
        };
        let events = SHCNE_DRIVEADD | SHCNE_DRIVEREMOVED | SHCNE_MEDIAINSERTED | SHCNE_MEDIAREMOVED;
        // SAFETY: `window` is this thread's window, which pumps the posted message; `entry`
        // points at the desktop list, alive until `Drop`, and the Shell copies the entry.
        let id = unsafe {
            SHChangeNotifyRegister(
                window,
                SHCNRF_ShellLevel | SHCNRF_InterruptLevel | SHCNRF_NewDelivery,
                events as i32,
                WM_SHELL_DRIVES,
                1,
                &raw const entry,
            )
        };
        if id == 0 {
            warn!(target: "volumes::windows", "No Shell drive notices: the registration was refused");
            // SAFETY: the list the Shell allocated above, freed once.
            unsafe { ILFree(desktop) };
            return None;
        }
        Some(Self { id, desktop })
    }
}

impl Drop for ShellNotice {
    fn drop(&mut self) {
        // SAFETY: `id` is this registration, ended once; `desktop` the list it was made with,
        // freed once after it.
        unsafe {
            SHChangeNotifyDeregister(self.id);
            ILFree(self.desktop);
        }
    }
}

/// The events whose `lParam` points at a `DEV_BROADCAST_HDR` this module reads.
fn carries_a_broadcast(event: usize) -> bool {
    matches!(
        event,
        change::DBT_DEVICEARRIVAL
            | change::DBT_DEVICEQUERYREMOVE
            | change::DBT_DEVICEQUERYREMOVEFAILED
            | change::DBT_DEVICEREMOVEPENDING
            | change::DBT_DEVICEREMOVECOMPLETE
            | change::DBT_CUSTOMEVENT
    )
}

/// The longest structure read: `DEV_BROADCAST_HANDLE` plus a custom event's data. A header
/// claiming more is read as nothing.
const MAX_BROADCAST: usize = 4096;

/// The window's procedure. ❌ Nothing here may panic or read a drive: it runs across the FFI
/// boundary, and a broadcast waits on it.
unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the window's own long, set right after creation to its state box (null before).
    let state = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const WindowState;
    if state.is_null() {
        // SAFETY: hands this window's own message, unchanged, to the default handler.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    if message == WM_NCDESTROY {
        // SAFETY: the state box `create_window` leaked, taken back once: the long is cleared
        // first, so no later message sees it.
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            drop(Box::from_raw(state.cast_mut()));
            return DefWindowProcW(window, message, wparam, lparam);
        }
    }
    // SAFETY: the box lives until `WM_NCDESTROY` above, and only this thread touches it.
    let state = unsafe { &*state };
    match message {
        WM_DEVICECHANGE => {
            let broadcast = (carries_a_broadcast(wparam) && lparam != 0).then(|| {
                // SAFETY: for these events `lParam` points at a `DEV_BROADCAST_HDR` whose
                // `dbch_size` is the whole structure's length (`dbt.h`), valid for the call.
                let size = unsafe { (*(lparam as *const DEV_BROADCAST_HDR)).dbch_size } as usize;
                let size = if (12..=MAX_BROADCAST).contains(&size) { size } else { 0 };
                // SAFETY: the structure is `size` bytes, read only during this call.
                unsafe { std::slice::from_raw_parts(lparam as *const u8, size) }
            });
            if let Some(answer) = on_device_change(window, state, wparam, broadcast) {
                return answer;
            }
        }
        WM_SETTINGCHANGE => settle(window, state),
        WM_SHELL_DRIVES => {
            let mut items: *mut *mut ITEMIDLIST = std::ptr::null_mut();
            let mut event = 0i32;
            // SAFETY: with `SHCNRF_NewDelivery`, `wParam` and `lParam` are the change's shared
            // memory and the sender's process id, to be locked and unlocked once each.
            let lock =
                unsafe { SHChangeNotification_Lock(wparam as HANDLE, lparam as u32, &raw mut items, &raw mut event) };
            if !lock.is_null() {
                // SAFETY: the lock taken just above, released once.
                unsafe { SHChangeNotification_Unlock(lock) };
            }
            debug!(target: "volumes::windows", "Shell drive notice {event:#x}");
            settle(window, state);
            return 0;
        }
        WM_TIMER if wparam == SETTLE_TIMER => {
            // SAFETY: this window's own timer, on its own thread.
            unsafe { KillTimer(window, SETTLE_TIMER) };
            state.settling.set(false);
            send(state, Job::Rescan);
            return 0;
        }
        WM_CLOSE => {
            // SAFETY: this window, on its own thread.
            unsafe { DestroyWindow(window) };
            return 0;
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        _ => {}
    }
    // SAFETY: hands this window's own message, unchanged, to the default handler.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// What a `WM_DEVICECHANGE` does; `Some` answer when it's this module's to give (`TRUE`, which
/// grants a query: whatever Cmdr held is let go by then, or as far as the wait allowed).
fn on_device_change(window: HWND, state: &WindowState, event: usize, broadcast: Option<&[u8]>) -> Option<LRESULT> {
    match change::decode(event, broadcast) {
        DeviceChange::Volumes {
            event,
            letters,
            media,
            network,
        } => {
            debug!(target: "volumes::windows", "Volume {event:?}: {letters:?} (media {media}, network {network})");
            settle(window, state);
            Some(1)
        }
        DeviceChange::Handle {
            event: HandleEvent::LetGo,
            registration,
        } => {
            let (done, answered) = mpsc::sync_channel(1);
            send(state, Job::LetGo { registration, done });
            if answered.recv_timeout(LET_GO_WAIT).is_err() {
                warn!(
                    target: "volumes::windows",
                    "Letting go of a drive took over {} s; answering Windows anyway",
                    LET_GO_WAIT.as_secs()
                );
            }
            Some(1)
        }
        DeviceChange::Handle {
            event: HandleEvent::TakeBack,
            registration,
        } => {
            send(state, Job::TakeBack { registration });
            Some(1)
        }
        DeviceChange::Handle {
            event: HandleEvent::Gone,
            registration,
        } => {
            send(state, Job::Gone { registration });
            Some(1)
        }
        DeviceChange::Other => None,
    }
}

/// Opens a burst (arms the settle timer) or joins the open one.
fn settle(window: HWND, state: &WindowState) {
    if state.settling.replace(true) {
        return;
    }
    // SAFETY: this window, on its own thread; no timer procedure, so it arrives as `WM_TIMER`.
    if unsafe { SetTimer(window, SETTLE_TIMER, SETTLE_MS, None) } == 0 {
        warn!(target: "volumes::windows", "Couldn't arm the settle timer (error {}), re-reading now", last_error());
        state.settling.set(false);
        send(state, Job::Rescan);
    }
}

fn send(state: &WindowState, job: Job) {
    if state.jobs.send(job).is_err() {
        debug!(target: "volumes::windows", "The drive events worker has ended; dropping a signal");
    }
}
