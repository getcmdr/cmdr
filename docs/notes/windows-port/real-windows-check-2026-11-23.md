# Real-Windows check of the 2026-11-22 and 2026-11-23 PRs (2026-11-23)

A check pass of PRs #1389, #1390, #1405, and #1406 in the running app on a real Windows desktop, plus a free look around
as a user would. Refs #942.

- **Machine**: mami (`ssh mami`), Windows 10 Pro 22H2 build 19045, Hungarian locale, 1600 × 900. How to reach it, build
  there, and run the GUI: `real-windows-check-2026-11-19.md` and `build-progress.md` § "Builder hygiene".
- **Build**: `main` at `14311db169` in the warm pair `D:\src\cmdr-m6` + `D:\targets\m6`, `CARGO_BUILD_JOBS=2`,
  `pnpm tauri build --debug --no-bundle` through `run-on-desktop.sh` (23 min, about 20 of them in cargo, after a
  dependency refresh). `main` built for the app with no local allows: #1382 is fixed.
- **How it ran**: started through `run-on-desktop.sh` with its own `CMDR_DATA_DIR` (`D:\w1123-data`), indexing on, and
  driven by Cmdr's MCP server (`CMDR_MCP_PORT`, a pinned `CMDR_MCP_TOKEN`, `CMDR_MCP_DIRECT_MODE=1`) through `ssh -L`
  with `scripts/mcp-call.sh`, plus window screenshots. Every dialog went through its real confirm path
  (`dialog_confirm`). Fixtures under `C:\Users\Admin\w1123` and `D:\w1123`, all removed after.
- **Folder mount**: `D:` also mounted at `C:\Users\Admin\w1123\dmount` (`mountvol`, David's one approved change,
  16:48:44–16:49:37, removed with `mountvol /D`, verified gone).
- **Screenshots**: not in the repo; in the session scratchpad's `shots/` folder, which a reboot clears.

## Results, one line per check

PR #1389 (shares and folder mounts):

- Pass: a copy of `test_file_1.txt` into `\\localhost\C$\Users\Admin\w1123\share-dst` (a share nobody mapped) opens with
  Copy live, the picker reading `//localhost/C$` and the box
  `//localhost/C$/Users/Admin/w1123/share-dst/test_file_1.txt`; it lands there with its bytes, nothing at
  `C:\test_file_1.txt`, no toast.
- Pass: with the mount in place, `list_volumes` reads `C:`, `build (D:)` once, and Servers.
- Pass: `nav_to_path` to `…\dmount\w1123\same` and Enter through `dmount` → `w1123` → `dst` each land with the tab,
  path, and listing agreeing; F5 there lands `test_file_2.txt` in `D:\w1123\dst`.
- Seen, already filed: below the mount the copy dialog's picker and the status bar read `C:`'s free space (#1392).
- Found: the visits below the mount wrote `D:`'s files into `C:`'s index at `D:`-relative paths, so a whole-drive search
  of `C:` lists `/w1123/same/d_file_1.txt`, which doesn't exist on `C:` (#1431).

PR #1390 (growing files, junction delete):

- Pass: two files appended every second by a PowerShell writer holding them open with `FileShare.Read`, one with
  `Flush($true)` and one with a plain `Flush()`: the pane's size followed within about 10 s with no refresh (9.8 KiB,
  17.6 KiB, 27.4 KiB against 12,024, 21,042, 30,060 bytes on disk), a refresh read the live size, and both ended at
  146.8 KiB for 150,300 bytes once closed. On this build NTFS's own record (`Get-Item`) also kept up, so #1387's stale
  record didn't reproduce as before.
- Pass: a permanent delete of a junction (`mklink /J`) removes the junction; its target and `keep.txt` stay.
- Found: that dialog's title says "Delete 1 folder" while its count line reads "1 file / 0 folders", and it calls the
  junction a symlink (#1432).

PR #1406 (typed write starts), each through its dialog:

- Pass: copy `C:` → `C:`, a folder move within `C:`, copy `C:` → `D:`, move `C:` → `D:`, a trash on `D:`, a permanent
  delete on `C:`, and a move of `to_zip_1.txt` into `C:\…\dst\test_archive_1.zip` (the zip then holds both entries).
- Pass: renames refuse cleanly with the editor kept open: `bad_name_1.` ("Filenames here can't end with a space or a
  period"), `bad:name_3` ("can't contain ':'"), `CON.txt` ("'CON' is reserved by Windows"), and `.` and `..` typed into
  the editor ("Filenames can't be '.' or '..'", red ring). MCP `rename` refuses `.` and `..` before opening the editor.
  A trailing space is trimmed before validation (`rename-operations.ts`), so `bad_name_2 ` renames to `bad_name_2`, by
  design.
- Pass: every finished operation's item rows read as app paths: `C:/Users/Admin/w1123/src/test_file_1.txt` →
  `D:/w1123/dst/test_file_1.txt`, the share copy's `//localhost/C$/…`, the mount copy's `C:/…/dmount/…`, the trash's
  `D:/$RECYCLE.BIN/<SID>/$R….txt`.
- Pass: undo of both renames, the trash on `D:` (the file back from `D:`'s Recycle Bin, the bin's item gone), both
  moves, both copies (one whose copy had since been deleted counts as rolled back), and the share copy: each
  `rolledBack`, the tree back as it started. The zip edit and the permanent deletes are not rollbackable, as designed
  (`zipEditUnsupported`, `permanentDelete`); a copy that overwrote is `overwrote`.
- Found: the trash on `D:` is journaled with `sourceVolumeId: root`, and finished trashes and deletes report
  `itemsDone: 0` (#1433). The undo isn't affected.
- Seen, not new: a clash's destination still reads `D:/w1123/dst\test_file_2.txt` in `cmdr://state` (#1225).

PR #1405 (bursts):

- Pass, three rounds each: 3,000 files made by a `cmd` `for /L` loop in the folder the left pane shows settle at exactly
  3,000 (3,001 rows with `..`) within 2–4 s of the loop ending (one sample caught 2,277 on the way); deleting them with
  the same loop settles at 0 within 2 s.

PR #1403: not testable here (CI only). PRs #1407 and #1409 were still open, so the thumbnail-file toggle, local About
cards, `~` in Go to folder, and the second-instance refusal weren't checked.

## Free look around (about 15 min)

- Pass: the viewer opens a text file in its own window (UTF-8 detected, 1 line, 11 bytes); About, Settings (system
  accent `#0078D7` as the app color), a new folder and file by name, a compress of a folder onto `D:`, a 1,000-file
  folder copy `C:` → `D:` (about 10 s with the dialog round trips), Brief view, and a new tab all work.
- Found: the Search dialog's default "Search in: Current folder" finds nothing on Windows and says it couldn't open one
  folder: the scope's `std::fs::canonicalize` answers `\\?\C:\…`, which the index never matches (#1430). A whole-drive
  search finds the files.
- Found: the MCP `search` tool's off-limits note says "protected by macOS itself" on Windows (#1434).
- Seen: the right pane's volume chip reads "Volume" on the unmapped share (no row for it), and the copy dialog spells
  its From native (`C:\Users\…`) and its To box as an app path (`C:/Users/…`). Left as is.
- Seen: eight small files sit loose in `C:\Users\Admin` (`alpha`, `bravo bravo`, `charlie`, `seed`, `a note for the zip`
  from 2026-11-22 06:55, `inside`, `lexical`, `physical` from 2026-11-18), most likely a test suite writing into the
  home folder. Not this run's; left in place.

## Open problems, by severity

1. **Medium**: a folder-scoped search finds nothing on Windows (#1430).
2. **Low**: a cover walk below a folder mount writes the other drive's rows into `C:`'s index (#1431).
3. **Low**: a `D:` trash journaled under `C:`, and `itemsDone: 0` on finished trashes and deletes (#1433).
4. **Low**: the junction delete dialog's counts and wording (#1432).
5. **Low**: the MCP search note names macOS on Windows (#1434).

Mami after the run: the app quit, no `cmdr`, `cargo`, `node`, or writer processes left, the fixtures, data dir, and
scripts gone, the mount point removed, nothing of this run in either Recycle Bin, and `cmdr-m6` on `origin/main`
(`14311db169`), clean. David's Task Manager was left as it was.
