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
- **A 64 KiB read buffer on the heap**: 8.2.2 read into a 16 KiB `[u8; N]` field of the request, which is neither big
  enough for one recursive watch over a whole drive (the index's, #1353) nor guaranteed the DWORD alignment
  `ReadDirectoryChangesW` requires. 9.0 reads 64 KiB into a `Box<[u32]>` (`read_buffer`); so does this copy. 64 KiB is
  also the most a network share accepts, so the listing watch on a share keeps working. Measured on the builder
  (Windows 10 22H2, NTFS, 2026-11-22, two `FileSystemWatcher`s on `C:\` with the same notify filter side by side):
  idle `C:` carried 0.2 events/s with no overflow at either size; copying and deleting 3,827 files (about 23,000
  events in 6 s) overflowed 16 KiB once and 64 KiB never, and so did a 2,671-file burst.

Verified on Windows 10 22H2 (NTFS, the builder, 2026-11-21) with a standalone program that arms a
`notify-debouncer-full` watch the way the app does: a flood of 20,000 files from four threads reported `Rescan` twice
on this copy and nothing at all on the crates.io 8.2.2; deleting the watched folder reported `Remove(Folder)` on it.
In the app: `docs/notes/windows-port/build-progress.md`, the task 35 listing watch entry.

## Dropping it

notify 9.0.0 shipped 2026-11-21 (with `notify-debouncer-full` 0.8.0), so it clears the 3-day release-age window on
2026-11-24. When the workspace moves to it (a major bump with API changes the index's and the app's backends both need,
so Renovate opens it as a major, not a routine bump), delete this folder, its `[patch.crates-io]` line, and its `exclude` entry. `desktop-rust-vendor-patch-applied`
fails as soon as a bump past 8.2.2 makes cargo resolve crates.io again. On that bump, re-check
`apps/desktop/src-tauri/src/file_system/watcher/notify_stream.rs`'s `close_now`: it leans on the Windows backend
answering `configure` on the same thread, after a queued `unwatch`.
