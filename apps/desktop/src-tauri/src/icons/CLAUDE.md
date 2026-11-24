# Icons module

OS icon retrieval and caching for the file list. Entries carry only an `iconId`; the FE batches the unique ids for
visible rows and fetches each once via `get_icons`, so 50k files never transmit 50k icon blobs. (`src-tauri/icons/` at
the crate root holds the app *bundle* icons, unrelated.)

## Icon-id scheme

`get_icon_id` (`crates/cmdr-fs/src/entry.rs`, next to `FileEntry`) assigns each entry an id; `get_icons` resolves it to
a base64 WebP data URL. By tier:

| Tier | Id | Assigned to | Fetched from |
| --- | --- | --- | --- |
| A | `dir` / `symlink-dir` | every plain folder (~99%) | a temp sample folder |
| A | `ext:{x}` / `file` / `symlink*` | files | a per-extension temp sample / `/etc/hosts` |
| B | `special:{name}` | the finite special system folders | the folder's REAL path (8 MB thread) |
| C | `path:{dir}` / `pkg:{dir}` | per-path icons (volumes, packages, custom-icon folders) | the real path (8 MB thread) |
| C | `path:{file}` | Windows files with their own icon (`.exe`, `.lnk`, `.ico`, …) | the real file (8 MB thread) |
| n/a | `git:{branch,tag,commit,fork}` | git-portal virtual entries | rendered by the FE via Lucide, never here |

Full details: `DETAILS.md`.

## Must-knows

- **The per-entry classifiers live in `cmdr-fs`** (`special_folders`, packages, `own_icons`): they run inside
  `FileEntry::new`, so anything reachable from `get_icon_id` stays pure, with no disk I/O. ❌ Never a `canonicalize`.
- **Custom-icon detection (`getxattr`) must NOT run during bulk listing**: only for the visible paths the FE asks
  about via `get_custom_folder_icon_ids`. So a custom-icon folder KEEPS the `dir` id: the FE resolves it by PATH
  (`getCachedCustomFolderIcon`), ❌ never `iconId`, or the icon is fetched and never drawn.
- **Gold recolors a custom icon only if `followsAccent`**; an emoji folder is gold `dir` plus the emoji on top.
- **`dir` / `symlink-dir` sample a Cmdr-owned empty temp folder, ❌ never `~`**: macOS bakes the home folder's house
  badge into the bitmap.
- **Real-place fetches run on 8 MB-stack OS threads, never rayon**, and in `get_icons` AFTER the type ids, through
  `per_place_fetch` (bounded wait, capped in flight): one slow place must cost only its own icon, ❌ never the batch.
- **All NSWorkspace fetches are FDA-gated in `commands/icons.rs`**; the FE re-requests once the gate clears.
- **Bounded vs unbounded key lifecycle**: `dir` / `ext:*` / `file` / `symlink*` / `special:*` persist to localStorage,
  so changing how one is PRODUCED needs a `CACHE_SCHEMA` bump in `#lib/icon-cache`. `path:*` / `pkg:*` are
  `PATH_KEY_CAP` LRU, never persisted (`is_per_path_key`).
- **System-only changes need explicit flushes**: theme/accent drops the folder keys and the disk tier; an
  `IconVariant` change drops all memory keys, and the disk tier keys by it.
- **Every cache write is epoch-guarded** (`cache_icon(epoch, ..)`, FE `applyIconsToCache(.., epoch)`), epoch taken
  BEFORE asking the OS. `DETAILS.md` § Flushes.
- **Disk-cache staleness token is the place's mtime**, ❌ not a watcher: Finder bumps it when re-iconing.
- **Windows asks a real place only if `may_read_own_icon` says it's local and holds its bytes**, and a `.lnk` only if
  its icon location and target are too; else its type's icon stands in, memory only, ❌ never the disk tier. ❌ No
  `.url`. `DETAILS.md` § Asking Windows for the pixels.
- **Providers live in `platform/`**; on Linux `special:*` degrades to the generic folder. The macOS fetch is ours
  (`platform/macos/workspace.rs`), not a crate: a crate can hard-link a framework newer than our floor.
