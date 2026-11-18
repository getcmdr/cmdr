//! macOS: `CFUserNotificationDisplayAlert`. The dialog is drawn by the system's
//! `UserNotificationCenter` agent, so our call is a plain blocking wait on this thread, and it
//! carries its own timeout.
//!
//! ❌ Not `NSAlert::runModal`: it pumps OUR main run loop, so a refused process kept booting behind
//! the unanswered alert: webview and frontend up, updater check fired, files written into the data
//! dir the other instance owns, then an abort when the frontend called a menu command while
//! `setup` was still parked in the modal.

use std::time::Duration;

pub(crate) fn show_alert(title: &str, body: &str, timeout: Duration) {
    use objc2_core_foundation::{CFOptionFlags, CFString, CFUserNotification, kCFUserNotificationCautionAlertLevel};

    let header = CFString::from_str(title);
    let message = CFString::from_str(body);
    let mut response: CFOptionFlags = 0;
    // SAFETY: every pointer argument is either `None` (the icon, sound, localization, and button
    // titles are documented as nullable, meaning "system defaults", which is the single "OK" button
    // we want) or a live `CFString` we own for the whole call, and `response_flags` points at a
    // stack local that outlives the call. The call blocks this thread, so both borrows stay valid.
    let err = unsafe {
        CFUserNotification::display_alert(
            timeout.as_secs_f64(),
            kCFUserNotificationCautionAlertLevel,
            None,
            None,
            None,
            Some(&header),
            Some(&message),
            None,
            None,
            None,
            &mut response,
        )
    };
    if err != 0 {
        super::super::to_stderr(format_args!(
            "Couldn't show the already-running alert (CFUserNotification error {err})."
        ));
    }
}
