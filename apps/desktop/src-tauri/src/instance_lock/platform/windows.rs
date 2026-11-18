//! Windows: `MessageBoxW`, owned by no window, on a thread of its own while the main thread waits.
//!
//! A message box runs a modal loop that dispatches every message of the thread that calls it. On
//! the main thread that's Tauri's event loop (its hidden main window and thread-message target are
//! already there when `setup` runs), so a refused process could keep booting behind the box, the
//! failure macOS hit with `runModal`. On a thread of its own the box pumps only its own messages,
//! and the main thread is a plain blocking `join`. It has no timeout of its own; the caller's
//! watchdog ends the process if nobody clicks.

use std::time::Duration;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MESSAGEBOX_STYLE, MessageBoxW,
};

const STYLE: MESSAGEBOX_STYLE = MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST;

pub(crate) fn show_alert(title: &str, body: &str, _timeout: Duration) {
    let caption = wide(title);
    let text = wide(body);
    let shown = std::thread::Builder::new()
        .name("instance-lock-alert".to_string())
        .spawn(move || {
            // SAFETY: `text` and `caption` are NUL-terminated UTF-16 strings this closure owns for
            // the whole call, and a null owner window is documented as "no owner".
            let answer = unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), STYLE) };
            // Zero is failure; read its reason on this thread, where it was set.
            (answer == 0).then(std::io::Error::last_os_error)
        });
    match shown.map(std::thread::JoinHandle::join) {
        Ok(Ok(None)) => {}
        Ok(Ok(Some(error))) => super::super::to_stderr(format_args!(
            "Couldn't show the already-running alert (MessageBoxW: {error})."
        )),
        Ok(Err(_)) => super::super::to_stderr("The already-running alert's thread panicked."),
        Err(error) => super::super::to_stderr(format_args!(
            "Couldn't start the already-running alert's thread: {error}."
        )),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};

    /// The box really appears (a window with our caption exists), `show_alert` blocks until it's
    /// dismissed, and then returns. Closed the way Escape or the title bar's X would.
    #[test]
    fn the_alert_blocks_until_dismissed_then_returns() {
        let caption = format!("Cmdr test alert {}", std::process::id());
        let (done_tx, done_rx) = mpsc::channel();
        let shown_caption = caption.clone();
        std::thread::spawn(move || {
            show_alert(
                &shown_caption,
                "A test of the already-running alert.",
                Duration::from_secs(5),
            );
            let _ = done_tx.send(());
        });

        // `#32770` is the dialog class every message box uses.
        let class = wide("#32770");
        let title = wide(&caption);
        // SAFETY: both strings are NUL-terminated UTF-16 alive for every call.
        let find = || unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) };
        crate::test_support::wait_until(Duration::from_secs(10), "the message box to appear", || {
            !find().is_null()
        });
        let window = find();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "show_alert returned while its box was still up"
        );

        // SAFETY: `window` is the box's handle, just found; posting `WM_CLOSE` to it takes no
        // pointers.
        let posted = unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        assert_ne!(posted, 0, "PostMessageW");
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("show_alert returns once the box is dismissed");
    }
}
