# Shell progress

The write queue's work as ONE bar the OS shell draws: over the Dock tile (macOS), on the main window's taskbar button
(Windows), on the launcher entry (Linux, where the desktop reads Unity's `LauncherEntry`). Backend-only: the frontend
never sees it.

## Module map

- `mod.rs`: `install` (managed hub + worker thread), the `note_*` feed, the throttled worker.
- `aggregate.rs`: pure fold of snapshots and ticks into a `ShellProgress` (+ `aggregate_tests.rs`).
- `platform/`: the seam, one `bar_state` mapping per OS onto Tauri's `set_progress_bar` (+ `tests.rs`, which loads all
  three arms on any host).

## Must-knows

- **`operations_host` is the only feeder**: `TauriEngineEvents::operations_changed` and `scan_wait_progress`,
  `TauriEventSink::emit_progress` and `emit_complete`. A test binary has no app handle there and never reaches it; a
  call before `install` is a no-op (no managed hub).
- **Membership comes from the snapshot, never from ticks.** A tick for an unlisted operation is dropped, so a late one
  can't resurrect a gone operation. `write-complete` precedes the snapshot that drops the op (the engine's `on_settled`
  runs after the terminal event); an op that leaves with no `write-complete` counts as cancelled.
- **Honesty rules live in `aggregate.rs`**: unmeasured = indeterminate, capped at 99 % while work is listed, paused only
  when a PERSON holds every started op, error while the queue keeps a failure. Change them there, with a test.
- **Each arm sets both status and value**: tao keeps the old value when one is `None`.
- **No polling**: the worker blocks on a channel woken only when the summary changes, and spaces OS updates by
  `MIN_UPDATE_GAP`.
- Exit needs no cleanup: the bar belongs to the process (macOS Dock tile, Windows window, Unity's D-Bus name).

The aggregation rules with their whys, each OS's look, and the QA recipe: `DETAILS.md`.
