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

## Recheck of #1481 after PR #1463 (2026-11-25, later)

- **How it ran**: as above, on `main` at `63cea1bd89` (PR #1463 in), then the same plus #1481's fix applied, each a
  `pnpm tauri build --debug --no-bundle` in `cmdr-m6` (17m 47s, then 13m 09s). Fresh `CMDR_DATA_DIR` (`D:\w1481\data`),
  MCP as above. `net use W: \\localhost\C$\Users\Admin\w1481 /persistent:no` ran in Admin's desktop session, as Cmdr
  did; a mapping made over SSH lives in another logon session and is no drive of Cmdr's. No USB stick plugged in.
- **Before the fix**: the drive watcher never started (`platform/startup/windows.rs` didn't call it, and the app's
  Windows `expect(dead_code)` hid that), so nothing heard the mapping. The row showed on the next volume-list read (5 s
  in, `list_volumes` reads the table fresh) as `C$ (\\localhost) (W:)`, id `smb-localhost-445-c-…`, root
  `//localhost/C$/`, `indexStatus: scanning`. That status was `root`'s: only `root` was indexed (`cmdr://indexing`), and
  the unregistered `W:/` routed to it by path. Navigating to `W:/open` from the share pane and from a `C:` pane listed
  the folder but kept the share's volume id (`volume: unknown`, the tab label left on the share). After
  `net use W: /delete` the row went on the next read, and both panes stayed on the gone `W:/open`.
- **After the fix**: `net use` raised `DBT_DEVICEARRIVAL` with `DBTF_NET` within 0.3 s (the Shell's `SHCNE_DRIVEADD` 5 s
  later); the row was `w1481 (\\localhost\C$\Users\Admin) (W:)`, Explorer's own name for it, id
  `smb-localhost-c-users-admin-w-…`, root `W:/`, `indexStatus: off`, registered on arrival. Both panes landed on
  `W:/open` on that volume. `net use W: /delete` raised `DBT_DEVICEREMOVECOMPLETE` (`DBTF_NET`), the volume
  unregistered, and both panes fell back to `~`. Nothing indexed the drive. Whole-share mapping
  (`net use W: \\localhost\C$`) still reads `C$ (\\localhost) (W:)` on the share's id.
- **Seen, filed**: with a pane on `//localhost/C$/…` and `W:` mapped to the whole share, unmapping `W:` unregistered the
  share's volume and sent the UNC pane to `~` too, though the share stays reachable (#1492). On quit Cmdr again rewrote
  `%APPDATA%\com.veszelovszki.cmdr\license.json` despite `CMDR_DATA_DIR`.
- **Mami after**: Cmdr quit, no `cmdr`, `cargo`, `node`, or `cmd` process of this run left, `net use` in Admin's session
  lists nothing, no firewall rule or prompt for `cmdr.exe`, no Recent shortcuts, `D:\w1481`, `C:\Users\Admin\w1481`, and
  this run's job folders removed, `cmdr-m6` back on `origin/main` (`63cea1bd89`) clean (`D:\targets\m6` holds the fix's
  build), David's Task Manager left open.

## Second pass (2026-11-25, later)

A check of what landed after the first pass: PRs #1461, #1462, #1463, #1465, #1471, #1473, #1483, #1485, #1486, #1488,
and #1493.

- **Build**: `main` at `3c2a8dea4c` (all eleven merged; PR #1490 still open) in the warm pair, `CARGO_BUILD_JOBS=2`,
  `pnpm tauri build --debug --no-bundle` through `run-on-desktop.sh` (17 min, 15m 33s in cargo).
- **How it ran**: as the first pass: a fresh `CMDR_DATA_DIR` (`D:\w1125b\data`, wiped between the two index runs), MCP
  over `ssh -L`, the same PowerShell driver in Admin's session, fixtures under `C:\Users\Admin\w1125b`. Mappings and
  `subst` ran in Admin's desktop session. No USB stick plugged in. Screenshots in the session scratchpad's `shots2/`.

### A. Index stall (#1480, PR #1485): pass, #1480 closed

- **Run 1** (nothing opened during the walk): home, then the whole volume, 677,672 entries in 260.2 s, "covered end to
  end". Heartbeats on `root` every 5.0–5.4 s except one 44.9 s gap:
  `one ComputeSubtreeAggregates held 'root' for 42.2 s` (237,830 dirs under one frontier root). Then the ledger heal:
  `ComputeAllAggregates: done, 257146 directories in 51.4s`, `one PayLedgerIfUnpaid held 'root' for 51.4 s`, two
  `Waited 30 s … to flush` warnings, and a live batch waiting 51.0 s (`process_live_batch_slow`). No drain yielded.
- **Run 2** (wide folders opened during the whole-volume walk: `WinSxS`, `Program Files`, `ProgramData`, then test
  folders): the walk stopped as in #1480, `Phases: 'root' stops with 2 folders still to walk` at 08:56:16.7, after
  562,503 entries in 684.6 s.
  - Heartbeats every ~5 s; gaps of 12.0 s and 11.5 s (two 10.7 s subtree aggregates) and 52.3 s (the heal's
    `PayLedgerIfUnpaid`, after the phases restarted).
  - 1,267 `Ancestor roll-up:` drains, 271 ending with folders left (yielded), 49 "nothing moved". The largest: 1,057
    newly queued, chains read in 25 ms, 256 recomputes in 250 ms, 801 left; the longest chain read of any yielded drain
    25 ms; peak `rollups_owed` 801.
  - From about 08:51 to 08:56 one or two folders stayed owed (`still owes 1 directory … after 60 s and 58 drains` up to
    `after 242 s and 220 drains`): each drain did ONE recompute in 470–750 ms, a wide parent re-owed by each newly
    walked child. It never stalled the writer.
  - **Time to settled**: the last owing drain ended 0.3 s before the stop line, and the first beat after it (08:56:22.0)
    reads `rollups_owed=0`: at most 5.3 s.
- **Live changes**: create, rename, and delete in `w1125b\live` showed in `list_dir` (`source: index`) within 0.3–1.0 s
  of the command returning, during run 1's walk and after run 2 settled (`indexStatus: fresh`, authoritative).
- **Exactness**: after each clean quit, `drift_audit_against_a_copied_index` on a copy of `index-root.db` (+ `-wal`),
  run on the Mac: pass both times.
- **Filed**: the 42–52 s single-message holds, during which live changes and flushes wait (#1499, low).

### B. Drive notifications (PRs #1463, #1493): 7 pass, 1 fail

- Pass: `Watching for drives arriving and leaving` at startup.
- Pass: `net use W: \\localhost\C$\Users\Admin\w1125b`: `Volume Arrived: ['W'] (… network true)`, `Drive arrived` 0.27 s
  later, in `list_volumes` 1.3 s after the command, as `w1125b (\\localhost\C$\Users\Admin) (W:)` (Explorer's name),
  `indexStatus: off`. `W:/` lands on that volume (`smb-localhost-c-users-admin-w-…`).
- Pass: `net use W: /delete` removed the row 0.26 s after the notice, and the pane on `W:/` fell back to `~`.
- Fail, #1492 reopened: a pane on `//localhost/C$/Users/Admin/w1125b` with `W:` mapped to the whole share: on unmap the
  backend logs `Keeping share //localhost/C$/: its mapped letter went, the share still answers` and
  `still serves from its active root`, yet the pane goes to `~` 110 ms later (three times).
- Pass: `subst X: C:\Users\Admin\w1125b` makes an `X:` row (`path-x-…`, local, not indexed), `X:/` lands on it, and
  `subst X: /d` removes it and the pane falls back.
- Pass: quit with `W:` mapped: the process was gone within 1.4 s of the quit call.

### C. USB stick: not checkable

- No stick plugged in. "Safely remove" isn't scriptable, and PR #1490 (eject) isn't on this commit.

### D. Open with, F4, terminal (PR #1462): 9 pass, 1 fail

- Fail, #1498: the Open with submenu draws no app icons (`.txt`: "Notepad (default)", "WordPad", "Choose another app";
  `.jpg`: "Photos (default)", "Snip & Sketch", "Paint 3D", "Paint", …), three menus in a row. Settings' "Edit files in"
  draws the same handlers' icons.
- Pass: the default is marked; picking WordPad opened `note.txt` in WordPad; `.json` offers only "Choose another app"
  (Windows has no handler for it); Open with is greyed for `.exe`, `.bat`, and `.lnk`.
- Pass: F4 on "System default" opens `data.json` and `marker.bat` in Notepad; the `.bat` never ran (its marker file
  never appeared).
- Pass: "Edit files in" lists "System default (Notepad)", "WordPad", and "Choose an app…", with icons.
- Pass: F9 with the default opens Windows PowerShell in the pane's folder (`PS C:\Users\Admin\w1125b\openwith>`; no
  Windows Terminal or `pwsh` installed); with Command Prompt, `cmd` in the folder; Command Prompt on
  `\\localhost\C$\…\openwith` shows "Command Prompt can't start in a network folder…" and starts nothing.

### E. Icons round two (PR #1473): 7 pass, 1 fail

- Pass: a `.lnk` to Paint draws Paint's icon with the arrow, readable at 100 %; a `.lnk` to `\\localhost\C$\…\calc.exe`
  draws the generic page with the arrow.
- Pass: the volume picker draws the Windows-logo drive on `C:` and a globe on the mapped `W:`.
- Pass: a read-only folder whose `desktop.ini` names a local `red.ico` draws it.
- Pass, security: a read-only folder whose first `IconResource=` is `\\192.0.2.1\x\a.ico` (a local one after it) and one
  with `CLSID2=` plus `IconFile=\\192.0.2.1\x\b.ico` both draw the generic folder. `Get-NetTCPConnection` showed nothing
  to `192.0.2.1` or to port 445 and `Get-SmbConnection` was empty before, after listing them, and at the end.
- Pass: on Cmdr gold, Desktop, Documents, Downloads, Music, Pictures, and Videos keep their own icons.
- Fail, #1479 reopened: on Cmdr gold, 3D Objects, Contacts, Favorites, Links, OneDrive, Saved Games, and Searches turn
  into plain gold folders; on "System theme color" they draw their icons (twice).

### F. Small fixes (PR #1488): 3 pass

- Pass: Ctrl+V on an empty clipboard shows the quiet "No files on the clipboard. Copy files first with Ctrl+C."
- Pass: Cmdr Ctrl+X, Explorer Ctrl+V (moved), then Cmdr Ctrl+V: the same quiet note, and the clipboard is empty after.
- Pass: `%APPDATA%\com.veszelovszki.cmdr\license.json` kept its size, modified time (05:27:29 UTC), and SHA-256 across
  both quits.

### G. Engine (PRs #1465, #1486): 3 pass, remote half not reached

- Pass: New folder with `a\b` is refused ("Folder names here can't contain "\""), OK greyed; rename to `x\y` is refused
  (red outline, nothing renamed on disk); a new folder `newdir` gets the cursor.
- Not reached: the SFTP half (a top-level remote `top\level.txt` to a local and a remote folder, the conflict Rename on
  `a\b.txt`). Adding an SFTP server takes the Servers UI or a `propose_server` approval with a typed password, which
  didn't fit the time.
- Seen: Enter sent about 0.1 s after retyping a valid name over an invalid one in New folder did nothing; with a 1 s
  pause it created the folder.

### H. Crash capture (PR #1471): not checkable

- The repo documents no debug command or environment variable that crashes a dev build on purpose.

### I. Store kind (PR #1461): pass

- Settings > Updates & privacy shows "Check for updates" and the usual toggles, no Store or package wording.

### Mami after the second pass

- Cmdr quit; no `cmdr`, `cargo`, `node`, Notepad, WordPad, PowerShell, `cmd`, or Explorer window of this run left.
- `net use` and `subst` in Admin's session list nothing; the display scale wasn't touched; no firewall rule or prompt
  for `cmdr.exe`; the clipboard is empty; nothing went to the Recycle Bin.
- `C:\Users\Admin\w1125b`, `D:\w1125b` (data dir, build log, driver), this run's job folders, and the three Recent
  shortcuts it made removed. The private SFTP container on the Mac removed.
- `cmdr-m6` on `origin/main` (`3c2a8dea4c`), clean; David's Task Manager and Settings windows left open.
