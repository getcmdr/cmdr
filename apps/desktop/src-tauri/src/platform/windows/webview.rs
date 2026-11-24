//! What every WebView2 webview gets before it's used: the browser accelerator keys off (#1115,
//! task 67 of `docs/specs/windows-port-plan.md`), its window guarded against Windows' own menu
//! loop (`keymenu.rs`, task 63). Its drop target is the drag seam's (`tauri_builder.rs`).
//!
//! WebView2 handles its own browser keys (`F5` and `Ctrl+R` reload, `Ctrl+F` / `F3` find,
//! `Ctrl+P` print, `Ctrl+J` downloads, `Alt+←/→` history, `F7` caret browsing, zoom, DevTools)
//! before, or instead of, the page. Cmdr binds several of them (`Ctrl+R` refresh, `Ctrl+J` latest
//! download, `Alt+←/→` back and forward), and in a window or state that doesn't claim a key
//! (Settings, the viewer, a text field) WebView2's action would run. `AreBrowserAcceleratorKeysEnabled`
//! false turns those off and still delivers each key to the page; it keeps the editing and
//! movement keys (`Ctrl+X/C/V`, `Ctrl+A`, `Ctrl+Z`, `Home`/`End`, `Page Up`/`Down`), so text
//! fields keep copy, paste, undo, and select-all (Microsoft Learn, `ICoreWebView2Settings3`,
//! `ms.date` 2026-10-19, read 2026-11-18).
//!
//! Tauri 2.12.5 doesn't expose the switch (tauri-apps/tauri#16397, which adds
//! `browserAcceleratorKeys`, was still open on 2026-11-18), so this reaches the core webview's
//! settings through `with_webview`, from the `cmdr-platform` plugin's `on_webview_ready`, which
//! every webview passes, the ones the frontend creates included.

use tauri::{Webview, Wry};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
use windows_core::Interface;

/// Turns WebView2's browser accelerator keys off for `webview`. A failure is logged, never
/// fatal: the window still works, it only keeps WebView2's own keys.
pub(crate) fn prepare_webview(webview: &Webview<Wry>) {
    super::keymenu::install(webview);
    let label = webview.label().to_owned();
    let dispatched = webview.with_webview(move |platform| {
        // SAFETY: `controller()` is the live `ICoreWebView2Controller` Tauri hands this closure on
        // the webview's own (UI) thread, which is the thread WebView2's COM objects must be called
        // from; each call below is a plain COM getter or setter on an interface obtained from it,
        // and every result is checked before the next call.
        let applied = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(false))
        };
        match applied {
            // allowed-log-private: a Tauri window label of our own
            Ok(()) => log::debug!(target: "platform::webview", "browser keys off in `{label}`"),
            // allowed-log-private: a Tauri window label of our own
            Err(e) => log::warn!(target: "platform::webview", "couldn't turn browser keys off in `{label}`: {e}"),
        }
    });
    if let Err(e) = dispatched {
        log::warn!(target: "platform::webview", "couldn't reach the webview to turn browser keys off: {e}");
    }
}
