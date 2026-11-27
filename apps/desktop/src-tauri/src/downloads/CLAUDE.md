# Downloads watcher

Recursive `notify` watch on `~/Downloads` that emits a `download-detected` Tauri event when an eligible final-form file
appears (a direct create of a final name, or a rename from a partial suffix to one). FDA-gated.

## Module map

- **`filter.rs`**: `is_eligible(path)`: not hidden, no partial-download suffix, a file or a symlink to one
- **`ignore_set.rs`**: `IgnoreSet` (5 s TTL, 1000-entry cap) suppresses events for paths Cmdr just wrote
- **`latest_ring.rs`**: `LatestRing`, capacity 10; reads skip gone files
- **`watcher.rs`**: `DownloadsWatcher` (debouncer + ignore set + ring); pure `classify_event()` /
  `translate_debounced()`. Tests: `watcher_test.rs` and `links_test.rs` (a Downloads with links out of itself)
- **`runtime.rs`**: the process-wide watcher slot; `refresh_runtime(&app)` aligns it with the FDA gate; owns
  `note_pending_write_for_cmdr`
- **`commands.rs`**: IPC. **`global_shortcut.rs`**: `GlobalShortcutManager<R: Registrar>` over the plugin

Lifecycle, scope rationale, and limits: `DETAILS.md`.

## Must-knows

- **The watcher is alive iff `fda_gate::is_fda_pending_runtime() == false`.** `refresh_runtime` runs at startup, on
  every main-window `Focused(true)`, and from the Settings pane (`recheck_downloads_watcher_gate`).
- **Own writes: `note_pending_write_for_cmdr(&final_path)` right before each write syscall**, unconditionally (the root
  filter lives in `IgnoreSet`). ❌ Don't reduce its root check to one `starts_with` (declared AND resolved spellings).
  ❌ Never `canonicalize` a registered path: a network `realpath` stalls the write. § "Cmdr-own-write hook contract".
- **The debouncer runs `NoCache` and never follows links** (`arm_debouncer`). ❌ Don't go back to `new_debouncer` /
  `RecommendedCache`: its `FileIdMap` walks the tree through links, and a Wine bundle's `z: -> /` made that the whole
  disk and hung launch (#1538). § "Decision: no file-ID cache".
- **A macOS rename arrives as TWO direction-less halves** (`RenameAny`), a normal path: dropping it loses every browser
  download's toast. Eligibility tells the halves apart; ❌ no `exists()` probe. § "Reading a rename on macOS".
- **Don't `tokio::spawn` from the notify callback**: it runs on notify's thread with no Tokio runtime and panics. Use
  `tauri::async_runtime::spawn` if async is ever needed.
- **Hotkey: `⌘` maps to `Super`, NOT `Meta`** (`binding_to_accelerator` and FE mirror `global-shortcut-binding.ts`);
  `global-hotkey` rejects `META`. The handler raises the main window before emitting `global-shortcut-fired`; don't drop
  the raise. Register/unregister is idempotent, a refused swap restores the previous binding, and refusals are typed by
  the plugin's error arm, never its message.
- **No `println!` / `dbg!`**: `log::debug!(target: "downloads::watcher", …)`.
- **Tests use `DownloadsWatcher::scripted_at` + `deliver()`**: same handler, no OS watch. ❌ Only
  `dropping_a_file_emits_one_event` arms real FSEvents; a second real cell times out under load. § "Testing".
