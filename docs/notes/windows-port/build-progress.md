# Windows build progress

A running log of the Windows build track (#942, `docs/specs/windows-port-plan.md` § "Windows build state"), one entry
per milestone, newest last. The machine is the Windows 10 22H2 builder (`ssh mami`, MSVC; how to reach it, build there,
and run the GUI: `docs/tooling/ci.md` § Windows lane); the fast loop is `cargo check --target x86_64-pc-windows-gnu` on
a Mac with zig as the C compiler and `AWS_LC_SYS_PREBUILT_NASM=1`: `CC_x86_64_pc_windows_gnu` names a script that drops
cc-rs's `--target` flag and runs `zig cc -target x86_64-windows-gnu`, and `AR_x86_64_pc_windows_gnu` runs `zig ar`. Two
things it can't judge: the app's build script (no `windres`) and `numkong`'s AVX-512 kernels, which zig's clang refuses.

- **2026-11-18, baseline** (`52f7ec04ea`): the seven must-compile members (`cmdr-collation`, `cmdr-http`, `cmdr-menu`,
  `cmdr-platform-cfg`, `cmdr-recents`, `cmdr-sort`, `cmdr-update-trust`) build and pass `cargo test` on the builder, 141
  tests, in 1m 58s cold. `cmdr-fs` failed its own compile (54 errors natively, matching the CI break report) and blocked
  the other 24.
- **2026-11-18, `cmdr-fs` compiles** (`40fb822f24`): off the pending list (24 left). `cargo test -p cmdr-fs` on the
  builder: 782 passed, 1 failed, 27 ignored. The runs found three real Windows gaps: app paths go through `std::path`
  (#1225, 24 tests ignored on Windows), deleting a cached SQLite database fails (#1226, one test), and the durable sync
  reopened landed files read-only, which `FlushFileBuffers` refuses (the one failure, fixed in the next entry); the test
  helpers had the same read-only habit.
- **2026-11-18, 16 more members compile** (`306b96dfab` and the commit after it): `cmdr-adb`, `cmdr-archive`,
  `cmdr-brief-columns`, `cmdr-git`, `cmdr-listing`, `cmdr-media-meta`, `cmdr-mtp`, `cmdr-nfs`, `cmdr-operation-log`,
  `cmdr-s3`, `cmdr-search`, `cmdr-sftp`, `cmdr-smb`, `cmdr-viewer`, `cmdr-webdav`, and `operation-log-dump` leave the
  pending list (8 left). `cargo clippy --all-targets -D warnings` over all 17 passes on the builder in 3m 11s. Fixed on
  the way: `durable`'s landed-file reopen (write access for `FlushFileBuffers`), `NameRules::host()` handing Windows
  Linux's case-sensitive rules, and git fixtures that followed the machine's `init.defaultBranch` (`master` from Git for
  Windows' installer).
- **2026-11-18, tests on the builder** (`cargo nextest run`, nextest 0.9.136 installed there): 3,683 of 3,939 passed
  before the cmdr-git fixture fixes; `cmdr-fs`, `cmdr-search`, `cmdr-brief-columns`, `cmdr-media-meta`, and
  `operation-log-dump` are green, `cmdr-git` is down to one #1225 case (ignored), and the network and device backends
  fail by the dozen on #1225 (counts in that issue). Plain `cargo test` also fails `cmdr-search`'s arena memory budget,
  whose counter is process-wide: run nextest there.
- **2026-11-18, after the fixture fixes** (`56ce685e42`, nextest on the builder): `cmdr-fs`, `cmdr-git`, and
  `cmdr-search` are green (1,622 of 1,624 with `cmdr-listing`). `cmdr-listing` keeps two failures: `foreign_path_test`
  (#1225) and `a_read_that_never_answers_reports_the_listing_stalled_within_the_deadline`, which attributes the stall to
  `Unknown` where macOS and Linux say `Drive` (not yet diagnosed).
- **2026-11-18, blockers at that point**: `cmdr-local` (46 errors: `LocalPosixVolume` is POSIX through and through;
  whether it grows Windows arms or gets a `LocalWindowsVolume` sibling is task 35's call) blocks `cmdr-operations`,
  `cmdr-multi-rename`, `cmdr-restic`, `cmdr-listing-wasm`, and the app. `numkong` and `usearch` build fine under MSVC;
  `cmdr-index` (and so `index-query`) fails in its own code with 11 errors, mostly its SMB and MTP index transports,
  gated to macOS and Linux while `indexing/handle/mod.rs` calls them unconditionally, plus the `platform_case`
  collation's missing Windows arm and `scanner/refusal.rs` comparing `libc::EPERM`.
- **2026-11-18, `cmdr-index` and `index-query` compile** (`e6c858bda3`): off the pending list (6 left). The SMB, MTP,
  ADB, and local-external index transports were gated to macOS and Linux with Windows stubs beside them; both backends
  ship on Windows (D2, task 21), so the gates and stubs went and the transports are every-host code. The `platform_case`
  collation asks `cmdr_fs::name_fold::compare_for_comparison` (a new arm per OS beside `normalize_for_comparison`, so
  collation and key can't disagree), and the scanner's refusal rule reads `OsErrorClass` where it compared
  `libc::EPERM`. The Mac cross-check needs numkong's x86 SIMD targets off (`NK_TARGET_<ISA>=0` for each of the 11 x86
  probes), since zig's clang refuses its AVX-512 kernels. On the builder: clippy `--all-targets -D warnings` clean in 53
  s warm; nextest 1,753 of 1,911 passed, 111 failed, 47 timed out, nearly all app paths (#1225) and the cold-drive
  fixture's walks never settling, filed with the clusters as #1232.
- **2026-11-18, `cmdr-local` compiles, with `LocalWindowsVolume`** (`bf10814efa`): off the pending list (5 left). The
  POSIX volume moved to `src/platform/local_posix*` under `cfg(unix)`, the Windows sibling sits beside it, and the
  reader and space grew `platform/` seams (`reading.rs` went from 18 OS cfgs to 6). On the builder:
  `cargo clippy -p cmdr-local --all-targets -D warnings` clean; `cargo nextest run -p cmdr-local` 30 of 30 pass, 8
  ignored by name (seven conformance cells waiting on task 35's streams, metadata, links, and xattrs, plus the
  mount-table cell). The hostile rename cells pass on NTFS: an exclusive rename onto a taken file or folder answers
  `AlreadyExists` with both intact; onto a file another handle holds open (`FILE_SHARE_READ` only), the exclusive rename
  is `AlreadyExists` and the forced one is refused with both files whole; a forced rename onto a read-only file is
  all-or-nothing; a 300-plus-character path creates, lists (app paths), stats, renames, and deletes; deleting a junction
  keeps its target.
- **2026-11-18, next blockers** (Mac cross-check, `cargo check --all-targets --keep-going`): `cmdr-operations` fails its
  own compile with 15 errors, and blocks `cmdr-multi-rename`, `cmdr-listing-wasm`, `cmdr-restic`, and the app. Each is a
  POSIX-only function with no Windows arm, task 36's stubs:
  `validation::{validate_destination_writable, refusing_folder}` and `local_disk`'s `write_probe` (an `access(W_OK)`
  pre-flight), `free_space::available_space`, `delete/cloud_trash::metadata_is_online_only` (also asked by
  `scan_preview/local.rs`), `transfer/volume/extract_quarantine::archive_quarantine`, and `local_disk/mod.rs` naming
  `LocalPosixVolume`. `cmdr-restic`'s own breaks are test-only (`std::os::unix` in `tree_tests.rs` and
  `volume_tests.rs`) plus its `open_cost` example naming `LocalPosixVolume`. About 150 sites across the consumers name
  `LocalPosixVolume` (mostly `cmdr-operations` tests), which waits on David's call on how a consumer names the host's
  local volume.
- **2026-11-18, `cmdr-operations` compiles** (leg 3, `a0d5994472` and `85aa3ec973`): off the pending list (4 left).
  `local_disk/` became a `platform/` seam with a Windows arm; free space and the destination pre-flights go through it,
  so `free_space.rs`, `validation.rs`, and most of `copy_strategy.rs` lost their OS cfgs. Real on Windows: the write
  probe (a `FILE_WRITE_DATA` open with backup semantics, which runs the DACL and read-only checks and writes nothing),
  free space, and the folder volume. A Windows copy takes the chunked loop where it took `std::fs::copy`. Directory and
  file syncs now open through `cmdr_fs::durable::{open_dir_to_sync, open_to_sync}`: `File::open` refuses a folder on
  Windows and `FlushFileBuffers` refuses a read-only handle, so every Windows copy's closing flush failed before. Clippy
  on Windows needed two boxes (`PathedVolumeError::aftermath`, `merge::LeafEnd::Refused`): a `PathBuf` is 32 bytes
  there, which took the transfer error past `result_large_err`'s 128. 65 POSIX-only test files are `#![cfg(unix)]`. On
  the builder: `cargo clippy -p cmdr-operations -p cmdr-fs --all-targets -D warnings` clean in 1m 27s warm;
  `cargo nextest run -p cmdr-operations` 1,445 of 1,734 pass, the five write-probe cells on real NTFS among them (a
  fresh folder and file, a read-only file refused with `ERROR_ACCESS_DENIED` and its bytes intact, a missing path, a
  file another handle holds unshared as `Unknown`, a folder past 300 characters). The 289 failures cluster: the local
  engine's finalize hands `cmdr_local::rename_local_exclusive` a native `C:\…` path, which it refuses as not an app path
  (#1225, the largest cluster: no local copy or move lands on Windows yet); a folder delete or move refuses as
  `MountedVolumeInside`, the cautious answer while `file_identity` has no device and `cmdr-local` no mount table on
  Windows (task 35); the in-flight-temps test ledger can't be cleared while a connection holds it (#1226's family); and
  the rest are app paths in remote-volume fixtures. Full log: the builder's `D:\tmp\ops-nextest.log`.
- **2026-11-18, `cmdr-multi-rename`, `cmdr-restic`, and `cmdr-listing-wasm` compile** (leg 3, `0ea49128c7`): off the
  pending list (only the app left). The first and last needed nothing past `cmdr-operations`; `cmdr-restic` gated two
  non-UTF-8 name cells and a fixture's executable bit to `cfg(unix)`, and its `open_cost` example opens
  `LocalWindowsVolume` on Windows. On the builder: clippy `--all-targets -D warnings` clean for all three in 2m 4s
  (vendored `rustic_core` warns once about an unused variable in a Windows-only arm). `cargo nextest run` over the
  three: 322 of 393 pass, 61 fail and 10 time out, nearly all `cmdr-restic` and `cmdr-listing-wasm` cells whose fixtures
  root at `/Volumes/…` or `/` (#1225).
- **Next blocker: the app `cmdr`** (Mac cross-check, `cargo check -p cmdr --lib`): 25 errors, past its build script once
  a stand-in `x86_64-w64-mingw32-windres` on `PATH` writes an empty object for each `.rc` (nothing links in a check; the
  MSVC builder needs no stand-in). Leg 3 un-gated `mount_registration`, `upgrade_existing_smb_mounts` and its two
  helpers (each asks `volumes::platform`, which has Windows arms), and gave `smb_mount_gate::probe` Windows the Linux
  answer (never proven encrypted, so refused): 21 left. Each cause, with what it waits on:
  - `cmdr_local::LocalPosixVolume` named in `file_system/mod.rs` (the root volume at `/` and macOS's cloud drives) and
    `network/smb_mount_gate.rs` (wraps the concrete type and downcasts to it): David's call on how a consumer names the
    host's local volume, then task 34 for what the root volume is on Windows.
  - `file_system::volume::eject` and `commands::eject` are `any(macos, linux)`, while `ipc.rs`, `server_move_smb.rs`,
    and `commands/network.rs` reach them on every OS. Inside, `not(target_os = "macos")` arms mean Linux (`umount`), so
    un-gating would hand Windows Linux's eject; it needs a `platform/` seam with a Windows arm (task 39).
  - `licensing/device_id.rs`: no `read_platform_uuid` Windows arm (task 38, `MachineGuid`).
  - `saved_servers/nfs_places.rs`: `getuid`, `getgid`, and `getgroups` with no Windows arm (an NFS identity there is the
    export's, or a configured one).
  - The rest (seven unused imports) cascade from the above. Every fix lands in a file the placement allowlist already
    counts, so each Windows arm goes behind a `platform/` seam or un-gates code; a new OS cfg outside a seam needs
    David's OK.
- **2026-11-18, the app compiles for Windows** (leg 4, `970527c2e4` through `ed3991ae38`): `windowsPending` is empty, so
  every workspace member is on the must-compile list. The four groups, each through a `platform/` seam:
  - The root volume and the SMB gate ask `volumes::{root_volume, local_volume_on_mount, is_local_volume}`; Windows
    registers `LocalWindowsVolume` at the system drive under `root` (a stub, task 34).
  - Eject is every-OS code with `eject/platform/` holding the tool, ejectability, and the SMB-disconnect rule; Windows
    refuses a local drive as `NotEjectable` (task 39).
  - The licensing device id reads `MachineGuid` (task 38, real).
  - NFS: `NfsIdentity::of_this_process` from `cmdr-nfs`' seam, `nobody` on Windows, the machine name from
    `GetComputerNameExW` (task 46, real). With the type errors gone, the lint pass found code only the missing Windows
    arms call (the unmount watcher, stored favorites, native-crash reports); one crate-root
    `expect(dead_code, unused_imports)` for Windows holds it (plan § "Windows build state"). The test target needed 31
    test files `#![cfg(unix)]` and cell gates in 36 more.
- **On the builder** (`df87f01096`): `cargo clippy -p cmdr -p cmdr-nfs --all-targets -D warnings` clean, 12m 01s cold.
  The first `cargo nextest run -p cmdr -p cmdr-nfs` built and couldn't list the app's harness:
  `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139), since tauri-build's manifest resource (Common Controls v6) reached the
  app's bins only. `build.rs` now has MSVC embed `windows/app.manifest` in every binary. Then: 5,511 tests, 5,274
  passed, 235 failed, 2 timed out, 146 skipped (151 s; failures `cmdr` 188, `cmdr-nfs` 49). Nearly all are #1225's
  family: 65 panics name "not an absolute Windows app path" and a dozen more a `\photos`-style native path; the root
  volume's path lookups (`resolve_location`, `a_home_relative_path_resolves_to_root`) wait on task 34; the two timeouts
  are a dead-share card and an agent-cancelled walk. Full log: the builder's `D:\logs\w4-nextest2.log`.
- **Starting the app** (`df87f01096`, plain `cargo build -p cmdr --bin Cmdr`, 13m 10s): `target\debug\Cmdr.exe` launched
  from SSH (session 0, no desktop) gets through plugin setup (the MCP bridge listens on 127.0.0.1:9223, WebView2 makes
  its profile under `%LOCALAPPDATA%\com.veszelovszki.cmdr\EBWebView`), then panics in Tauri's setup:
  `failed to create webview: WebView2 error … HRESULT(0x80070578) "Invalid window handle."` The app's own log never gets
  written (the logger starts later). That's SSH's missing window station, not a Cmdr fault: the next proof needs a
  signed-in desktop session, task 55's QA.
- **Frontend and the full build** (`ed3991ae38`): `pnpm install --frozen-lockfile` 2m 46s and the desktop `vite build`
  17.8 s both pass on the builder. `pnpm tauri build --debug` first died at once with `spawn pnpm ENOENT` (the wrapper
  spawned the `pnpm` `.cmd` shim without a shell; it now runs the Tauri CLI with its own Node). After that fix it ran
  the `beforeBuildCommand` (llama-server placeholder, vite), compiled in 13m 02s, and bundled
  `target\debug\bundle\nsis\Cmdr_0.61.0_x64-setup.exe` (36.89 MiB); the run exits 1 only at the updater-signature step,
  for want of `TAURI_SIGNING_PRIVATE_KEY`, as any keyless local build does. Also seen: the root `prepare` script is
  POSIX shell (`[ -n "$CI" ] || …`), so `pnpm install` on Windows prints "'[' is not recognized" and skips setting
  `core.hooksPath`.
- **Next blockers for a usable Windows build**: #1225 (app paths through the local engine: no local copy, move, or
  listing of a native path lands yet), task 34's drive ids and multi-root volumes, task 35's streams and lister, and a
  GUI session for the first real launch (task 55).
- **2026-11-18, first launch on a real desktop** (task 55's front edge; Windows 10 22H2 build 19045, mami, debug
  `pnpm tauri build --debug` binaries of `ed3991ae38` and then `a0eee76edd`): the window comes up, lists `C:\`, and
  enters folders, with no panic, no Application event log entry, and no CPU over a 10 s idle sample more than two
  minutes after the last action.
  - **How it ran**: SSH sits in session 0, which has no desktop, so a one-off scheduled task (principal `admin`,
    `LogonType Interactive`, the menu spike's technique) starts a PowerShell runner in the signed-in console session.
    The runner launches `Cmdr.exe` with stderr redirected, saves a `CopyFromScreen` shot whenever a request file appears
    (lid closed, 1600 × 900), sends `SendKeys` the same way, and on a stop file collects the app log and the Application
    event log; the task is deleted after each run. Cmdr's MCP server (`CMDR_MCP_PORT=19225`, a fixed `CMDR_MCP_TOKEN`)
    answered `scripts/mcp-call.sh` from the Mac through `ssh -L`. Nothing else on mami changed.
  - **What worked on the first try**: every startup step (logger, settings migration, index start, operation log and
    agent DB migrations, the MCP server and bridge), "What's new" on a first launch, the first-run panes on `~` and
    `~/Downloads`, up to the system drive's root, Enter into `C:\Windows` (107 entries), and real arrow and Enter key
    presses through WebView2. Data lands where task 48 put it: `%APPDATA%\com.veszelovszki.cmdr`, and `drive-index`,
    `logs`, and `EBWebView` under `%LOCALAPPDATA%\com.veszelovszki.cmdr`.
  - **Fixed** (`a0eee76edd` and the commit after it): `resolve_path_volume_fast` answered no volume for every path on
    Windows, so `nav_to_path` to `C:/` refused ("Couldn't reach that location's drive") and Go to path and favorites
    couldn't land; it now answers the root volume for a path on the system drive. The favorites seed and the switcher's
    rows published native `C:\Users\…` paths, which no resolver reads (every favorite read `reachable: false`); both now
    store the app path through `path_grammar::app_spelling`. Rerun on `a98c04b8a9` with a fresh `favorites.json`: all
    four favorites resolve to `root` and `nav_to_path` lands on `C:/Users/<account>/Documents`. Its unit tests
    (`volumes::platform::windows`, `app_spelling`) ran on the Mac only, since another agent held the builder.
  - **Observed, not chased**: the root index starts at `/`, which Windows reads as the current drive, so a binary
    started from `D:\…` walked `D:\` (257,236 entries in 38 s) while its watcher's branches came out as `/C:/Windows`
    and were refused (plan § "Windows build state"); `nav_to_path` with a `\` path resolves but its landing check times
    out (the frontend normalizes typed text, MCP doesn't); the breadcrumb shows nothing at the drive root and `\Windows`
    below it while `~/Downloads` keeps its `/`; the secret store is the plain-file fallback; the focus watchdog warns
    once at startup (nothing focused for 500 ms); a second `nav_to_parent` at the root times out instead of refusing;
    the first `nav_to_path` about 30 s after launch got no frontend answer, and the same call worked a minute later;
    `Ctrl+D` opens the favorites menu, but a typed `2` (SendKeys) left it open with nothing picked; `index_provider`
    logs "not an absolute Windows app path" reading the volume's used bytes.
- **Task 38's identity bundle** (`52047a9377`): secrets in Credential Manager, the real OS version and memory, the
  "already running" message box, and managed policy from the registry (plan, task 38). On the builder:
  `cargo clippy -p cmdr --all-targets -D warnings` clean, 13m 23s cold in a fresh target dir; nextest over the touched
  modules 145 of 151. Found on the way: key-based OpenSSH logons have no credential vault, so every Credential Manager
  call answers `ERROR_NO_SUCH_LOGON_SESSION`; the round-trip cells pass when the test binary runs as SYSTEM from a
  scheduled task (`schtasks /ru SYSTEM`). The cargo here puts test binaries under
  `target\debug\build\<crate>\<hash>\out\`, not `deps\`. Plain PowerShell `*>` redirection wraps a native tool's stderr
  at console width; `cmd /c "… > log 2>&1"` keeps lines whole.
- **2026-11-18, task 34: a volume per drive** (D8, `44b67fd1c2` through `76c34e657c`): drives, folder mounts, and mapped
  shares register as their own volumes, keyed by the disk (volumes/`DETAILS.md` § "Windows: a volume per drive"). On the
  builder: `cargo clippy -p cmdr -p cmdr-fs -p cmdr-index -p cmdr-operation-log --all-targets -D warnings` clean (2m 57s
  warm); nextest over the touched modules 107 of 113 before two index cells learned the system-drive root (fixed in
  `693033a8eb`); the other four, in `priority::roots`, fail on their POSIX fixtures (`/Users/…` isn't absolute on
  Windows), as they did before. A second run after `76c34e657c` (clippy clean again, 12m 25s): the `cmdr-index` paths,
  walker, `notify`, and phases cells 127 of 147; the 19 `lifecycle::phases` failures are #1232's native-tempdir cluster,
  and `a_run_of_walks_reuses_its_threads` is a thread count that passed in the first run.
  - **Win32 facts, verified** (PowerShell P/Invoke over SSH, Windows 10 22H2, 2026-11-18; recorded in
    `volumes/platform/windows/drives.rs`): a fixed drive gives its `\\?\Volume{…}\` GUID, and the USB build SSD reports
    `DRIVE_FIXED`; a `subst` drive has no GUID (`ERROR_INVALID_PARAMETER`), `QueryDosDeviceW` names its folder
    (`\??\C:\Windows`), and its serial is the target drive's; a mapped drive (`net use` to `\\localhost\C$`, removed
    after) is `DRIVE_REMOTE` with no GUID, `WNetGetConnectionW` names the share, and its serial is the server's; an
    empty optical drive has a GUID but `GetVolumeInformationW` answers `ERROR_NOT_READY`. Not verified: GUID stability
    across a USB stick's re-plug (none plugged in; the build SSD stays put).
  - **The launch** (`run-on-desktop.sh`, started from `D:\` on purpose; screenshots in David's
    `~/Downloads/cmdr-windows-volumes/`): the root index walked `C:/` (its `WholeVolume` phase), with watcher branches
    as `C:/Program Files`, where the first launch walked `D:\` and watched `/C:/Windows`; `D:` registered as
    `vol-caa3226d-…`; the switcher (`Alt+F1`) lists `C:` and `build (D:)` with NTFS and free space, and the empty `E:`
    not at all; switching to `D:` lists it and offers to index it; the path bar reads `C:\`, `C:\Windows`, and `D:\`;
    `volume-roots.json` saved both roots; the MCP `root` row's `mountPath` is `C:/`.
  - **Seen, not fixed**: the phases' `Home` walk logged `C:\Users\Admin` native-spelled (fixed in `76c34e657c`, not
    relaunched); MCP `list_volumes` and `cmdr://state` list `root` alone on Windows (the non-macOS snapshot); the "Turn
    indexing on?" dialog names the volume by its raw id; a short first segment (`C:`) keeps the 26 px hit-width floor,
    so the path bar shows a gap before the first `\`; SendKeys `Enter` didn't press that dialog's focused Allow.
- **2026-11-18, the `LocalVolume` alias** (app-path plan milestone 2, `bc690227d6`): consumers that mean "this machine's
  local disk" name `cmdr_local::LocalVolume`, and the Windows arms of `volumes/platform/` and `local_disk/platform/`
  name it too. Windows compiles exactly the code it compiled before: every renamed `LocalPosixVolume` site was already
  behind `cfg(unix)` (else the Windows lane couldn't have built it), and the Windows arms name the same type through the
  alias. So the alias changes no Windows test outcome by construction; the tests it can turn on stay gated until their
  crate's app-path milestone (#1225's list). On the builder:
  `cargo clippy -p cmdr-local -p cmdr-operations -p cmdr-restic -p cmdr --all-targets -D warnings` clean (25m 44s cold,
  shared with another agent's build); `cargo nextest run` over the same four: 7,327 tests, 6,788 passed, 528 failed, 11
  timed out, 112 skipped (237 s). Per crate: `cmdr-local` 30 of 30; `cmdr-operations` 1,455 of 1,747; `cmdr` 5,221 of
  5,404; `cmdr-restic` 82 of 146. Against the earlier runs above the counts moved with the work that landed in between,
  not with this rename. The Mac's zig cross-check also needs `CXX_x86_64_pc_windows_gnu` (`zig c++`, for
  `link-cplusplus`), a fresh `CARGO_TARGET_DIR` (a cloned `target/` carried stale Windows artifacts, E0461), and zig's
  binary named by path, since mise's `zig` shim has no version outside a pinned project; `cargo clippy` there still
  fails on host and target artifacts mixing (E0461), so Windows clippy runs on the builder.
- **2026-11-18, the app-path baseline** (app-path plan milestone 1, the run at `e03f087946`): the first full-workspace
  test run on the builder, `cargo nextest run --workspace --exclude cmdr-fsevent-stream --no-fail-fast` in a fresh clone
  (`D:\src\cmdr-m1`, `CARGO_TARGET_DIR=D:\targets\t34`), 23m 02s to build the tests warm-ish, then 613 s: 13,831 tests,
  12,897 passed, 869 failed, 65 timed out, 669 skipped. `cmdr-fsevent-stream` is macOS-only and fails to compile there
  (`core-foundation`), so a workspace run has to leave it out. Every failure, by name, crate, and cause cluster, is
  `docs/notes/windows-port/app-path-baseline.md`, rendered from the run's JUnit report by
  `scripts/windows-test-baseline` (`render`; a later run diffs with `diff`, and `explain` shows what each cause was read
  from). Per crate: `cmdr-operations` 297, `cmdr` 182, `cmdr-index` 157, `cmdr-restic` 67, `cmdr-adb` 52, `cmdr-nfs` 49,
  `cmdr-s3` 48, `cmdr-webdav` 23, `cmdr-viewer` and `cmdr-smb` 13 each, `cmdr-archive` 11, `cmdr-sftp` 7,
  `cmdr-listing-wasm` 7, `cmdr-listing` and `cmdr-operation-log` 3 each, `cmdr-mtp` 2; `cmdr-fs`, `cmdr-local`, and the
  rest none. By cause: `std::path` on an app path (S) 406, a native tempdir where an app path belongs (N) 333, timed out
  71, #1226's open files 53, task 35's identity 32, Windows-only expectations 13, unread 13, task 34's root volume 6, a
  Linux errno through host codes 4, product gaps 3; 407 of them name no cause in their output and take their nearest
  sibling's cluster, marked "cause unread". Against the plan's round-four reading: `cmdr-operations`' finalize cluster
  (F) never names `rename_local_exclusive` in a panic, so its cells sit under N by their tempdir paths; `cmdr-index`
  timed out on 85, not 47; and `cmdr-adb`'s 51 path cells read as `NotFound("\\sdcard")`, a POSIX path re-rooted by
  `std::path`. Gotchas: a job launched over SSH dies with the session, so it runs through the infra repo's
  `run-on-desktop.sh` (detached, in Admin's desktop session); nextest wrote the JUnit report to
  `target\nextest\default\target\nextest\default\junit.xml` under the checkout, not under `CARGO_TARGET_DIR` (kept as
  `D:\logs\m1\baseline-junit.xml`); the timed-out cold-drive tests left 47 `crates/cmdr-index/cmdr-cold-cover-*` trees,
  removed after the run along with 46 older ones in `D:\src\cmdr`; and a `cmdr-index` proptest left a
  `proptest-regressions/indexing/` seed in the checkout.
