# First real-Windows check of tasks 34, 60, 62, 63, and 67 (2026-11-19)

The first run of the in-page menus, the browser-key switch, and the Windows look on a real Windows desktop. Everything
before this was built on a Mac with a mocked Windows look and checked on the Linux E2E lane (WebKitGTK).

- **Machine**: mami (`ssh mami`), Windows 10 Pro 22H2 build 19045.6466, Hungarian locale, en-US keyboard layout
  (`0x04090409`), light app mode, transparency effects on, accent `#0078D7`, 1600 × 900 at 100 %, WebView2 Evergreen
  157.0.4341.60. No Segoe UI Variable installed (`C:\Windows\Fonts\SegUIVar.ttf` absent).
- **Build**: `main` at `4314b3cf1f`, `pnpm tauri build --debug --no-bundle` in `D:\src\cmdr-verify`
  (`CARGO_TARGET_DIR=D:\targets\verify`, seeded from `D:\targets\triage`, 17m 58s), so the MCP bridge was in and
  `devtools` would have been reachable if F12 worked. The fixes below were re-run on `968733ae7f`.
- **How it ran**: a PowerShell driver in Admin's console session (started with
  `~/projects-git/vdavid/infra/mami-laptop/run-on-desktop.sh`) launched the app with its own `CMDR_DATA_DIR`, read
  queued command files, sent real input with `keybd_event` and `mouse_event` (so WebView2 saw OS keys, a lone Alt and
  AltGr's Ctrl+right Alt included), read `GetGUIThreadInfo` for Windows' menu mode (`GUI_INMENUMODE` 0x4,
  `GUI_SYSTEMMENUMODE` 0x8), read and set the clipboard, and saved window-cropped screenshots. The Tauri MCP bridge
  connected over `ssh -L` but every call failed ("WebView execution failed"), so DOM state came from screenshots and
  behavior only. Another agent's Windows Firewall prompt for `adb.exe` sat topmost over the desktop; it was moved aside,
  never answered.
- **Screenshots**: not in the repo; numbered as below in the session scratchpad's `win-verify/` folder
  (`/private/tmp/claude-501/…/scratchpad/win-verify/`), which a reboot clears.
- **A slip**: one queued step ran after Cmdr had quit, so Ctrl+Shift+Q and Ctrl+W reached another agent's `menu-spike`
  window (still running after). The driver now refuses input unless Cmdr is in front.

## Results, one line per check

Task 63 steps 6 and 7 (in-page menus):

- Pass: the bar shows in the main window (`03`) and the viewer (`90`), no native bar.
- Fail, fixed: a lone Alt tap gives the bar the keyboard (↓ then opens File, `11`) and shows the underlines, but drew no
  ring on File (`10`): Chromium doesn't count a modifier-only keydown as keyboard use, so `:focus-visible` missed. F10
  drew it (`20`), as did Escape back to a title (`13`). Re-run on `968733ae7f`: File draws its ring after a lone Alt
  (`202`).
- Pass: Alt, F10, and Alt plus a letter no title takes (Alt+Q) never start Windows' menu loop: `GetGUIThreadInfo` flags
  0 after each, and the next key reaches the page (`103`, `104`). No beep is expected with `SC_KEYMENU` swallowed; the
  box has no speaker check, so audibly untested.
- Pass: Alt+V opens View (`22`), Alt+E opens Edit; ↓ walks rows, → crosses to the next menu (`12`), Enter on a submenu
  row opens it (`24`).
- Pass from a focused pane: Escape closes the menu (focus on its title, `13`), the next Escape puts focus back on the
  pane list (↓ moved the cursor, `19`, `43`).
- Fail, fixed: with focus adrift on `<body>` (the first-launch "What's new" closed by a click), Alt then Escape twice
  left focus on the Edit title of a bar that had let go, and ↓ did nothing (`14`, `15`). Re-run on `968733ae7f` after
  the same click-closed dialog: Alt then Escape hands the pane the keys (`202`–`205`).
- Pass: picking View > Show hidden files lists the dot folders and the row shows its checkmark the next time (`26`,
  `27`); the viewer's View > Word wrap checks and unchecks (`95`, `96`).
- Pass: Alt+Space opens the window's system menu (Restore, Move, Size, Minimize, Maximize, Close; menu mode 0xC, `28`);
  Alt+F4 closes the app.
- Partly untested: AltGr typing, since the layout is en-US and changing it was off limits. What AltGr sends (left
  Ctrl+right Alt, with and without a letter) never opened or focused the bar (`31`, `32`); a right Alt alone acts as Alt
  on this layout (`30`), as Windows defines it.
- Pass: right-click on a row opens the in-page menu at the pointer (`40`); Shift+F10 and the Menu key open it under the
  cursor row with its first row highlighted and underlines on (`41`, `42`); it flips up at the bottom edge (`44`) and
  left at the right edge (`46`). No second menu (WebView2's own) appeared after the Menu key.
- Pass: the rename editor's right-click opens the in-page edit menu (Undo and Redo greyed on a fresh field, `51`). Copy
  put "3D Objects" on the clipboard; Paste replaced the name with the clipboard's text (`53`); Undo turned live after
  the paste and restored the name (`54`, `55`); Select all selected it (`56`); Cut emptied the field and the clipboard
  held the name (`57`). The rename stayed open throughout. Shift+F10 and the Menu key inside the field open the same
  menu under the field, once (`213`, `214`); the Settings search field gets it too, Undo live after typing (`211`).
  Shift+F10 and the Menu key inside the field open the same menu under the field, once (`213`, `214`); the Settings
  search field gets it too, Undo live after typing (`211`).
- Pass: Edit > Paste from the bar (Alt+E, P) while renaming pasted into the field and kept the rename open (`58`, `59`).
- Pass: Ctrl+W closes Settings, the queue, Keyboard shortcuts, and the viewer. Tab walks Settings (sidebar, then the
  Language select, `79`, `80`) and Keyboard shortcuts (the "Hide features" checkbox, `85`); the empty queue has no live
  control to Tab to, so untested there.
- Pass: the `SC_KEYMENU` guard on windows opened later: Alt and F10 in Settings, Alt+X in Settings, and Alt in the
  queue, Keyboard shortcuts, and the viewer leave menu-mode flags at 0.

Task 67 (browser keys):

- Pass: in Settings with "xyz" typed into its search, F5, Ctrl+R, Alt+←, Ctrl+plus, F7, F12, Ctrl+Shift+P, Ctrl+P, and
  Ctrl+J changed nothing: the text stayed, no zoom, no caret-browsing prompt, no DevTools, print, or downloads window
  (`78`, window list before and after). The same in the viewer (`94`). In the main window F5 opens Cmdr's Copy dialog,
  its own binding, and the rest did nothing of WebView2's (no new window).

Tasks 60 and 62 (look):

- Fail, fixed: Settings and the operation queue were see-through: the windows behind showed through the page (`70`,
  `81`). Their openers opened `transparent` with a clear fill whenever macOS's Reduce transparency was off, and the
  Sidebar material they asked for exists only on macOS. Keyboard shortcuts was opaque (`84`). Re-run on `968733ae7f`:
  both open opaque (`210`, `212`).
- Pass: the Fluent radio, switch, and select read as WinUI's (`70`). On `main` before task 40 the accent was Cmdr's gold
  under "System theme color"; on `a394bbdcfc` (task 40 merged) the radio, the swatch, and the selected segment draw
  exactly the system accent, `#0078D7` by pixel sample (`220`).
- Pass: text renders in Segoe UI. Segoe UI Variable isn't on this Windows 10, so the stack's `'Segoe UI Variable Text'`
  falls through to `'Segoe UI'`, as designed; the Variable cut needs a Windows 11 check. Not confirmed by a computed
  style read (the bridge failed), only by the glyphs.
- Untested: dark mode (the system is light, and switching it was off limits).

Task 40 (system look, merged mid-run as PR #1286, re-run on `a394bbdcfc`):

- Pass: `cargo nextest run -p cmdr --no-fail-fast system_look intl::` on the builder, 81 of 81 (13 `system_look`, 68
  `intl`), the first Windows run of those tests.
- Pass: the first system-look read at startup took 12.6 ms (`Windows look, read in 12.622ms` in the file log, accent
  `#0078d7`), with another agent's build running; recorded in `apps/desktop/src-tauri/DETAILS.md`.
- Pass: the controls' accent matches the system accent (above).

Task 34 leftovers:

- Pass: the volume switcher lists `C:` (no label) and `build (D:)` with NTFS and free space, and Servers (`100`).
- Pass: the path bar reads `C:\Program Files` (`102`).
- Open: at the drive root the path bar shows nothing after the volume chip (`101`), and the home and Downloads tabs read
  `~` and `~/Downloads`, with a `/` (`03`).

## Open problems, by severity

1. **High: the Copy dialog refuses every local copy** with "Filename can't be empty" under a destination of
   `~/Downloads/3D Objects`, Copy greyed (`77`). Likely `resolveTransferFilename`
   (`apps/desktop/src/lib/file-operations/transfer/transfer-dialog-utils.ts`) answering `null` on Windows, which the
   dialog words as an empty name: `~/` expands through `joinPath` onto the home path, and `splitNormalizedPath`'s
   `parsePath` may not read the result (a native home, or a mixed `C:/…` spelling). Not chased; local copy doesn't land
   on Windows yet anyway (#1225), but the message is also wrong for a `null` answer. Fixed (2026-11-21, #1292, PR
   #1304): the native home was the cause. The dialog read Tauri's `homeDir()` (`C:\Users\Admin`), so `~/Downloads/…`
   expanded to a path holding a `\`, which the app-path grammar refuses; it now reads the backend's `getHomeFolder`
   (`C:/Users/Admin`), as do the pane's breadcrumb `~` and the drop rules, which had the same bug.
2. **Low: Settings is slow to appear under load**: on `a394bbdcfc`, with another agent's build on both cores, Ctrl+,
   took more than 3.5 s to show Settings, and the main window went "Not responding" (a ghost window) for a few seconds
   the first time; Alt+F4 then took over 5 s to exit. Not seen on `4314b3cf1f` under a similar load; not attributed. Now
   #1307.
3. **Low: path bar spelling**: the drive root shows no segment, and `~`-paths keep `/` (task 34 leftovers).
4. **Low: the Copy dialog offers "Skip Mac-only hidden files (.DS_Store, .\_\*)" on a Windows-to-Windows copy** (`77`).
   Now #1305: a Mac-made stick or share carries those files on Windows too, so it needs a decision.
5. **Low: the Tauri MCP bridge connects but runs nothing on Windows** ("WebView execution failed" for `execute_js` and
   `manage_window list`), so agents can't read DOM state there. Now #1306.
6. **Untested**: AltGr typing (needs a layout with AltGr), dark mode, Segoe UI Variable (Windows 11), an audible beep,
   Tab in a non-empty queue.
7. **High: a move within one drive refuses before it starts** (found 2026-11-21 with task 35's mount table, a debug
   build of `f07ceab5d4`): the Move dialog hands the same-root local move (`transfer-dispatch.ts` → `moveFiles` →
   `commands/file_system/write_ops.rs::move_files`) its destination volume-relative, and nothing anchors it at the root
   volume's `C:/`, so the engine refuses it. The log:
   `move_files_start: sources=["C:/Users/Admin/w35/src/same-drive"], destination="/Users/Admin/w35/dst"`, then
   `failed: {"type":"io_error",…,"message":"a path shaped for another OS"}`. Copy and a move onto another drive carry
   their volume ids (`copyBetweenVolumes`, `moveBetweenVolumes`), whose paths the backend anchors, so they land. On
   macOS the root is `/`, so the two spellings coincide. Fix in flight (#1225): `move_files` anchors its destination at
   the root volume's root, like the copy path.

## Task 35's mount table on the desktop (2026-11-21)

A debug `pnpm tauri build --debug --no-bundle` of `f07ceab5d4` in `D:\src\cmdr-m6` (`CARGO_TARGET_DIR=D:\targets\m6`,
13m 18s warm), started through `run-on-desktop.sh` with its own `CMDR_DATA_DIR`, and driven by Cmdr's own MCP server
(`CMDR_MCP_PORT`, a pinned `CMDR_MCP_TOKEN`, `CMDR_MCP_DIRECT_MODE=1` so `dialog_confirm` runs) over `ssh -L` with
`scripts/mcp-call.sh`: `nav_to_path`, `select_volume`, `select`, `move` / `copy`, `dialog_confirm`, `resolve_conflict`,
and `cmdr://state`. Fixtures under `C:\Users\Admin\w35` and `D:\w35-dst`, removed after.

- Blocked: a folder move within `C:` (problem 7 above). The engine half (a same-drive move renames, the file id kept) is
  the suite's: the `move_op::tests::a_same_fs_*` and `journal_capture_tests::*same_fs*` cells turned green
  (`windows-test-failures.md` § "By milestone", task 35).
- Pass: a folder move from `C:` to `D:` (`cross-drive` with a subfolder): staged under `D:\w35-dst\.cmdr-staging-…`,
  landed whole with both files' bytes, the source folder gone from `C:`, no staging or temp left.
- Pass: Overwrite a folder with a file: copying the file `report` onto the folder `report` (holding `old.txt`) asked per
  clash; answered Overwrite, the folder gave way to the 19-byte file, nothing else left (`Get-ChildItem -Force`).
- Pass: a folder onto a same-named folder with Overwrite all merges: `photos\p.txt` reads `NEW`.
- Not one `MountedVolumeInside` in the app's log. Not testable here: the refusal on a real folder mount (making one is a
  `mountvol` system change, not done); the mount-table cells and the Win32 probes in
  `crates/cmdr-local/src/mounts/platform/windows.rs` cover it.
- Seen, not chased: `nav_to_path` to `D:/w35-dst` from a `C:` pane timed out its 2 s existence check (with the index
  walking `C:` on both cores); `select_volume` first, then `nav_to_path`, landed. A pending clash spells its destination
  `C:/Users/Admin/w35/ow-dst\report` (#1225's mixed separators).

## Task 36's delete on the desktop (2026-11-21)

Driven the same way as task 35's above (Cmdr's MCP server, `CMDR_MCP_DIRECT_MODE=1`, requests sent on the builder
itself), fixtures under `C:\Users\Admin\w36`, a file held open by a PowerShell process that opened it with no sharing.
Every fixture and every item this run put in the Recycle Bin is gone again; nothing else in the bin was touched.

- **Before, on `main` at `98e2c50409`**: a trash of a file and of a folder both refused ("this platform has no Trash",
  `trash_refused` with reason `other`), items kept. A permanent delete of a file and of a folder worked. A permanent
  delete of a READ-ONLY file deleted it with no word (`std::fs::remove_file` clears the attribute). A permanent delete
  of a file held open refused as `file_in_use`.
- **After, PR #1329 merged with `main` at `d6130c29b8`** (a debug build, 12 min warm):
  - Pass: a trash of a file, then of two folders: all three in the Recycle Bin as the Shell lists it
    (`Shell.Application` namespace 10), each with its original location `C:\Users\Admin\w36`.
  - Pass: a restore by hand, the Shell's own Restore verb on one folder: back in place with its file's bytes. (The Shell
    left that folder's `$I` record behind, which the bin no longer lists.)
  - Pass: the app's undo of the two-folder trash (`operations_rollback`): `rolledBack` for both (the restored one
    counted as already back), the other folder back with its bytes and its `$I` record gone.
  - Pass: a permanent delete of a file and a folder.
  - Pass: a permanent delete of a read-only file refuses as `file_locked` and the file stays; a trash of one recycles.
  - Pass: a permanent delete of a held file refuses as `file_in_use`; a trash of one refuses as `trash_refused` with
    reason `inUse` (the Shell's `HRESULT 0x80270027`), the file kept.
  - Pass (PR #1327): a folder moved within `C:` through the Move dialog renamed in place: the same file id
    (`fsutil file queryfileid`), its two files' bytes intact, the source gone. Every delete above ran from a normal pane
    row after that PR's `..` refusal.
  - Not seen on screen: the dialogs' wording, since an elevated Task Manager sits on top of the desktop. The error types
    are what the dialog words (`errors.write.fileLocked`'s Windows suggestion, `trashRefused.*.inUse`).
  - Not tested in the app: a drive with no Recycle Bin (no USB stick or share on the builder); `windows_tests.rs`'s
    `\\localhost\C$` cell covers the veto.
  - Seen, not chased: two folders PowerShell made in the listed folder never appeared in the left pane until it was
    navigated away and back: no listing watch is armed on a Windows drive at all (#1330).
- **The review round** (`windows_tests.rs` on the builder, sparse files on `C:`, whose bin holds 5,831 MB): a file past
  the bin's limit was DELETED FOR GOOD by a trash before `bin.rs` (the Shell announced it with the recycle flag and
  reported success with no bin item, out of the sink's sight); at the limit it recycled. A folder past the limit, even
  of files that each fit, fell back to its children, which the sink vetoed whole. A folder holding a file another
  program holds unshared refused whole, a junction recycled as the link with its target kept, and a permanent delete on
  a drive's volume walk (every drive but the boot one) now refuses a read-only file as `file_locked` too. Every item
  these runs put in the bin is gone again.

## PR #1349 on the desktop (2026-11-22)

A debug build of `ce19cd766e` in `D:\src\cmdr-m6`, driven the same way as task 35's above (Cmdr's MCP server over
`ssh -L`, `CMDR_MCP_DIRECT_MODE=1`), plus `keybd_event`-style keys and one `mouse_event` click through
`run-on-desktop.sh`. Fixtures under `C:\Users\Admin\r1349` and `D:\r1349-dst`, removed after, with the two items this
run put in the Recycle Bin (nothing else was in it).

- Pass: a same-drive move of a read-only file asks "Move read-only items?" before anything moves; Move anyway moves it,
  still `ReadOnly` where it lands; Skip keeps it in place, still read-only, and moves the rest.
- Pass: a read-only folder moves within `C:` with no question.
- Pass: a `C:` → `D:` move asks before anything reaches `D:` (the destination folder empty while it waits); Move anyway
  removes the original. The landed copy isn't read-only (#1352).
- Pass: "Go to Recycle Bin" from the trash toast and from the command palette opens File Explorer's Recycle Bin with the
  trashed items; neither pane moves.
- Pass: on a `subst` drive (`S:` → `C:\Users\Admin\r1349`, made in Admin's session and removed after) F8 opens on
  "Delete permanently" with "This volume doesn't have a Recycle Bin", and "Go to Recycle Bin" says "This drive doesn't
  have a Recycle Bin". On `\\localhost\C$` (a share nobody mapped, so no volume row) the delete dialog opens on the
  Recycle Bin by the frontend's optimistic default (`snapshot-source-volume.ts`), and the trash refuses as
  `trash_refused` with the file kept; "Go to Recycle Bin" says there's none.
- Not tested: a USB stick (none on the builder), a folder-mounted volume and a drive whose bin is turned off (both
  system changes).

## PR #1374 and the day's PRs on the desktop (2026-11-22)

A debug build of `01407cb5d7` in `D:\src\cmdr-m6` (13m 25s warm), driven the same way as task 35's above (Cmdr's MCP
server, requests sent on the builder itself, `CMDR_MCP_DIRECT_MODE=1`), indexing on. `main` didn't build for the app
(#1382), so the run allowed the two dead-code items locally, then reverted them. Developer Mode is off, OneDrive isn't
signed in (no account, not running), no mapped drive exists. Fixtures under `C:\Users\Admin\r1374` and `D:\r1374`,
removed after with the one item this run put in the Recycle Bin. An elevated Task Manager sat on top of the desktop, so
Cmdr's window was moved beside it for screenshots.

- Pass: a copy `C:` → `D:` lands `ro.txt` `ReadOnly`, `hid.txt` `Hidden`, and the folder `hiddir` `Hidden` (its file
  plain). A `C:` → `D:` move of a read-only file asks "Move read-only items?", reading "1 item you're moving is
  read-only. Cmdr can make it writable just for the move and read-only again where it lands."; Move anyway lands it
  `ReadOnly` and removes the original.
- Fail: a copy into `\\localhost\C$\Users\Admin\r1374\dst` (a share nobody mapped) is refused up front as "no
  permission", though a normal-token write there succeeds. The dialog's picker reads `C:` and the copy's destination
  collapses to `C:/` (#1384).
- Pass: a folder whose DACL denies Admin writes (`icacls … /deny`) shows "You don't have permission to add files to this
  folder." with Copy greyed. (A `(W)` deny also takes `SYNCHRONIZE`, so the pane couldn't list it either: a fixture
  quirk.)
- Pass, by the second branch: a relative folder symlink copied within `C:` refuses as `links_not_supported`, its target
  folder landing (#1376's stub; its dialog's Windows words are stale, noted there).
- Fail: a permanent delete of a junction, and of a folder symlink, refuses "Access is denied", the link kept (#1386). A
  trash of the junction recycles the link; its target's file stays.
- Pass: a folder of 20,000 small files on `D:` opens in 1,743 ms cold (read 1,253, sort 467) with the `C:` index walking
  on both cores, 1,343 ms the second time.
- Fail: a file another program appends to every second keeps its first-flush 9.8 KiB in the pane, refresh included,
  while the disk passed 420,000 bytes; it updates once the writer closes it (#1387).
- Skipped: OneDrive (not set up).
- With `D:` mounted a second time at `C:\Users\Admin\r1374\dmount` (`mountvol`, David's one approved change,
  14:23–14:26, removed): at the mount's root, a copy of a `D:\` file onto itself through the other path is seen as the
  same folder and duplicated as `name (1).txt` (no overwrite offer, no `cantTellIfSameFile`), and a move from
  `D:\r1374\mv` into it renamed (same file id). Below the root the pane goes wrong: navigation "doesn't land" while the
  path bar and `cmdr://state` say it did, the tab keeps the old folder, and the transfer dialog offers that old folder
  as the destination (#1385). The volume list showed the mount as a second `D:` row with `D:`'s id and root.
- Pass: the regression sweep. Two items made outside Cmdr appear in the pane within 2 s; F8 sends to the Recycle Bin;
  two files compress to a zip on `D:` and extract back to `C:` with their bytes; the volume list reads `C:`,
  `build (D:)`, and Servers with `C:` indexing; a same-drive move's undo puts the file back. "Go to Recycle Bin" not
  re-run (keys sent through `run-on-desktop.sh` went to Task Manager, which held the foreground).

## PR #1389's share and folder-mount fixes on the desktop (2026-11-22)

A debug build of `16ba1be9ed` in `D:\src\cmdr-m6` (13m 44s warm, #1382's two dead-code items allowed locally, then
reverted), driven by Cmdr's MCP server over `ssh -L` (`CMDR_MCP_DIRECT_MODE=1`), indexing on. Fixtures under
`C:\Users\Admin\r1389` and `D:\r1389`, removed after. `D:` mounted a second time at `C:\Users\Admin\r1389\dmount`
(`mountvol`, David's one approved change, 15:17:02–15:18:15, removed with `mountvol /D`).

- Pass (#1384): a copy of `C:\…\src\share-src.txt` into a pane on `\\localhost\C$\Users\Admin\r1389\dst` (a share nobody
  mapped) opens with no "no permission" line and Copy live, the box reading
  `//localhost/C$/Users/Admin/r1389/dst/share-src.txt`; confirmed, it lands in `…\r1389\dst` with its bytes, nothing at
  `C:\share-src.txt`. The log reads `dest=//localhost/C$/` (it read `dest=C:/` before).
- Pass (#1385): `list_volumes` reads `C:`, `build (D:)` once, and Servers. `nav_to_path` to `…\dmount\r1389\same` lands
  with the tab, path, and `volumeId: root` agreeing; Enter on `dmount`, then `r1389`, then `same` (`open_under_cursor`)
  does too. F5 from the other pane offers `C:/Users/Admin/r1389/dmount/r1389/same/s.txt`, and the file lands in
  `D:\r1389\same`.
- Seen, not chased: the picker reads "Choose…" for the unmapped share (it has no row), and "C:" with `C:`'s free space
  for the folder below the mount. After the share copy, a toast said Cmdr couldn't confirm the copies were fully written
  to `//localhost/C$` (the durability barrier on a share).

## PR #1389's review fixes on the desktop (2026-11-22)

A debug build of `960bd9302c` (the review's first fix commit; 12m 0s warm, #1382's two allows local, then reverted),
driven by Cmdr's MCP server over `ssh -L`, indexing on. Fixtures under `C:\Users\Admin\r1389c` and `D:\r1389c`, removed
after. `D:` mounted a second time at `C:\Users\Admin\r1389c\dmount` (`mountvol`, David's one approved change,
16:08:35–16:14:34, removed with `mountvol /D`).

- Pass: below the mount, the tab, path, and `volumeId: root` agree; F5 of `D:\r1389c\x.txt` there lands in
  `D:\r1389c\same`.
- Pass, data safety: a copy of `D:\r1389c\x.txt` into `…\dmount\r1389c` (the same folder by another path) with
  "Overwrite all" lands as `x (1).txt`, the source intact; a move of `D:\r1389c\mv\m.txt` into `…\dmount\r1389c\mv` (the
  same file) is a no-op, the file intact; a move into `…\dmount\r1389c\same` is a rename (`fsutil file queryfileid`
  unchanged).
- Pass: a file trashed below the mount lands in `D:`'s Recycle Bin (the run's item removed after); the listing watch
  below the mount shows a new file within 4 s, no refresh.
- Pass: the picker for a copy into `\\localhost\C$\…\dst` reads `//localhost/C$` (it read "Choose…"), and the copy
  lands.
- Found: that copy's "couldn't confirm the copies are fully written" toast is a share refusing the folder flush with
  `ERROR_INVALID_FUNCTION` (1), which the durability pass didn't read as "not offered"
  (`flush: couldn't fsync the directory …: Incorrect function. (os error 1)`); classed `Unsupported` in the next commit,
  proven by its Windows test cell rather than a second desktop build.
