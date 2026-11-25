# Real-Windows check of the 2026-11-24 and 2026-11-25 PRs (2026-11-25)

A check pass of PRs #1447, #1448, #1449, #1450, #1451, #1456, #1458, and #1459 in the running app on a real Windows
desktop, plus what the 2026-11-23 pass owed. Refs #942.

- **Machine**: Mami (`ssh mami`), Windows 10 Pro 22H2 build 19045, Hungarian locale, 1600 × 900 at 100 %. How to reach
  it and run the GUI: `real-windows-check-2026-11-19.md` and `build-progress.md` § "Builder hygiene". No USB stick was
  plugged in, so no removable-drive check ran.
- **Build**: `main` at `ecafa5824c` (all eight PRs merged) in the warm pair `D:\src\cmdr-m6` + `D:\targets\m6`,
  `CARGO_BUILD_JOBS=2`, `pnpm tauri build --debug --no-bundle` started through `run-on-desktop.sh` (17 min, 15m 03s in
  cargo).
- **How it ran**: as the 2026-11-23 pass: its own `CMDR_DATA_DIR` (`D:\w1125\data`, fresh), indexing on, Cmdr's MCP
  server (`CMDR_MCP_ENABLED`, `CMDR_MCP_PORT`, a throwaway `CMDR_MCP_TOKEN`, `CMDR_MCP_DIRECT_MODE=1`) over `ssh -L`
  with `scripts/mcp-call.sh`. Real input and screenshots came from a PowerShell driver run in Admin's session through
  `run-on-desktop.sh --wait`: `keybd_event` keys, `mouse_event` clicks and drags, `MoveWindow`, `System.Windows.Forms`
  clipboard reads, `Graphics.CopyFromScreen` crops. Fixtures under `C:\Users\Admin\w1125` and `D:\w1125`.
- **Remote names**: Windows can't mount an SMB share on a port other than 445 ("SMB mounting isn't supported on this
  platform"), so the backslash-name check used an OpenSSH server instead (the repo's SFTP fixture image, run privately
  on the Mac and reached through `ssh -R`), which is the same POSIX-name path PR #1451 fixes.
- **Screenshots**: not in the repo; in the session scratchpad's `shots/` folder, which a reboot clears.

## Results, one line per check

### A. Folder search (PR #1447): 4 pass

- Pass: the Search dialog's "Current folder" on `C:\Users\Admin\w1125\search` finds all three `s_alpha_*` files,
  subfolder included; MCP `search` with `scope: "C:/Users/Admin/w1125/search"` finds the same three, `unresolvedScopes`
  empty.
- Pass: the same on `D:\w1125\search` (dialog and MCP), and Go to file on a `D:` result lands on `D:/w1125/search/sub`.
- Seen, already filed: boot-drive results read `/Users/Admin/w1125/…` (the dialog's Path column "Users / … / w1125 /
  search", no drive), #1452; `D:` results read `D://w1125/search/s_bravo_1.txt` (the dialog "D: / w1125 / search"),
  #1453. Evidence on both.
- #1430 closed.

### B. Open and reveal (PR #1450): 21 pass, 1 fail, 1 not checkable

- Pass: Enter on `.txt` opens Notepad, `.jpg` opens Photos, both in front, no toast. Enter on `.pdf` shows Windows' "How
  do you want to open this file?", the same chooser `cmd /c start doc.pdf` shows (Chrome was installed since, so Windows
  asks again); none was picked.
- Pass: an unknown `.zzqx` shows Windows' picker; a `.bat` echoing `%CD%` prints `C:\Users\Admin\w1125\open`.
- Pass: Enter opens a file on `\\localhost\C$\…` and one on a drive mapped while Cmdr ran (`W:`).
- Pass: a name ending in a dot and a 296-character path show "Other apps can't open … by its path"; a file deleted from
  outside right before Enter shows "gone.txt didn't open. Windows couldn't find the file" (`Win32(2)`).
- Pass: Ctrl+Shift+E selects a file, a folder, a comma folder (`a,b folder`), a comma file inside it, and a folder at a
  drive root (`D:\w1125`) in Explorer, which comes to the front; a second reveal in the same folder reuses the window.
  The context menu, the File menu, and the About card's button all reveal. "Go to Recycle Bin" (command palette) opens
  the bin in front. Reveal of a deleted file and of the long path show their toasts.
- Fail: a drive mapped while Cmdr runs has no row at first, navigation to it from a share pane keeps the old volume, and
  the row that arrives later indexes the whole `//localhost/C$/` (#1481).
- Not checkable: revealing a drive itself (a root has no row to put the cursor on).

### C. Icons (PR #1449): 12 pass, 1 fail

- Pass: `.txt`, `.pdf`, `.zip`, `.html`, `.mp3`, `.mp4`, `.ps1`, `.bat`, `.jpg`, `.png` draw their Shell icons; `.docx`,
  `.xlsx`, `.json`, `.zzqx`, and extensionless files the plain page (no Office on Mami, as in Explorer).
- Pass: copied `notepad.exe` and `regedit.exe` draw their own icons; a `.lnk` to `mspaint.exe` draws Paint's icon, with
  no shortcut arrow (as designed for now); a `.lnk` to `\\127.0.0.1\C$\…\calc.exe` and a `.url` with
  `IconFile=\\127.0.0.1\C$\…` draw the plain icons, and neither `net use` nor `netstat` showed a connection to
  `127.0.0.1:445` before or after.
- Pass: a folder with a `.lnk` to a dead share (`\\w1125-nohost\share`) had every icon drawn in the first screenshot,
  about 4 s after navigation (the driver's resolution).
- Pass: edges are clean at 100 % and 150 % (enlarged crops, no dark fringe), and both scales look crisp.
- Pass: on the default "System theme color", Desktop, Documents, Downloads, Music, Pictures, and Videos draw their own
  icons.
- Fail: with App color on Cmdr gold those six turn into plain gold folders, and folders Windows customizes through
  `desktop.ini` (3D Objects, Contacts, Favorites, Links, OneDrive, Saved Games, Searches) show the generic folder in
  either mode (#1479).
- For David: on Windows the default App color is "System theme color", so a fresh install shows Windows' own yellow
  folders untinted. The gold tint needs the `appearance.appColor` setting on `cmdr-gold`. Screenshots of
  `C:\Users\Admin`: `C2-folders-appcolor-system.png` (shipped default), `C3-folders-appcolor-gold.png` (gold), and
  `C4-explorer-admin.png` (Explorer), with enlarged crops `C2-…-crop3x.png` and `C3-…-crop3x.png`.

### D. Junk set (PR #1448): 6 pass

- Pass: copy `C:` → `D:` of a folder holding `real.txt`, `.DS_Store`, `._x.jpg`, and a hidden-system `Thumbs.db`: on
  `skip` only `real.txt` lands; on `include` all four; on `ask` the dialog offers "Skip system clutter files (.DS_Store,
  Thumbs.db, and more)" unticked at 23 bytes / 4 files, and ticked it reads 4 bytes / 1 file and lands only `real.txt`.
- Pass: moves `C:` → `D:` on `skip` (only `real.txt` lands, the source folder goes), on `include` (all four), and on
  `ask` left unticked (all four).

### E. Index watch (PR #1456): 1 fail, 3 not checkable

- Fail: on this fresh data dir `C:`'s first scan never finished, and its writer stopped heartbeating at 02:23:04 while a
  flush waited over an hour (`Waited 3481 s for 'root''s index writer to flush`); 2,000 files (100 MB) made at 02:36
  were still missing from the index at 03:21 (#1480).
- Not checkable because of it: parent sizes after renaming and deleting a big folder, one rescan round for 10,000 files,
  and the "watch ended" / "Watch re-arm" lines (no `subst` or VHD tried).

### F. Clipboard (PR #1458): 9 pass, 2 fail

- Pass: Cmdr Ctrl+C → Explorer Ctrl+V copies; the clipboard holds `FileDrop`, `Preferred DropEffect` 1, and the paths as
  text, one per line (Notepad's paste shows them so).
- Pass: Cmdr Ctrl+X (`Preferred DropEffect` 2) → Explorer Ctrl+V moves both files.
- Pass: Explorer Ctrl+X → Cmdr Ctrl+V moves, the clipboard empties (Explorer's handshake), and after a partial cut the
  files left in Explorer aren't ghosted. Explorer Ctrl+C of three files and a folder → Cmdr Ctrl+V copies them to `D:`.
  Cmdr Ctrl+C of a folder and two files → Explorer Ctrl+V copies all three.
- Pass: Ctrl+C and Ctrl+X on a `dup.` name and on a 296-character path show "Other apps can't open … from the clipboard,
  so nothing went on it", and a sentinel text stays on the clipboard.
- Fail: after Explorer pastes Cmdr's cut, Cmdr's Paste stays live and Ctrl+V tries the move again: "Couldn't find the
  file" with "Send error report…" (#1478, twice).
- Fail: Ctrl+V with nothing on the clipboard shows a persistent red "Cmdr couldn't create a file from the clipboard.
  This volume doesn't support that." (#1477, twice). So "a second Ctrl+V does nothing" passes for the files and fails
  for the feedback.

### G. Drag (PR #1459): not checkable

- Driven with `mouse_event` press, 30–40 absolute moves 15–50 ms apart, a hold of 0.4–1.2 s over the target, and a
  release, from Admin's session; tried also with `SetCursorPos` plus relative nudges, and with the source window already
  in front (no Alt tap).
- No drag from Explorer ever started, not even Explorer to its own navigation pane, so the injected input doesn't start
  Explorer's drag on this setup. A drag from a Cmdr row did start (`native_drag drag out ended: effect 0x0`) but never
  landed in Explorer.
- So nothing of G could be judged: the target highlight, the dialog, the Move/Copy defaults, Ctrl and Shift, sources
  kept until confirm, at 100 % or 150 %, a second drag, or the fast flick. It needs a human hand (or a test that calls
  the drop target directly).

### H. Engine (PR #1451 and the walker change): 9 pass

- Pass: a copy of a tree into a folder holding the same tree with clashes asks per file ("stop"): Overwrite on
  `sub\clash.txt` and Rename on `same.txt` (`same (1).txt`), the rest lands, `only_dst.txt` stays.
- Pass: its undo is refused as `notRollbackable` / `overwrote`, as designed; a clean copy `C:` → `D:` undoes to
  `rolledBack` with the copies gone and the source intact.
- Pass: a move `C:` → `D:` and a same-drive move, each undone to `rolledBack`, the tree back where it was.
- Pass: a move-merge with "skip all" moves the two new files and leaves the two clashing ones in the source.
- Pass: a trash of the tree (one item in the bin) undone to `rolledBack`, the bin item gone; a permanent delete of a
  tree.
- Pass: copying a remote folder holding `a\b.txt` and `plain.txt` into `C:\Users\Admin\w1125\remote_dst` asks about
  `a\b.txt` per item ("Name not allowed at the destination", Skip / Skip all) and lands `plain.txt`.
- Seen, already filed: a top-level remote `top\level.txt` lands with no question as a new folder `top` holding
  `level.txt` (#1457, evidence there). Clash rows in `cmdr://state` still spell `…/engine/src\sub\clash.txt` (#1225).

### I. Owed from 2026-11-23: 6 pass

- Pass: `icacls` on `mcp.token`, `mcp.port`, and a report preview (`report-preview\ERR-QTZDC.txt`, opened from "What's
  about to be sent"; nothing was sent) each read `DESKTOP-UQ7GNDA\Admin:(F)` alone. The `report-preview` folder inherits
  `D:`'s ACL (Administrators, SYSTEM, Authenticated Users modify, Users read).
- For reference: `%APPDATA%` (`C:\Users\Admin\AppData\Roaming`) grants `SYSTEM`, Administrators, and Admin full control,
  all inherited.
- Pass: Go to folder with `~` lands on `C:/Users/Admin`, `~\` on `C:/Users/Admin`, `~\Downloads` on
  `C:/Users/Admin/Downloads`.
- Pass, #1434 closed: the off-limits note reads "(folders only the system or another account may read)", names 10
  folders, and says "and 109 more (all listed in offLimits)".

## Also seen

- The first SMB connection put up a Windows Firewall prompt for `D:\targets\m6\debug\cmdr.exe`, and Windows added two
  Block rules for it at 02:52:40. The prompt was never answered (its process was ended) and both rules were removed.
- On quit Cmdr rewrote `%APPDATA%\com.veszelovszki.cmdr\license.json` (54 bytes, a 2026-11-18 reminder timestamp) though
  `CMDR_DATA_DIR` pointed elsewhere. Left as it was.
- The `D:` index was "off" yet ran a walk and live reconciles for `D:` paths after a `D:`-scoped search.
- Mami's Servers list showed Naspolya (`192.168.1.111`) and raspi as found nearby, so it isn't on the isolated guest
  Wi-Fi the infra repo's Mami notes describe.

## Open problems, by severity

1. **Medium**: a fresh `C:` index's writer stalls for an hour and live changes never land (#1480).
2. **Medium**: a cut Explorer pasted stays on Cmdr's clipboard; a second paste errors (#1478).
3. **Low**: Ctrl+V with an empty clipboard shows a red "couldn't create a file" toast (#1477).
4. **Low**: Cmdr gold drops the special-folder icons; `desktop.ini` folders show the generic icon (#1479).
5. **Low**: a drive mapped while Cmdr runs: late row, wrong volume, whole share indexed (#1481).
6. Untested: all of G (drag), E beyond the stall, removable drives (no stick).

## Mami after the run

- Cmdr quit; no `cmdr`, `cargo`, `node`, Notepad, Photos, `cmd`, or OpenWith process of this run left; the Explorer
  windows it opened closed.
- Display scale: 100 % before (relative step 0, 96 DPI, no `PerMonitorSettings` key), 150 % for about four minutes
  (around 03:16–03:19), back to 100 %, and the `PerMonitorSettings` key the change created removed. The change moved
  David's Task Manager and Settings windows; both were put back where they were.
- `mountvol` not used. The `W:` mapping (`net use`, non-persistent, 02:17 until about 02:56) removed; `net use` lists
  nothing. The firewall rules above removed.
- Fixtures, the data dir, the driver, the build job files, the icon samples this run added to
  `%TEMP%\cmdr-icon-samples`, and the four Recent shortcuts it left removed; nothing of this run in the Recycle Bin; the
  clipboard empty.
- `cmdr-m6` on `origin/main` (`ecafa5824c`), clean. David's Task Manager left open.
