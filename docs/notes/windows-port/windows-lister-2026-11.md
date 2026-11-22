# The Windows lister: find data versus a stat per entry (2026-11)

Windows-port plan task 35 asked for a `FindFirstFileExW` lister (`FIND_FIRST_EX_LARGE_FETCH`, basic info level) if it's
measurably faster than the one `LocalWindowsVolume` shipped with, which read `read_dir` and then one `lstat`
(`std::fs::symlink_metadata`, a handle open plus a query) per entry. The decision and the code it led to:
`crates/cmdr-local/DETAILS.md` § "The Windows sibling", `crates/cmdr-local/src/reading/platform/windows.rs`
(`find_data_entries`).

## How it was measured

- **Where**: GitHub's hosted `windows-latest` runner (NTFS, Defender's real-time scan off), inside the `tests` job of PR
  #1374's light runs, through a temporary nextest cell (`lister_measurement_test.rs`, removed once these numbers were
  in).
- **Fixture**: one folder of 20,000 one-byte files, written fresh by the cell.
- **Method**: one warm-up round, then seven rounds with the listers interleaved round-robin so none gets a
  systematically warmer cache; the median wall time per lister.
- **Listers**: the reader end to end (`FileEntry` rows, what a pane gets), and the bare enumeration loops beneath it.

## Numbers

Run 1 (CI run 45008311254, 2026-11-22), the reader before the change:

- The reader, `read_dir` plus an `lstat` per entry, `FileEntry` rows: 631 ms.
- `read_dir` plus `symlink_metadata` per entry, no rows: 347 ms.
- `read_dir` plus `DirEntry::metadata` (the find data, no call per entry), no rows: 9.5 ms.
- `FindFirstFileExW`, basic info, no flags: 7.4 ms.
- `FindFirstFileExW`, basic info, `FIND_FIRST_EX_LARGE_FETCH`: 9.3 ms.

Run 2 (CI run 45010557898, 2026-11-22), the reader after the change, against the forced stat-per-entry lister:

- The reader off the find data (`find_data_entries`), `FileEntry` rows: 246 ms.
- The forced `Lister::ReadDir` (an `lstat` per entry), `FileEntry` rows: 732 ms.
- The bare loops again: 417 ms with `symlink_metadata` per entry, 10.3 ms with `DirEntry::metadata`, 7.5 ms for
  `FindFirstFileExW` without flags, 9.9 ms with `FIND_FIRST_EX_LARGE_FETCH`.

The cell that took them, `crates/cmdr-local/src/lister_measurement_test.rs`, landed and left with PR #1374 (its commits
say "TEMPORARY, price the Windows listers"); bring it back from git for a re-run.

## What it says

- **A listing is about three times faster end to end** (246 ms against 732 ms for 20,000 rows). What's left is building
  the rows themselves (about 12 µs each: the app-path join, the icon id, the entry), not the disk.
- **The stat per entry was the cost**, about 40 times the enumeration itself. `std::fs::read_dir` on Windows already is
  `FindFirstFileExW` with basic info (`library/std/src/sys/fs/windows.rs`, Rust 1.100), and `DirEntry::metadata` hands
  back the find data it read, so dropping the per-entry stat gets almost the whole win with no Win32 code of our own.
- **`FIND_FIRST_EX_LARGE_FETCH` buys nothing measurable** at 20,000 entries on a local NTFS disk (9.3 ms against 7.4 ms
  without it, within the noise of the rest), so the reader keeps `std`'s loop. It may matter on a share, where each
  `FindNextFileW` batch is a round trip; unmeasured.
- **The find data can lag**: NTFS updates a directory record's size and last-write time for a file another program holds
  open for writing when that handle closes (Explorer lists the same stale size). The watcher re-stats a changed entry
  (`get_single_entry`), which reads the live answer.
