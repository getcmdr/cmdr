# Clipboard

File clipboard (Cmd+C/X/V, Ctrl+C/X/V) with Finder's NSPasteboard on macOS and Explorer's `CF_HDROP` on Windows. The
move-vs-copy decision happens at paste time: a cut is Cmdr's own flag on macOS (`state.rs`), and the clipboard's
`Preferred DropEffect` on Windows, so Explorer and Cmdr move each other's cuts.

## Module map

- `mod.rs`: re-exports, the shared types (`ClipboardIntent`, `ClipboardFiles`, `ClipboardFilesError`), and the `#[cfg]`
  switch between the prod NSPasteboard backend and the E2E mock.
- `platform/`: each OS's seam. `macos.rs` (main-thread hop), `windows.rs` (`CF_HDROP` + drop effect),
  `linux.rs`/`unsupported.rs`, `arboard_text.rs` (plain text on Linux and Windows), and `shell_formats.rs` (pure bytes
  of the Windows formats: `DROPFILES`, `DROPEFFECT`, the text; tested on every OS).
- `cut_paste.rs`: follows a paste of a clipboard-marked cut from the read to the move's end.
- `pasteboard.rs` / `mock.rs` / `store.rs`: macOS FFI, its E2E mock, and their shared statics.
- `payload.rs` / `paste_file.rs`: paste content (text, image, PDF) as a new file. `state.rs`: Cmdr's own cut flag.

## Gotchas

- **NSPasteboard is NOT thread-safe**: all pasteboard calls dispatch to the main thread via `run_on_main_thread`.
- **The Windows clipboard is a system-wide lock another app can hold**: `windows.rs` retries a bounded number of times,
  and the commands run every seam call in `spawn_blocking`, ❌ never on the main thread or the async runtime.
- **Clipboard paths cross as PLAIN native paths** (`to_native_plain`): other apps read `CF_HDROP` without `\\?\`. A row
  whose plain form names another file (long path, trailing dot, `CON`) refuses the whole set as `NoPlainPath`.
- **Paths come back as app paths** (`C:\x` → `C:/x`) in `read_clipboard_files`, so the move the paste starts names
  what `cut_paste.rs` noted. ❌ Don't hand the frontend native text.
- **A paste-as-move of a marked cut owes its source a word**: `move_files` / `move_between_volumes` claim it,
  `operations_host::terminal_effects` settles it. Windows then sends `Performed DropEffect` = NONE and `Paste
  Succeeded` = MOVE (❌ never `Performed DropEffect` = MOVE: that asks Explorer to delete) and empties a cut nobody else
  ended. `DETAILS.md` § Windows.
- **Paths coming IN pass the plain-form rule too**: a plain `C:\x\a.` or `C:\x\CON` from another app refuses the whole
  paste (`UnreadablePath`), ❌ never a subset. A refused cut disarms the earlier one, so Ctrl+V never moves stale files.
- **Cut state lives in Rust, not the frontend**. A clipboard that no longer matches the cut-state paths clears the
  stale state and pastes as a copy, so the wrong files never move.
- **Both files and plain text are written**, so pasting into a text editor gives the paths, one per line.
- **Scheme paths (MTP, SFTP, archives, Cmdr's own SMB) never reach the clipboard**: the frontend refuses them and
  suggests F5/F6; `native_rows` refuses them too.
- **Paste-content-as-file uses a SEPARATE `ClipboardData` static from the file-URL `ClipboardEntry`.** Don't merge
  them: a content paste would clobber a pending file copy. `DETAILS.md` § Paste clipboard content as a file.
- **Runtime `CMDR_CLIPBOARD_BACKEND=mock`** lives in `pasteboard.rs` (macOS, a prod-build debugging tool), sampled
  once via `LazyLock`; see the "Mock-backend convention" in `docs/tooling/instance-isolation.md`.

Full details (why `objc2` over a Tauri plugin, why the plain Windows clipboard API over `OleSetClipboard`, the E2E mock
as a `cfg` swap, the cut handshake): `DETAILS.md`.
