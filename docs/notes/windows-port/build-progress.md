# Windows build progress

A running log of the Windows build track (#942, `docs/specs/windows-port-plan.md` § "Windows build state"), one entry
per milestone, newest last. The machine is the Windows 10 22H2 builder (`ssh mami`, MSVC; how to reach it, build there,
and run the GUI: `docs/tooling/ci.md` § Windows lane); the fast loop is `cargo check --target x86_64-pc-windows-gnu` on
a Mac with zig as the C compiler and `AWS_LC_SYS_PREBUILT_NASM=1`: `CC_x86_64_pc_windows_gnu` names a script that drops
cc-rs's `--target` flag and runs `zig cc -target x86_64-windows-gnu`, and `AR_x86_64_pc_windows_gnu` runs `zig ar`. Two
things it can't judge: the app's build script (no `windres`) and `numkong`'s AVX-512 kernels, which zig's clang refuses.

## Builder hygiene

Mami got too slow for David to use after about 24 hours of agent builds (2026-11-19: 11 leftover checkouts and five
target dirs, about 170 GB on `D:`, plus idle `cmd` windows on the desktop). So:

- **One agent at a time, and ask David before heavy use** (a cold workspace build, a whole-workspace nextest, a bundle).
  Check first: `Get-Process cargo,rustc,link,node,cargo-nextest,cmdr -EA SilentlyContinue` must come back empty.
- **`CARGO_BUILD_JOBS=2`** for every build: 2 cores, 8 GB of RAM, and more jobs run out of page file (`os error 1455`).
- **Keep one warm pair, delete the rest**: one checkout `D:\src\cmdr-<slug>` with one `D:\targets\<slug>`, the most
  recent. When you're done, delete your checkout and target dir unless you're the newest pair, and check first that the
  checkout has nothing unlanded (`git status`, and its `HEAD` is on `origin/main` from the Mac, since the box can't
  push). `D:\src\cmdr` (the main checkout), `D:\src\mtp-rs`, `D:\src\menu-spike`, and `D:\tools\winapp-0.8.0` stay.
- **Clean up after a desktop run**: a `start ... cmd /K` job leaves its window open, holding its folder; close it.
- **Defender skips the build drive**: real-time scanning excludes `D:\` (set by the infra repo's
  `mami-laptop/setup-builder.ps1`), so `D:\src` and `D:\targets` are covered; on Windows, scanning every fresh `rlib`
  and object is the biggest cost in a Rust build. Real-time protection stays on for everything else (verified
  2026-11-19, `Get-MpPreference`, `Get-MpComputerStatus`). No process exclusions.

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
- **2026-11-18, task 34's leftovers** (`f399bcaeb6` through `324b72e7d8`): MCP `list_volumes` and `cmdr://state` list
  `C:`, `build (D:)`, and Servers on Windows (the shared listing pipeline, `tools/resources/DETAILS.md`), the "Turn
  indexing on?" dialog reads "build (D:)", and the path bar's first segment carries its hit floor as an overlay, so
  `C:\Windows` shows no gap. `a_run_of_walks_reuses_its_threads` failed 3 of 60 nextest stress runs under CPU load (21
  threads for 20 walks: `walk` returned before its workers re-idled in the pool); with the engine waiting for its live
  workers, 200 of 200 pass (`walker/DETAILS.md` § The engine). On the builder:
  `cargo clippy -p cmdr -p cmdr-index --all-targets -D warnings` clean (4m 08s warm), nextest over the touched modules
  64 of 64, and a `pnpm tauri build --debug` launch from `D:\` with MCP over `ssh -L` confirmed all three by tool call
  and screenshot.
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
- **2026-11-18, the typed app path** (app-path plan milestone 3): `cmdr_fs::app_path` exists and nothing adopts it, so
  no Windows outcome moves outside `cmdr-fs`'s own new cells. Its native conversions are pure (bytes and UTF-16 units as
  data), so the Windows arm's refusals (`:`, `NoPortOnUnc`, `NotPlainSafe`, the verbatim `.`/`..`/empty names, `C:` and
  `C:foo`) run on every OS; only the `OsStr` crossing sits behind `app_path/platform/`. The Mac's zig cross-check
  `cargo check --all-targets --target x86_64-pc-windows-gnu` is clean for `cmdr-fs` (with `testing`) and `cmdr-local`,
  and the wasm build of `cmdr-fs` too. On the builder, `cargo nextest run -p cmdr-fs --features testing` in a fresh
  checkout (`D:\src\cmdr-m3`, `CARGO_TARGET_DIR=D:\target-m3`, `7eba6045c3`): 858 of 858 passed, 26 skipped (the `#1225`
  ignores), and `cargo clippy -p cmdr-fs -p cmdr-local --all-targets -D warnings` clean. The first run's two reds were
  the cells, not the type: deserializing a POSIX path is refused on a Windows host (the `from_wire` fence), and the
  allocation cell's comparison caught today's `PathBuf::join` writing a `\` into an app path (#1225's bug, by
  construction). Cmd-shell recipe that works there:
  `cmd /c "cd /d <dir> && set "CARGO_TARGET_DIR=<dir>" && cargo nextest run ... > <log> 2>&1"` (an unquoted
  `set X=Y && …` keeps the trailing space in the value).
- **2026-11-18, the typed app path's review fixes** (`3951274cdf`): comparison now reads text with the host's
  `Path::components`, so the builder ran the Windows property against `PathBuf` (4,000 cases over drive, UNC, verbatim,
  and mixed-separator text): its first runs found where the type's prefix reading departed from std's (a UNC prefix
  needs a server AND a share, `\\.\` takes either separator, a UNC or device prefix implies a root), now mirrored.
  `cargo nextest run -p cmdr-fs --features testing` 868 of 868 (26 skipped, the `#1225` ignores), and
  `cargo clippy -p cmdr-fs -p cmdr-local --all-targets -D warnings` clean (it caught a `manual_strip` the Mac's clippy
  lane didn't).
- **2026-11-18, the typed app path integrated and its second review fixed** (`0f464fc516`, the pins and the type merged
  onto `main`; `docs/notes/windows-port/app-path-type-review-fixes.md`): in `D:\src\cmdr-m3` with
  `CARGO_TARGET_DIR=D:\target-m3`, launched at low priority through `run-on-desktop.sh` while another agent's `cmdr`
  build held the box: `cargo nextest run -p cmdr-fs --features testing` 899 of 899 (26 skipped, the `#1225` ignores),
  the Windows `PathBuf` comparison property over the text Win32 opens included, and
  `cargo clippy -p cmdr-fs -p cmdr-local -p cmdr-operation-log --all-targets --features cmdr-fs/testing -D warnings`
  clean. Its first runs found two test-build breaks (an import only the Unix column reads, a method only a Unix cell
  called; the Mac's zig cross-check, `--all-targets` over `cmdr-fs`, `cmdr-operation-log`, `cmdr-operations`, and
  `cmdr-local`, then agreed) and a property whose `prop_filter` rejected most cases, which timed out at 8 s on the
  2-core box and now builds its input.
- **2026-11-18, task 52: the release packages** (`3224901455` plus the task's tooling, a release
  `pnpm tauri build --no-bundle` in a cold `D:\targets\t52`, about two hours on the 2-core builder, most of it the
  `cmdr` crate): one compile, three packages, each installed and started on the desktop (screenshots in David's
  `~/Downloads/cmdr-windows-release/`).
  - **NSIS**: `tauri bundle --bundles nsis` made `Cmdr_0.61.0_x64-setup.exe` (36.90 MiB) with no signing step
    (`createUpdaterArtifacts` off on Windows), and `release-assets.sh vet-nsis` passed it (version resource 0.61.0, no
    Authenticode). `/S` installed it per user into `%LOCALAPPDATA%\Cmdr` (`Cmdr.exe`, `resources\`, `uninstall.exe`)
    with an HKCU uninstall entry naming "Rymdskottkärra AB" and 0.61.0; the installed exe carries the NSS stamp, and its
    updater ran a real check ("No update available", so the NSIS install path reads as updatable). Started from
    `run-on-desktop.sh`, it listed `C:\Windows` and `D:\` (`1-nsis-installed-running.png`), closed with Alt+F4, and
    `uninstall.exe /S` removed the folder, the uninstall entry, and the shortcuts, leaving the data dir.
  - **Bundler stamp**: tauri-cli 2.12 stamps the copy it installs and leaves `target\release\Cmdr.exe` byte-identical
    (same SHA-256 before and after, still `…_UNK`), so copying the exe first is insurance, not a must, today.
  - **MSIX**: `windows-package msix -identity dev` (winapp 0.8.0, unpacked from its release zip; it fetched
    `Microsoft.Windows.SDK.BuildTools` 10.0.28000.2957 on first use) made `Cmdr_0.61.0_x64.msix` (55 MB) and
    `verify-msix` passed it; the release identity failed on its placeholder as intended. Windows 10's Appx module has no
    `-AllowUnsigned`, and Developer Mode registration refused the unsigned-marker publisher (0x80073D2D), so the dev
    identity is plain `CN=Cmdr Dev`; with Developer Mode on (and off again after), the unpacked package registered,
    started from `shell:AppsFolder\Rymd.Cmdr.Dev_v99ez5794820m!Cmdr`, listed both drives
    (`2-msix-registered-running.png`), closed, and unregistered. The first pack also shipped the manifest template
    beside `AppxManifest.xml`, since winapp packs its whole input folder; the template now stages outside it and
    `verify-msix` refuses a stray one. The packaged run logged into the existing
    `%LOCALAPPDATA%\com.veszelovszki.cmdr\logs\cmdr.log` and logged no update check within three minutes (task 57).
  - **Group Policy zip**: `cmdr-group-policy-0.61.0.zip` (4.7 kB) holds `cmdr.admx`, `rymd.admx`, and `en-US/` only.
  - `go test` for `scripts/windows-package` and `scripts/release-assets/{nsis,manifest}` passed on the builder too.
- **2026-11-19, the failures sorted by cause** (`main` at `8db5b83ac4`, then the fix branch `253f57d74f`): a fresh
  whole-workspace run in `D:\src\cmdr-triage` (`CARGO_TARGET_DIR=D:\targets\triage`): 13,978 tests, 13,037 passed, 875
  failed, 66 timed out. Every failure's cause, by cluster, with what #1225 takes and the product bugs it found (the
  in-flight ledger never emptying on Windows, Windows-made zips dropping their modes, both fixed):
  `docs/notes/windows-port/windows-test-failures.md`, with the machine-readable record beside it. The cold build died
  once on `os error 1455` (the page file too small to mmap an rlib); `CARGO_BUILD_JOBS=2` got through.
- **2026-11-19, task 46's small arms** (`85c8ddb0d4`, in `D:\src\cmdr-t46` with `CARGO_TARGET_DIR=D:\targets\triage`,
  run on the desktop through `run-on-desktop.sh`; plan task 46 has what each arm is):
  - `cargo clippy -p cmdr-fs -p cmdr-adb -p cmdr-http --all-targets --features cmdr-fs/testing -D warnings` clean;
    `cargo clippy -p cmdr --all-targets -D warnings` clean (10m 03s warm-ish; vendored `rustic_core`'s known warning).
  - **Thread QoS**: Windows accepts both `SetThreadPriority(BELOW_NORMAL)` and the EcoQoS throttle, and the priority
    reads back; Windows 10 can't read the throttle back (`GetThreadInformation(ThreadPowerThrottling)` answers
    `ERROR_INVALID_PARAMETER`, 87), so that half is checked on Windows 11 only.
  - **`adb.exe`**: with Google's platform-tools 37.0.1 in `D:\tools\t46` (`CMDR_TEST_ADB`), a refused connect started a
    real server through `start_quietly` (stderr piped, no hang) and `host:version` answered, in 4.9 s; a child started
    quietly reads `GetConsoleWindow` null. 13 of 13 `server` cells.
  - **Special folders**: `FOLDERID_Downloads` classifies in both spellings (`C:\Users\…` and `C:/Users/…`); the
    favorites seed's known folders resolve and exist (3 of 3 `favorites::platform` cells; this builder keeps them under
    the profile, so the OneDrive case is a unit cell only).
  - **Proxy**: 50 of 50 `cmdr-http` cells, the shared PAC ones against WinHTTP and a loopback PAC file. WinHTTP drops a
    PAC's trailing `DIRECT` (`PROXY proxy.test:3128; DIRECT` answers `proxy.test:3128`), reading the user's settings
    takes under 2 ms, and WPAD with no WPAD server fails in about 70 ms. The builder's own settings stayed untouched, so
    a real manual proxy, setup script, or WPAD network is unverified.
- **2026-11-19, task 46's review fixes** (in `D:\src\cmdr-t46`, default target dir, over ssh; the builder's settings
  read-only: auto-detect on, no setup script, no manual proxy, no WPAD server on its network):
  - **WinHTTP's two PAC calls, side by side** (a PowerShell P/Invoke probe and a loopback PAC server): the synchronous
    `WinHttpGetProxyForUrl` answers "no proxy" for `SOCKS s:1080`, `SOCKS5 …`, and even `SOCKS s:1080; PROXY p:3128`,
    turns `HTTPS h:443` into the text `https://h.test:443`, and drops a trailing `DIRECT`. The async resolver
    (`WinHttpGetProxyForUrlEx`) reports each entry typed (SOCKS scheme 4, HTTPS 2, `DIRECT` kept; a versioned SOCKS
    keyword arrives as host `"5 s.test"`), and closing the resolver cancels a stalled evaluation at once (`12017`, then
    `HANDLE_CLOSING`). With session timeouts of 3 s, the synchronous call gave up on a stalling server after 3.58 s.
  - **`C:\ProgramData`'s ACL** (`icacls`): `BUILTIN\Users:(CI)(WD,AD,WEA,WA)` plus `CREATOR OWNER:(OI)(CI)(IO)(F)`, so
    any standard user can create `C:\ProgramData\chocolatey\bin` where Chocolatey isn't installed (it isn't here).
  - **Launch routing** (`windows_test.rs::measure_launch_routing`, three runs each; the warm-up call, then five hosts'
    first routing at once, then a warm route): before, `warm_up_pac` returned in 0.35–0.56 ms, the burst took 21–26 ms
    (each host 12–25 ms, every one running WinHTTP's WPAD call), and a warm route 2.5–2.8 ms. After (WPAD discovered
    once in the background): the warm-up call 0.37–0.88 ms, the burst 12.9–13.3 ms (all five hosts sharing the one
    discovery, 12.3–12.9 ms each), a warm route 2.6–2.7 ms. Launch isn't slowed: nothing synchronous grew, and the burst
    shrank.
  - **Cells**: 72 of 72 `cmdr-http` (the async resolver's typed SOCKS, SOCKS4/5, HTTPS, and ordered list with its
    trailing `DIRECT`; a stalling PAC server given up on within its budget; the address-change subscription's callback
    reaching the network number; the WPAD state machine; the per-network PAC cache), and 5 of 5 `cmdr-adb`
    `binary_search` (the Windows search pinned to user- or admin-controlled folders).
- **2026-11-19, task 40: the system look** (`7a2d5eb037`, merged with `main` at `d3bbc2aeea`): the accent with WinUI's
  fill shades, contrast themes, Transparency effects, and the language and Regional format, all live from the system
  look's settings window (`apps/desktop/src-tauri/src/system_look/platform/windows/`); Text size stays WebView2's own
  zoom. The Mac's zig cross-check of `cmdr` (`--all-targets`, a stand-in `windres`) was clean first time; the Windows
  lane's clippy ran green on the branch. On the builder, in `D:\src\cmdr-t40` with `CARGO_TARGET_DIR=D:\targets\t40`
  (seeded from `D:\targets\triage`, yet most workspace members and several dependencies rebuilt: 80 min at
  `CARGO_BUILD_JOBS=2`), `cargo nextest run -p cmdr system_look intl::`: 79 of 79, the real-machine cells included
  (`UISettings` activates in an unpackaged process and answers `#0078d7` with `AccentDark1` `#005a9e` and `AccentLight2`
  `#76b9ed`, the change events register, `GetUserPreferredUILanguages` resolves to a shipped catalog, and the Regional
  format composes a well-formed tag). Gotchas: the box's disk is slow under load (the target copy ran at about 9 MB/s
  and a `git reset --hard` across 11,678 files took 53 min while it ran), and a command started over SSH can outlive the
  session, so check for a leftover `git` before blaming a stale `index.lock`. Only read on the builder, never changed
  (the settings belong to the box): that each Settings page broadcasts or fires its `UISettings` event, a contrast
  theme's and another accent's look, the Text size zoom, and a non-English language all wait for a human on a real
  Windows.
- **2026-11-19, the first real-desktop check of the menus, browser keys, and look** (`main` at `4314b3cf1f`, a debug
  `pnpm tauri build --no-bundle` in `D:\src\cmdr-verify`, 17m 58s seeded from `D:\targets\triage`): the in-page bar,
  right-click and text-field menus, the `SC_KEYMENU` guard, and the browser-key switch pass in every window; three
  failures were fixed and re-run on `968733ae7f` (see-through Settings and queue, no ring after a lone Alt, focus
  stranded on a title). Per-check evidence and the open problems (a Copy dialog that refuses every local copy first):
  `docs/notes/windows-port/real-windows-check-2026-11-19.md`.
- **2026-11-21, app-path milestones 7 and 8 on the builder** (`main` at `97befe7c07`, the warm pair `D:\src\cmdr-m6` +
  `D:\targets\m6`, 9 min warm for nine crates): 2,095 tests, 6 red where `windows-test-failures.tsv` had 164; per crate
  and per cause in `docs/notes/windows-port/windows-test-failures.md` § "By milestone". Elevated through
  `run-on-desktop.sh --elevated`, a run holds the symlink privilege, so nothing skipped itself.
