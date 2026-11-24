# File system module

Sync status, volume management, and file watching. The engines are crates: listing `crates/cmdr-listing`, writes
`crates/cmdr-operations`.

## Module map

- Submodules with their own docs: `volume/`, `sync_status/` (cloud badges), `restic/`. Around them, a dozen leaves (`watcher.rs`, `google_drive/`, `share.rs`, …), one line each in DETAILS § Module map.
- `mod.rs` is a facade: it re-exports downward and bootstraps the volume registry (`init_volume_manager`), which is why
  it may know every backend.

## Gotchas

- **Reach the volume manager at `volume::manager::get_volume_manager()`; ❌ never re-export it here**: that welds the
  subtree into one cycle. `volume/DETAILS.md` § "Key decisions".
- **Transient scratch hides on the listing READ path only** (`CachedListing::rows`): a watcher-side skip strands an
  entry forever. Cmdr's own (`.cmdr-tmp-*`) hides by OWNERSHIP, others' (`.sb-`) by NAME. § "Hiding transient scratch".
- **Tag writes touch ONLY `_kMDItemUserTags`** (`FinderInfo` holds folder icons), as **binary** plist, through the
  volume (`tags_io`), merging via `read_for_merge`. **Clearing writes the empty array, ❌ never deletes**: a QNAP keeps
  what it's told to delete. § "Finder tags".
- **Never call macOS frameworks from rayon or any constrained-stack pool**: FileProvider XPC blows the 2 MB stack and
  can block forever. Use pooled, hard-capped 8 MB OS threads (`framework_pool.rs`). § "Threading".
- **Watcher rules** (each has a `DETAILS.md` section; read them before touching `watcher.rs`):
  - ❌ Never `tokio::spawn` from a watcher OS thread (no reactor: it panics). Use `tauri::async_runtime::spawn`.
  - Arm with `start_watching_detached`, ❌ never `start_watching` (p90 653 ms inline); `arm_missed_a_change` (mtime)
    gates the catch-up re-read, ❌ never an unconditional one.
  - Rebase event paths (`rebase_event_path`), or firmlinks and a symlinked root drop every event.
  - A `Structural` change on the ROOT re-reads; ❌ not a root `Content` change (every child change bumps its mtime).
  - macOS uses `cmdr-fsevent-stream`, ❌ never `notify`: it dropped lost-history flags anchored ABOVE the folder.
    § "Lost history".
  - A Windows watch blocks renames above its folder; Cmdr's writes release it (`watcher/release.rs`).
  - Test builds arm no OS stream unless `watch_for_real`. § "Tests arm no OS stream".
  - A re-sorted row is one `DiffChangeType::Move`, ❌ never a remove plus an add (cursor and selection ride along by
    identity). § "Reordered rows".
- **A watch on an OS-mounted network share is `WatchCoverage::ThisMachineOnly`, never `EveryWriter`** (FSEvents on
  `smbfs` sees this machine's writes only). Arm it anyway, but it must never let a delete walker or copy scan skip a
  read. `volume/DETAILS.md` § "Trait capability model".
- **"Open terminal here" probes known apps, ❌ never scans a folder.** Windows: the folder is the working directory, ❌
  never argv (`wt` splits on `;`). § "Open terminal here".
- **F4's editors come from LaunchServices' editor-ROLE plain-text query** (`text_editor/`): ❌ never the UTI crate
  (10.15 floor) or a made-up `.txt` URL (no apps). § "Text editor".
- **Windows' Open with and F4 use the Shell's `IAssocHandler`s** (`../platform/windows/assoc.rs`): `app_path` holds the
  handler's id, ❌ not a file; files go as a data object, ❌ never a command line. § "Open with on Windows".
- **`cloud_actions.rs` is iCloud Drive only**, gated by `CloudProvider::supports_eviction`; the cross-provider-looking
  `NSFileProviderManager` methods need the bundled extension. Don't widen it. Provider identity lives once, in
  `cmdr_fs::cloud_provider`, which the volume switcher reads too.
- **Google Drive (`google_drive/`), three ❌:** never gate on a path prefix (mirror mode keeps real files outside
  `~/Library/CloudStorage` with no xattr: gate on a resolved item ID); never resolve a mirrored file by NAME; never read
  a mirror `stable_id` from the stream-mode `metadata_sqlite_db` (different id space). Read-only; every failure means
  no menu item, since a wrong link points at someone else's file. § "Mirror mode".

Read `DETAILS.md` before non-trivial work here: open-with, cloud actions, threading, and watchers.
