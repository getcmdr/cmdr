# cmdr-local

The local disk: `LocalPosixVolume` (the `Volume` over a `std::fs` root) and its Windows sibling `LocalWindowsVolume`,
the local reader, the one no-replace rename, local volume space, and the mount probes. Tauri-free. Callers name the
alias `LocalVolume`; only OS-dependent code names a sibling (`platform/mod.rs`).

## Module map

- **platform/** picks the volume per OS family: **local_posix.rs** with **local_posix/** (`scan`, `links`, `xattrs`,
  one-line delegations), **local_windows.rs** with **local_windows/** (`paths`, the `\\?\` crossing; `rename`, `links`,
  `scan`), and the pure Windows rules **anchor.rs**, **reparse.rs**, **short_names.rs**. **streams.rs**,
  **copy_scan.rs**, and **item_metadata.rs**: both siblings' byte movement, copy scan, and landed metadata.
- **reading.rs** (the local reader; the watcher and `tags.rs` read through it) with **reading/row.rs** (row paths) and
  **reading/bulk.rs** (macOS `getattrlistbulk`), **mutation.rs** (a mutation's listing patch), **space.rs** +
  **live_space.rs**, the OS probes (**filesystem_kind.rs**, **mount_table.rs**, **statfs.rs**, **nsurl.rs**,
  **linux_mounts.rs**), and **mounts/**, the one door to the mount table.

## Must-knows

- **It reaches the app only through its `VolumeHost`** (`host.listings()`: pane patches, watch coverage, name order), a
  required constructor argument. ❌ Never default it to `VolumeHost::detached()` in production: the scan oracle's fast
  path turns off and panes go stale after every local copy. ❌ Never depend on `cmdr-listing` or the app.
- **❗ On Windows, `std::fs::rename` REPLACES**: forced path only.
- **One crossing per OS**: a body turns its `&AppPath` into the OS path once (`os_path`, `locate`: `to_native`), and
  rows join by `AppPath::join`; ❌ no `Path::join` on an app path. A bridge value still reaches the POSIX OS as its raw
  text (`lstat("link/")` follows the link). DETAILS § "App paths in and out".
- **`rename_local_exclusive` is the ONE no-replace rename**, and it degrades on a filesystem with no kernel flag (smbfs
  answers `ENOTSUP`). ❌ Never write a second `renamex_np` call: while `overwrite::rename_no_replace` was a separate
  copy, only it degraded, and every rename on a mounted SMB share failed (ERR-8RFN4). DETAILS § "Local renames are
  atomic-exclusive".
- **`write_from_stream` hands each file to its drive; `flush_writes` makes it durable.** The per-file sync is a plain
  `fsync` (kept per file here, unlike the local engine's: this engine's concurrent copies overlap them), and the device
  barrier comes from the volume's `DeviceBarriers` (every 2 s, and in `flush_writes`). ❌ A caller that reports a write
  done, or deletes a moved source, without `flush_writes` first loses data on eject. DETAILS § the durability decision.
- **Force-rename is an atomic replacement**, so `supports_atomic_replace_rename()` is true. Network backends whose force
  path deletes first must not inherit that answer.
- **❗ The two listers yield the same entries, field for field** (`reading_parity_test.rs` runs every cell through
  both): delete trusts `is_symlink`, hardlink dedup `inode`. A doubtful bulk record re-stats through `entry_from_lstat`,
  a doubtful stream re-lists through `read_dir` from scratch: ❌ never drop, invent, merge, or return a partial listing.
  `added_at` is filled on APFS/HFS+ whichever lister ran, and by `get_single_entry`. DETAILS § "The local reader".
- **`FileEntry::is_hidden` folds in `UF_HIDDEN` and root-only `/.hidden`** on top of the dot check, from data the
  listing already has (no extra syscall).
- **Feed `on_progress` in `list_directory`**: the stat loop runs on `spawn_blocking`, so it publishes into a tally the
  async side samples. DETAILS § "The local reader".
- **A test build answers space from `statvfs`, never NSURL** (#357). Gate test scaffolding on
  `any(test, feature = "testing")`, ❌ never `cfg(test)` alone: the app's tests turn `testing` on and need the same
  behavior. A gate that changes BEHAVIOR reads `TEST_FAST_PATHS` instead, so the engine's bench measures what ships.

Decisions, the reader, space, the rename's degradation, the tests, and the capped surface: `DETAILS.md`. Read it before
any non-trivial work here: editing, planning, reorganizing, or advising.
