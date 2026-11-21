# notify 8.2.2, patched

The crates.io release of `notify` 8.2.2 (checksum `c41a04a5…205ab`, as `Cargo.lock` pinned it), swapped in through the
root `Cargo.toml`'s `[patch.crates-io]`. Everything here is the published package byte for byte, apart from the changes
below and the one file left out:

- `Cargo.lock`: a path dependency resolves through the workspace lockfile, so it was never read, and `osv` would scan
  its stale pins as if they shipped.

`fuzz/` keeps the crates.io copy: the fuzz targets build on Linux, where the patch changes nothing.

## The changes

All in `src/windows.rs` (the `ReadDirectoryChangesW` backend), each marked `CMDR-PATCH`, each a backport of what notify
9.0.0 does (upstream `notify-rs/notify` `main`, `notify/src/windows.rs`, read 2026-11-21). No other backend is touched,
so inotify and FSEvents builds are the release's code.

- **A buffer overflow reaches the handler** (#1128). Windows reports lost notification details as a zero-byte
  successful completion or `ERROR_NOTIFY_ENUM_DIR` ("enumerate the directory yourself"). 8.2.2 ignored the byte count,
  parsed its freshly zeroed buffer as one entry with action 0, and skipped it, so the loss reached nobody; on
  `ERROR_NOTIFY_ENUM_DIR` it logged "unknown error" and stopped watching. Now both re-arm and emit
  `Event::new(EventKind::Other).set_flag(Flag::Rescan)`, which the app's listing watch (`translate` in
  `apps/desktop/src-tauri/src/file_system/watcher/notify_stream.rs`) and the index
  (`crates/cmdr-index/src/indexing/watch/watcher/platform/notify_backend.rs`, `translate_notify_event`) already turn
  into a re-read.
- **A deleted watch root says so** (#1330). The pending read completes with `ERROR_ACCESS_DENIED` once the watched
  directory goes; 8.2.2 stopped watching only when the path no longer answered, and silently. Now a directory watch also
  recognizes the delete-pending handle (the name still answers until the last handle closes:
  `GetFileInformationByHandleEx(FileStandardInfo).DeletePending`) and emits `Remove(Folder)` on the watched path before
  stopping, so a pane learns its folder is gone.
- **A fatal read error reaches the handler**: any other completion error (a pulled drive, a dropped share) is emitted
  as an `Err` before the watch stops, where 8.2.2 only logged it.

Verified on Windows 10 22H2 (NTFS, the builder): see `docs/notes/windows-port/build-progress.md`, the task 35 listing
watch entry.

## Dropping it

When the workspace moves to notify 9.0 or later (a major bump with API changes the index's and the app's backends
both need), delete this folder, its `[patch.crates-io]` line, and its `exclude` entry. `desktop-rust-vendor-patch-applied`
fails as soon as a bump past 8.2.2 makes cargo resolve crates.io again.
