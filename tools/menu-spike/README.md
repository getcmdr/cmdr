# Menu spike

A throwaway measuring tool for the Windows menu model (task 63 of `docs/specs/windows-port-plan.md`). It isn't part of
the app, no check compiles it, and it goes once the menu model is decided. The test script, the results table, and the
upstream facts are in `docs/notes/windows-port/menu-spike.md`.

It shows Cmdr's real Windows menu bar (from `crates/cmdr-menu`, English labels from the app's catalog) over a two-pane
page with a focused list and a text field, in three modes:

- **A. Native bar**: muda's Win32 menu bar through Tauri, with mnemonics (`&File`) and the bar's registered
  accelerators, as the app builds it today. The "Key bridge" switch forwards a lone Alt, F10, and Alt+letter from
  WebView2 to the bar.
- **B. Hybrid**: an in-page strip of menu titles (Alt or F10 focuses it, Alt+letter opens a menu, Left and Right walk
  it), each title opening a native popup menu under it.
- **C. In-page bar**: a page-drawn bar and dropdowns, unpolished.

Every mode has the native right-click menu (the app's real file context menu). The event log names the layer that heard
each key: `page keydown`, `webview2 accel` (what WebView2 tells the host), `host` (Windows' `SC_KEYMENU` and menu loop),
`menu event` (a native menu item or accelerator fired), `bridge`, and `in-page menu`.

## Run it

On Windows, double-click `menu-spike.exe`. It needs the WebView2 runtime, which Windows 11 and an updated Windows 10
have; nothing else is installed or written. Flags:

- `--mode native|hybrid|inpage` (or `a|b|c`): the starting mode. The page switches modes too.
- `--theme light|dark`: force the window's theme; without it, it follows Windows.
- `--bridge`: start with the key bridge on.
- `--no-browser-keys`: start with WebView2's browser keys (F5 reload, Alt+Left back) off.
- `--log <file>`: also write every log line to a file.
- `--selftest <dir|->`: type a fixed key script in every mode with `SendInput`, save a screenshot per step to `<dir>`,
  write the log (with `--log`), and quit.

## Build it

It's its own Cargo workspace, pinned to the app's Tauri versions (its `Cargo.lock` started as a copy of the root one).

```sh
cd tools/menu-spike
cargo build --release   # on Windows: target\release\menu-spike.exe
```

On a Mac, `cargo run` shows the page with a macOS bar, which is only useful for working on the page itself: the Windows
hooks are off there.
