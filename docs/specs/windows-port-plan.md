# Windows port plan

**Cmdr runs on macOS (and builds on Linux), and had zero Windows code when this plan started.** This plan gets there
gradually: most steps are refactors that make the codebase better on their own (platform seams, decoupling, typed
errors), each landable on a Mac, so the Windows-only work at the end shrinks to filling in arms behind seams that
already exist.

Status: planned 2026-11-04 at `7a199a9705`. Tracking issue: [#942](https://github.com/getcmdr/cmdr/issues/942). Evidence
(static reads, no Windows compile yet):

- `docs/notes/windows-port/crates-audit.md`: every crate under `crates/`, per concern, with the Windows API per site.
- `docs/notes/windows-port/app-audit.md`: the app crate, feature by feature, and the six dispatch patterns in use.
- `docs/notes/windows-port/frontend-build-audit.md`: frontend, Tauri config, CI, release, website, api-server, tests.
- Linux precedent: `docs/specs/linux-builds-plan.md` and `docs/notes/linux-gaps-2026-08-10.md`. Most wins below close
  Linux gaps too.
- An earlier rough spec (`linux-and-windows-versions.md`, 2026-01-18, deleted in `eac9e6187f`) predates almost all of
  today's code; read it with `git show eac9e6187f^:docs/specs/linux-and-windows-versions.md` for history only.

## Where we started

The state at planning (2026-11-04). What compiles for Windows now: § Windows build state.

- **The crate boundary isn't an OS boundary.** "Nothing below imports the app" (`index-crate-isolation`) says nothing
  about the OS. `cmdr-operations`, `cmdr-index`, `cmdr-fs`, and `cmdr-local` hold about 900 unix/macOS sites; the
  network backends, `cmdr-listing`, `cmdr-collation`, `cmdr-menu`, `cmdr-media-meta`, and `cmdr-operation-log` are
  nearly clean.
- **The app crate**: 1,156 `macos` cfgs in 188 files, 269 `linux`, zero `windows`. The "other OS" arm (`stubs/`) has
  never been compiled and doesn't (`stubs/volumes.rs:194` calls `libc` off unix).
- **Nothing compiles for Windows today, for manifest reasons first**: unconditional unix-only deps (`xattr`, `uzers`,
  `exacl`, `libc`) in the crates, and cross-platform deps (`url`, `semver`, `pulldown-cmark`, …) declared for
  `any(macos, linux)` only in the app.
- **Gates encode "two platforms"**: about 180 `not(target_os = "macos")` arms mean "Linux" by accident, and 194
  `any(macos, linux)` gates switch pure-Rust features (ADB, SFTP, WebDAV, S3, NFS, saved servers) off on Windows only
  because they sit next to OS-mount code.
- **Data-safety hazards**: `not(unix)` stub arms in `cmdr-operations` compile on Windows and answer the unsafe way
  (`is_same_file` → `false`, `validate_destination_writable` → `Ok`). Errors are classified by errno, but Windows hands
  back Win32 codes. Both must be fixed by seams, not patched per site.
- **The frontend knows only "macOS or not"**, by sniffing the user agent (74 sites), and the release pipeline, website,
  and api-server hardcode three macOS builds.

## The architectural wins

Each is worth landing even if Windows never ships. Together they're most of the port.

1. **Platform seams, one shape everywhere.** `<feature>/platform/{mod,macos,linux,windows}.rs`, with `mod.rs` pinning
   every exported signature (`const _: fn(&Path) -> Option<String> = imp::fetch_icon;`) so each arm must match, a
   `compile_error!` for unknown targets, and a check that keeps platform cfgs inside seams. Kills six ad hoc dispatch
   patterns, 63 IPC commands defined once per OS, and about 1,000 lines of hand-mirrored stubs; gives Linux compile-time
   parity with macOS.
2. **Remote backends stop depending on OS-mount code.** Un-gating ADB, SFTP, WebDAV, S3, NFS, and saved servers from
   `any(macos, linux)` and putting the OS-mount half of `network/` behind its own seam is the biggest layering fix.
3. **Typed OS-error classes at the volume boundary.** Classify once in `VolumeError::from_io_at`; consumers match a
   class, network backends construct one instead of faking `libc::EIO`. Raw errnos stop crossing layers (their numbers
   differ per OS), which is the type-level version of the no-string-match rule.
4. **One file-identity vocabulary** (`FileId`, `same_node`, `device_of`, `link_count`) replaces about 30 scattered
   `dev()`/`ino()` sites asked three different ways, and deletes the unsafe "don't know, say no" stubs.
5. **The backend tells the frontend what exists.** A synchronous platform value plus a typed capability set (Quick Look,
   Services, FDA, drag-out, OS file-manager name, …) replaces the UA sniff, so features hide on purpose and tests can
   run each platform.
6. **Name validity follows the destination filesystem, not the host OS.** An exFAT stick or an SMB share on a Mac takes
   Windows' forbidden names but stores something else (private-use stand-ins, a mangled 8.3 name for `CON`), so this
   fixes real macOS cases and makes Windows a data entry.
7. **N-platform release and services.** One release asset manifest drives `latest.json`, checksums, SBOM, vetting, and
   the website; downloads and telemetry gain an explicit `os`. The Linux release falls out of the same work.
8. **Smaller seams with Linux payoffs**: a watcher backend trait (Windows then gets watching from `notify`), one mount
   façade, process stats behind one module (Linux gains the memory watchdog it lacks), platform-variant UI copy (Linux
   needs it too), a per-platform keymap (task 26, which also fixed the `Ctrl+D` clash off macOS).

## Decisions

Settled:

- **Image ML indexing (Vision, Core ML CLIP) doesn't ship on Windows.** An explicit "unavailable" backend reports it
  off, and the setting hides. `Windows.Media.Ocr` can come back later as its own effort.
- **Native window decorations on Windows** (as on Linux): no custom title bar fighting Snap Layouts.
- **NSIS installer, per-user (`currentUser`), `embedBootstrapper` for WebView2.** MSI only if enterprise asks.
- **D1. The path model** (David, 2026-11-04): every local path on Windows is an absolute forward-slash path whose first
  component names its root: `C:/Users/test_user_1` for a drive, `//server/share/x` for a UNC share. Each drive and UNC
  share is its own volume, rooted at `C:/` or `//server/share/`. The drive letter is always uppercase. Backslashes exist
  only inside the Windows local backend (`\\?\C:\…` for long paths) and in the one display formatter. Case sensitivity
  is a per-volume property (`caseSensitive`), never inferred from the OS.
  - Why: it's the rule `crates/cmdr-fs/src/volume/remote_paths.rs` already sets for remote volumes, that an app path
    names its own volume. So the grammar becomes "a scheme URL, or a `/`-separated absolute path whose first component
    is the root". Every existing `/` arithmetic in the frontend, the index, and `lib/path` stays correct; `lib/path`
    only learns the two new root shapes; Win32 APIs accept `/` everywhere except the `\\?\` form.
  - ❌ Rejected: a drive as a volume rooted at `/` with volume-relative paths (`/Users/test_user_1` plus a volume id).
    Those are exactly the bare paths `remote_paths.rs` forbids: a restored tab, favorite, drag, or go-to-path that loses
    the volume hint resolves to the wrong disk. Also rejected: native `\` on the wire (touches ~110 frontend sites and
    every Rust↔TS fixture) and MSYS-style `/c/Users` (unfamiliar, and pasted paths need translating).
- **D2. MTP ships on Windows** (David, 2026-11-04), through `mtp-rs`'s WPD COM backend. `mtp-rs` picks it automatically
  behind its high-level `mtp::` API, with no driver install (verified on a Pixel 9 Pro XL, per its README), and
  `cmdr-mtp` uses only that API, never the USB-only `ptp::` layer. Windows' MTP driver can stall any call for 5–6
  minutes after files change on the phone; that's a Windows quirk every app and library meets (File Explorer too), so
  Cmdr shows it as a "the phone is busy" state with a timeout, never as a hang or an error. Task 56. The crates audit's
  MTP note (WinUSB driver binding) predates this and is superseded.
- **D4. Minimum Windows version** (David, 2026-11-04): Windows 10 22H2 and Windows 11, x86_64 first, ARM64 later. Drop
  Windows 10 when WebView2 does (Microsoft promises WebView2 updates on 10 22H2 until at least 2028-10).
  - Why: Windows 10 is 25.13% of Windows desktops (StatCounter, 2026-10; Windows 11 74.29%, the rest about 0.6%), and
    Total Commander's audience likely skews to it. Code cost is a few fallbacks where Windows 11 has extras (EcoQoS
    thread mode, Mica, 24H2 block-clone copies), plus a Windows 10 VM in each release's QA.
  - Hard floor: Windows 7/8/8.1 are out. WebView2 stopped updating there in 2023-01, and Rust's tier-1
    `x86_64-pc-windows-msvc` has required Windows 10 since 2024 (the Win7 target is tier 3, no prebuilt std). Total
    Commander reaches Windows 95 only because it's a native Win32 app with no browser engine.
- **D6. App paths are a type** (David, 2026-11-18, #1225 option a): the `Volume` trait takes a typed `AppPath`, so a
  native path can't go where an app path belongs, and a consumer that means "this machine's local disk" names
  `cmdr_local::LocalVolume` (the lead, on David's delegation). Plan: `docs/specs/app-path-plan.md`. (Numbered D6 because
  the menu model's branch already holds D5.)

- **D5. The menu model** (PROVISIONAL, 2026-11-18; recommended by the lead's research, pending David's spikes on two
  real Windows laptops): off macOS, Cmdr draws its menus in the page from `cmdr-menu`'s data. Windows, KDE, and other
  Linux desktops get an in-window menu bar (`windowMenuBar`), GNOME a "Main menu" button opened by F10 (`primaryMenu`,
  new), and a "Show menu bar" setting flips between the two. muda keeps macOS's bar and every right-click menu. Task 63;
  evidence and rejected options: `docs/notes/windows-port/menu-model.md`.
  - **Why provisional**: David prefers native menus if they're little work, and expects Tauri to close gaps over time.
    The recommendation rests on evidence nobody ran on our stack, so no custom-menu build starts before the spike.
  - **What the spike must answer** (`tools/menu-spike/`, test script and results in
    `docs/notes/windows-port/menu-spike.md`), on Windows 10 and Windows 11:
    1. With focus in the page, do a lone Alt, F10, and Alt+letter reach muda's native bar (highlight it, open the right
       menu), and does Escape give focus back to the page?
    2. Do the bar's accelerators fire while the page has focus, and does any key fire twice (page and menu)?
    3. Does the native bar and its menus follow dark and light mode, and high contrast, well enough?
    4. Do AltGr combos on a second layout type their character, not open a menu?
    5. Does Narrator read each mode's bar as a menu bar?
  - **First measurement** (the agent's selftest on mami, Windows 10, 2026-11-18): on today's muda bar a lone Alt and F10
    work, Alt+letter and the bar's accelerators don't; a ~40-line WebView2 key bridge fixes Alt+letter; native popups
    stayed light with only the window forced dark. Details: the spike note.
  - **Decision rule**: the native bar if 1–3 are acceptable (with a small key bridge if that's what it takes); the
    hybrid (an in-page title strip opening native popups) if only the bar strip fails; the in-page bar only if both
    fail.

- **D3. Code signing and distribution** (David, 2026-11-04): Store first, a paid certificate only once Windows pays for
  it.
  1. **The Microsoft Store, as an MSIX package, is the primary Windows channel.** Microsoft signs it for free (the
     company developer account is free too), so there's no SmartScreen warning and Smart App Control lets it run from
     day one. The Store delivers updates, so the Store build turns Cmdr's own updater off. Slower hotfixes (Store
     certification per release, hours to days) are accepted. MSIX also gives the package identity Windows 11's top-level
     context menu needs.
  2. **An unsigned "Run anyway" NSIS installer on the website**, built from the same release binaries, for people who
     avoid the Store. The download page explains SmartScreen's "More info → Run anyway" step. Its updates travel the
     Tauri updater, trusted by the minisign signature plus the Rust-side checks (task 33).
  3. **Later, once Windows sales cover it**: Azure Artifact Signing through Rymdskottkärra AB ($9.99/month) signs the
     direct download, which retires the "Run anyway" step.
  - Why: the Store gives the highest OS trust at zero cost, and a signed direct download buys less trust than that for
    $119.88/year (SmartScreen still warns until reputation builds). Total Commander's route, a Sectigo OV certificate
    for its company (valid 2025-09-26 to 2028-09-25, read from `tcmd115801x64.exe`), costs more for the same trust as
    Azure. Since 2024, EV no longer buys instant SmartScreen reputation either.

Open, waiting on David (each task entry has the detail):

- **The drive and UNC volume id scheme** (task 34).
- **The menu model** on Windows and per Linux desktop (task 63): D5 stays provisional until David tries the menu spike.
- **macOS hand checks** from the keymap work (task 26).

## Bugs the audits found on the way

- **Linux: image indexing writes invented data.** Off macOS the production media backend is `FakeVisionBackend`
  (`crates/cmdr-index/src/media_index/scheduler/lifecycle.rs:231`), and the setting isn't hidden
  (`apps/desktop/src/lib/settings/definitions/indexing.ts:91`), so turning it on fills `media.db` with OCR text and tags
  derived from file names. Fixed in [#943](https://github.com/getcmdr/cmdr/issues/943) (task 1): off macOS the scheduler
  doesn't start, and the Image indexing settings page is gone.
- **Linux: dialog backdrops left a 29px strip live** at the top (`--titlebar-height` was unconditional, the title bar
  renders only on macOS). Fixed in task 24: `--titlebar-height` is 0 off macOS, the viewer's 35px override and the queue
  window's in-webview title line are macOS-only too.
- **Off macOS, `Ctrl+D` was bound twice**, so favorites never fired in the file list. Fixed in task 26: Duplicate is
  `Shift+F5` and the error screen's details toggle `Ctrl+Shift+D` there, and the conflict test runs with an empty
  allowlist on all three OSes.
- **api-server titled Linux reports "macOS Ubuntu …"** (`apps/api-server/src/report-cards.ts`). Fixed (task 2):
  `osLabel` shows the OS name as the app sent it and prefixes `macOS` only onto a bare version number.
- **Off macOS the updater skipped the advisory list and trust checks** (the frontend called the Tauri plugin's fused
  `check()` + `downloadAndInstall()`). Fixed in task 33: every OS runs Cmdr's own check, policy, and verification, and
  the plugin only runs the staged installer.
- **Linux licenses carried no device id.** Fixed in [#980](https://github.com/getcmdr/cmdr/issues/980): Linux now hashes
  `/etc/machine-id`, the precedent task 38's `MachineGuid` arm follows.

## Windows build state

Which members compile for Windows is `windowsPending` in `scripts/check/checks/member-coverage-allowlist.json` (the
check shrinks it). Milestones and what the builder ran: `docs/notes/windows-port/build-progress.md`.

Honest stubs: Windows arms that compile and answer "unsupported", "unknown", or nothing, each waiting for the task that
owns its real body. Remove a line when its body lands.

- `cmdr_fs::metadata` (`metadata/platform/windows.rs`), tasks 35 and 36: mode as the read-only attribute, xattrs as
  alternate data streams, the DACL, Hidden and Locked as attributes, and read-only as a removal blocker all answer
  `NotApplicable` or `Free`; only dates are real.
- `cmdr_fs::entry` owner and group names (`entry/platform/windows.rs`), task 35: always `None`, since a Windows owner is
  a SID, not a uid.
- `cmdr_fs::file_provider` (`file_provider/platform/windows.rs`), task 37: no File Provider marker; Cloud Files
  placeholders are read their own way.
- `cmdr_fs::mapped_file_faults` (`mapped_file_faults/platform/windows.rs`), task 47: recovers nothing, so a read of a
  mapping whose disk left still crashes; Windows' answer is a vectored handler for `EXCEPTION_IN_PAGE_ERROR` beside the
  crash reporter.
- `cmdr_fs::durable` directory syncs, task 36: `File::open` on a folder fails on Windows (it needs backup semantics), so
  `DeviceBarriers`' pass logs and skips each noted folder, and `cmdr-operations`' `fsync_dir` returns the error, which
  fails a copy's flush. Files sync through `open_to_sync`, which opens for write on Windows.
- `cmdr_fs::name_fold::normalize_for_comparison` (`name_fold/platform/windows.rs`), task 35: the drive index's key
  lowercases with Unicode rules, where NTFS folds through its own upcase table; they disagree on a handful of characters
  (dotted and dotless I, final sigma).
- `cmdr_fs::validation::MAX_PATH_BYTES`: Windows borrows Linux's 4,096 bytes. Task 14 kept path length on the host's
  limit for every volume, so a per-destination limit has no task yet.
- `cmdr_fs::process_memory` and `process_cpu` (each `platform/windows.rs`), task 13: no footprint and no process CPU
  census, so the watchdog, `memory_diagnostics`, and `cpu_diagnostics` are off on Windows. `thread_cpu` reads
  `GetThreadTimes`, so the index writer and rescan lines carry thread CPU there.
- `cmdr-sftp` agent (`transport/platform/windows.rs`), task 46: asks the OpenSSH agent's pipe (or `SSH_AUTH_SOCK`),
  never Pageant.
- `cmdr_local::LocalWindowsVolume` (`platform/local_windows.rs`), task 35: no read or write streams (so no copy onto or
  off it, no export), no copy scan, no `create_symlink` / `read_link_target` (`can_hold_links` false), no ADS-backed
  xattrs, no `apply_metadata` or metadata caps, no `write_access_at`, no trash, no listing watch, and no
  `FindFirstFileExW` lister. Each keeps the trait's conservative default, and each conformance cell it can't pass yet is
  `#[ignore]`d with its piece (`local_windows_conformance_test.rs`).
- `cmdr_local::reading` Windows arm (`reading/platform/windows.rs`), task 35: no allocation size, mode `0`, no owner or
  group; only `FILE_ATTRIBUTE_HIDDEN` is real.
- `cmdr_local::mounts` Windows arm (`mounts/platform/windows.rs`) and `filesystem_kind::probe_mount_for_path`, task 35:
  no mount table and no filesystem type, so every answer is "can't say" (its one test is ignored by name).
- `cmdr_operations::local_disk` Windows arm (`local_disk/platform/windows.rs`), tasks 35 and 36: `set_mode` and
  `symlink` refuse as unsupported (a copied link fails its item; a copied folder's mode narrowing is skipped), nothing
  reads as locked (`is_locked_item`, so a refusal never gets the "uncheck Locked" advice), and there's no
  purgeable-space figure. Real: the write probe (`FILE_WRITE_DATA` open), free space (`GetDiskFreeSpaceExW`), and the
  folder volume (`LocalWindowsVolume`).
- `cmdr_operations` local copy strategy on Windows (`transfer/copy_strategy.rs`), task 36: every file takes the chunked
  loop (cancellable, live progress, staged). No `CopyFileExW` yet, so a copy carries no attributes or alternate data
  streams (Mark of the Web included), and gets no ReFS block clone or SMB copy offload.
- `cmdr_operations` trash on Windows (`delete/trash.rs`), task 36: `move_to_trash_sync` refuses with the typed
  `TrashNotSupported`, and `trash_dir_for_path` answers `None`; no Recycle Bin yet.
- `cmdr_operations::delete::cloud_trash::metadata_is_online_only` on Windows, task 37: never online-only, the cautious
  branch, so a Cloud Files placeholder trashes (and hydrates) like any file.
- `cmdr_operations` extract quarantine on Windows (`transfer/volume/extract_quarantine.rs`), task 37: an extract carries
  no Mark of the Web.
- `cmdr_fs::file_identity` on Windows (`file_identity/platform/windows.rs`), task 36: every answer is `Unknown`, since
  `std::fs::Metadata` doesn't carry the file id there. The guards take the cautious-but-silent branch on it today: a
  copy onto a taken name duplicates to a ` (N)` name, a move onto one counts the item in place and leaves both, and a
  compress onto an existing archive refuses. Nothing is lost, but each is the wrong outcome until the path-based id
  reader (`GetFileInformationByHandleEx(FileIdInfo)`) and task 36's `Unknown` rule land.
- `cmdr-operations` tests on Windows: 65 test files that drive `LocalPosixVolume`, POSIX modes, or errnos are
  `#![cfg(unix)]` (three single cells too), and `lib.rs` allows `dead_code` and `unused_imports` in a Windows test build
  for the fixtures only they use. Each file un-gates as `LocalWindowsVolume` grows.
- Tests ignored on Windows by issue: 24 app-path tests in `cmdr-fs` (#1225, app paths through `std::path`) and one
  `sqlite_util` deletion test (#1226). `cmdr-index`'s suite isn't green there yet: 158 of 1,911 fail, filed as #1232.
- The app's root volume on Windows (`volumes/platform/windows.rs` `root_volume`), task 34: `LocalWindowsVolume` at the
  system drive alone (`%SystemDrive%`, else `C:/`), under today's `root` id and named "Root", which the switcher's
  main-volume row points at too. `resolve_path_volume_fast` answers that volume for a path on the system drive and no
  volume for any other drive or share, so Go to path and favorites reach `C:` alone. Which drives and UNC shares
  register, and under which ids, is task 34's open decision.
- The root drive index on Windows (`cmdr-index` `start_indexing`), task 34: it starts at `/`, which Windows reads as the
  process's CURRENT drive, so a `Cmdr.exe` started from `D:\…` walks `D:\`, and the watcher's branches come out as
  `/C:/Windows` (rejected: "neither a file nor a directory"). Sizes on `C:` are unreliable until the index's path space
  roots at a drive.
- The app's eject on Windows (`file_system/volume/eject/platform/windows.rs`), task 39: nothing reads as ejectable, so a
  local drive's eject refuses as `NotEjectable`; a tool run (an SMB share's unmount, "Eject and save") answers that no
  tool could start (`EjectError::Unexpected`); the holder scan is "couldn't tell"; an SMB Disconnect drops the direct
  session, as on Linux. MTP, ADB, SFTP, and WebDAV ejects are every-OS code and work.
- Code only the missing Windows arms will call is dead there, held by one crate-root
  `cfg_attr(target_os = "windows", expect(dead_code, unused_imports))` in the app's `lib.rs`: the unmount watcher's
  handling (`VolumeManager::remove_root`, `os_mount_notice::forget_unmounted_volume`; task 39), stored favorites in the
  switcher (`favorites::target::{on_disk, FavoriteTarget::discovered}`: Windows lists three home folders and the root,
  never the favorites store), and the native-crash conversion (`next_launch::to_iso8601`, `symbolicate`; task 47).
  `expect`, so the build fails once nothing's dead and the line can go.
- App tests on Windows: 31 test files are `#![cfg(unix)]` and 36 more gate their POSIX-only cells (and helpers) with
  `#[cfg(unix)]`, each for `LocalPosixVolume`, POSIX modes, inodes, `libc` errnos, or `std::os::unix`. Each un-gates as
  `LocalWindowsVolume` grows (task 35) or app paths go through the volume (#1225).

## Task list

Sizes per `docs/tooling/backlog-board.md` (XS: one agent under 200k tokens; S: one agent 200–400k; M: one agent with up
to three subagents; L: 4–13 agents). "Win" means worth it without Windows; "Windows" means Windows-only. Each task names
its prerequisites; tasks without any can start any time. Order within a phase is a suggestion.

### Phase 0: quick fixes

1. [XS] Win. **Done** ([#943](https://github.com/getcmdr/cmdr/issues/943)). Media ML off macOS is unavailable: no
   backend (`platform_backends` answers `None`), so the scheduler doesn't start, an older build's invented `media.db`
   rows go at launch ([#1001](https://github.com/getcmdr/cmdr/issues/1001)), and the image-indexing settings carry
   `needsImageIndexing`, which drops their page from the sidebar. The Linux E2E build keeps a fake engine
   (`cmdr-index/playwright-e2e`) so its specs still run. Task 25's `imageIndexing` capability took over the frontend
   half (it answers yes in that build).
2. [XS] Win. **Done**. Small fixes: `macOsLabel` prefixing non-mac OS names, the MCP instructions' "for macOS",
   `'Cascadia Mono'` before `Consolas` in `--font-mono`.

### Phase 1: make platform code honest and checkable

3. [XS] Win. **Done**. Honest crate manifests: `xattr`, `uzers`, `exacl`, `libc` under `cfg(unix)` in `cmdr-fs` and
   `cmdr-local`, the `xattr` dev-dependencies too; `notify` stays `linux` in `cmdr-index` until task 12 gives Windows a
   watcher arm. Of the nine ungated `std::os::unix` imports, two remained (`cmdr-local/src/reading.rs`,
   `cmdr-git/src/repo.rs`), now gated.
4. [XS] Win. **Done**. Honest app manifest: `url`, `semver`, `plist`, `pulldown-cmark`, `unicode-normalization`, and
   `urlencoding` sit in `[dependencies]` (`ammonia` and the viewer's positional read had already moved to
   `cmdr-viewer`), and the six unix-only tests carry `cfg(unix)`. `open_with_copies.rs` and `measure.rs` keep their
   `MetadataExt` inode reads for task 9's vocabulary: a `cfg(unix)` there would add cfgs outside a seam.
5. [S] Windows (cheap insurance). **Done**, non-blocking: `windows-lane.yml` runs clippy on the members declared for
   Windows and reports where the rest break; promoted into `ci.yml` once the `windowsPending` list in
   `member-coverage-allowlist.json` is empty (`docs/tooling/ci.md` § Windows lane).
6. [XS] Win. **Done**. Tauri config split: macOS keys into `tauri.macos.conf.json`, Linux's into
   `tauri.linux.conf.json`, explicit per-OS `bundle.targets`, a `tauri.windows.conf.json` (NSIS, `currentUser`,
   `embedBootstrapper`), and `windows/app.manifest` (`longPathAware`, per-monitor-v2 DPI) through `build.rs`.
   `macos-private-api` stays on for every target: tauri-build refuses a per-target feature, and it's a no-op since Tauri
   2.12.1 (`apps/desktop/src-tauri/DETAILS.md` § "Tauri config per platform").
7. [S] Win. **Done**. `docs/guides/platform-seams.md`; one named cfg, `cmdr_host`, from the `cmdr-platform-cfg` build
   dependency (`cmdr_apple` and `cmdr_unix_host` would only respell `target_os = "macos"` and `unix`); the app root's
   unknown-target `compile_error!` (Windows is known); `platform-cfg-placement` counting negated OS cfgs and unseamed
   ones per file (seeded at 351 and 1,569 over 261 files); `platform/`, `system_memory/`, and `diagnostics_snapshot/`
   piloted, `linux_distro.rs` folded into `platform/linux/`.
8. [S] Win. **Done**. The crates' negated OS gates went from 138 to 18: each arm names its OSes, the browser build
   (`target_family = "wasm"`) where the old arm served it too, and Windows only where the body is right there, so a
   Windows build fails at each missing arm instead of inheriting Linux's. `cmdr-fs` takes `cmdr_host` (build script,
   crate-root gate, `rt`, the fallback runtime) and asks `cfg(unix)` where `not(wasm)` meant a POSIX host. Left for
   their own tasks: the identity stubs (9) and `cmdr-http` (16). Positive `any(macos, linux)` lists stay: honest, if not
   yet in seams.

### Phase 2: crate seams

9. [S] Win. **Done**. `cmdr_fs::file_identity` (`FileId` with a 128-bit node number, `device_of`, `link_count`,
   `same_node` / `same_device` answering a three-state `Sameness`, `first_sighting` for hardlink dedup) behind a
   `platform/` seam whose Windows arm answers "unknown". Every raw `dev()`/`ino()`/`nlink()` outside tests and every
   `not(unix)` identity stub in the crates and the app goes through it; data guards take the cautious branch on
   "unknown" (`cmdr-fs/DETAILS.md` § "File identity").
10. [M] Win. **Done**. Typed OS-error classes: `cmdr_fs::os_error::OsErrorClass`, read per OS behind
    `os_error/platform/` (POSIX names, macOS's BSD extras, Win32 and Winsock codes), with `InUseByAnotherApp` for
    Windows' sharing and lock violations and no Unix errno producing it. `friendly_error`, `transport_fault`,
    `cmdr-operations`, the media fetch, and the app's stale-mount promotion match the class; the NFS, SFTP, MTP, WebDAV,
    S3, ADB, and in-memory backends build one (`VolumeError::io_of_class`) instead of a per-OS number. The code still
    travels on the wire (`rawOsError`, `errno`) unchanged. Characterized first by every-errno sweeps.
11. [S] Win. **Done**. Mount façade: `cmdr_local::mounts` (`list`, `is_mount_point`) and `user_mount_prefixes` in
    `cmdr_fs::mount_layout` (pure path knowledge, so the index asks it without depending on `cmdr-local`), each behind a
    `platform/` seam. The write engine's mount guards and the index's external-volume routing and volume-root classifier
    stop naming the OS.
12. [S] Win. **Done**. `DriveWatcher` over a `WatcherBackend` trait, per OS in `watch/watcher/platform/`: FSEvents on
    macOS, one `notify` backend for Linux and Windows (the `notify` dependency is now Linux and Windows), the fake
    journal as a third backend for macOS tests. Replay support and the branch-watch shape are backend properties.
    Windows still needs a smoke test on a real runner.
13. [S] Win (Linux gains a watchdog). **Done** for macOS and Linux: `cmdr_fs::process_memory::current_footprint` behind
    a `platform/` seam (macOS `phys_footprint`, Linux `RssAnon` + `VmSwap`), and the watchdog runs on every OS with a
    reader, with a trip's breakdown per OS (`cmdr-index` `resources/platform/`). The CPU readers sit behind
    `thread_cpu/platform/` and `process_cpu/platform/` (Linux: the POSIX thread clock, and `/proc/self/task` with
    `schedstat` runtimes), so Linux offers `cpu_diagnostics` (its own `cpuDiagnostics` capability, apart from
    `memoryDiagnostics`) and logs thread CPU in the index writer, rescan, and transfer stall lines. Windows' thread
    clock is `GetThreadTimes` (`thread_cpu/platform/windows.rs`). Left: the Windows arms that answer "unknown" today:
    `K32GetProcessMemoryInfo` for the footprint, and the `process_cpu` census (a Toolhelp thread walk with
    `GetThreadTimes` and `GetThreadDescription` per thread, `GetProcessTimes` for the total), through `windows-sys`.
14. [S] Win. **Done**. Name validity is keyed on the destination's `FilesystemKind` plus how this host's driver writes a
    Windows-format disk (`FilesystemKind::name_rules` → `NameRules`, `crates/cmdr-fs/DETAILS.md` § "Name rules"), in two
    typed tiers: a refusal only where the destination refuses or would lose data, a `PortabilityWarning` where it takes
    the name but Windows (and, for a stick's stand-ins, Linux) won't read it as typed. From a Mac, exFAT, FAT32, and a
    share warn; Linux refuses a stick's characters and trailing period (its drivers refuse or strip them), Windows
    refuses all. The SMB specs forbid no device name (MS-FSCC § 2.1.5.2, v20251121), so a share warns about `CON` too.
    Every destination refuses an empty or invisible-only name, `.` and `..` (#1229), and a name past its own limit in
    its own unit (bytes or UTF-16 units); the path limit is the path API's (the host's for a local probe, SMB2's for a
    share, none for a backend that answers for itself, #1228). Create, rename, Multi-Rename (per row and a summary), the
    MCP `multi_rename` tool, and Ask Cmdr's bulk renames (preflight and the engine, #1229) all apply it; a transfer
    warns upfront from its scan's tally (`scan_preview/name_tally.rs`) read against the destination's rules. The
    frontend's keystroke mirror is pinned to the Rust rule by `crates/cmdr-fs/src/name_rules_cases.json`. Evidence, on a
    Mac and on Linux: `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`.
15. [XS] Win. **Done**. `cmdr_fs::testing::{symlink, is_root, can_create_symlinks, make_fifo}` behind a `platform/`
    seam; the crates' 83 test `symlink` calls, seven `geteuid` root checks, and four `mkfifo` fixtures use them. The
    tests' own `cfg(unix)` gates stay until a Windows lane can say which ones compile there.
16. [XS] Win. **Done**. `cmdr-http`: `MacSystem` → `OsSystem` behind `system/platform/`; the PAC warm-up thread is the
    macOS arm's; Linux and Windows answer direct explicitly. The crate checks clean for `x86_64-pc-windows-gnu`.
17. [S] Win (moderate). **Done**. `cmdr-fs` metadata classes: every per-OS body is in `metadata/platform/` (a shared
    `unix.rs`, macOS, Linux, the browser, and a body-less Windows arm), the ordering rules stay in the class modules,
    and `cmdr-local`'s caps constant is `cmdr_fs::metadata::LOCAL_DISK_CAPS` beside them. Linux's behavior is pinned
    first (`metadata/linux_characterization_tests.rs`).

### Phase 3: app seams

18. [M] Win. **Done** for every command not tied to volumes (task 20 retires `stubs/volumes.rs`). One command each over
    a pinned seam: `permissions/`, `system_look/` (accent color, display options, Liquid Glass, text size; the startup
    arms call its `observe_changes`), `clipboard/platform/`, `commands/file_system/drag/platform/`,
    `quick_look/platform/`, and `commands/file_actions/platform/` (reveal, Get Info, open in editor). Renamed
    `show_in_finder` → `reveal_in_file_manager` and `get_macos_major_version` → `get_os_major_version` (it answers a
    major number); the frontend wrappers keep their names. Still macOS-only by design: Dock, reveal handler, CLI
    install, memory diagnostics, terminal and editor lists, Services. Left: `system_has_saved_smb_password`'s pair.
19. [S] Win. **Done**. Startup seam: `platform/startup/` with one pinned `install(app)` per OS, called once in the setup
    hook after the volume broadcast and before the MTP watcher, the first volume list, and the menu bar. It holds the
    order-free installers (macOS observers, mouse navigation, Services, volume watcher, unmount approver, `ptpcamerad`,
    File Provider warm-up, drag-out handle; Linux watcher, accent, title-bar revert); order-bound steps (the E2E
    activation policy, the FDA probe, `after_update`) stay on the spine.
20. [M] Win. **Done**. One `volumes/` defines the model (`LocationInfo`, `SmbMountInfo`) once, and `volumes/platform/`
    pins 17 fns and a const (locations, path resolution, SMB mounts, the mount table's rows and identities, live space,
    the watcher's start and stop, `NETWORK_FS_TYPE`) across a macOS arm (the old `volumes/`), a Linux arm (the old
    `volumes_linux/`), and a Windows arm (the old stub's answers). `stubs/` is gone; `commands::volumes`, its IPC group,
    and `mount_registration` compile everywhere, and `listing_host/mounts.rs`, `space_poller`, `index_provider`,
    `volume_listing`, and the SMB callers ask `crate::volumes` with no `cfg`. Eject's `is_mount_point` reads go through
    `cmdr_local::mounts`. `mount_table` and `linux_mounts` stay public: the arms read their full rows (fs type, source,
    flags, owner), which the façade doesn't carry, and `cmdr-operations` reads `linux_mounts` too.
21. [M] Win. **Done**. `adb`, `network`, `managed_policy`, `saved_servers`, `server_move`, `server_volumes`, and the
    `nfs`/`s3`/`servers`/`sftp`/`volume_sign_in`/`webdav` commands are on every OS (their IPC groups are `cfg(all())`),
    and `smb2` and `mdns-sd` sit in `[dependencies]` (both build for `x86_64-pc-windows-gnu`). Still macOS+Linux: MTP,
    eject, volumes, `commands::network`, and `network/`'s OS-mount files, whose per-OS gates now mark exactly where
    Windows lacks an arm (tasks 20 and 22).
22. [M] Win. **Done**. `network/os_mount/` holds `mount_share`, the NetFS-wedge ledger, the mount watch, and one
    `MountResult`/`MountError` (three hand-mirrored copies before); `os_mount/platform/` pins each arm's mount, unmount,
    share-list CLI (smbutil, smbclient, none), and sign-in-helper probe, with an explicit Windows arm. `keychain.rs` and
    `os_mount_notice.rs` stay in `network/`: they have no OS code. `commands::network` and MTP (`mtp-rs` builds for
    Windows) compile everywhere, and `stubs/network.rs` and `stubs/mtp.rs` are gone. Eject stays macOS+Linux (task 20).
23. [S] Win. **Done** except the size measurer's inode dedup, which waits for task 9's vocabulary (with
    `open_with_copies.rs`'s inode check). Seams: `icons/platform/` (path and id lookups, the per-path threads, the
    autorelease pool, tag tinting; `macos_workspace.rs` moved under it), `intl/platform/` (the UI locale's two answers),
    `cmdr-viewer`'s `platform/` (`positioned_read`: `pread`, `seek_read` on Windows, a refusal in the browser), and
    `crash_reporter/platform/` (native-crash hooks, image base, thread count; the next-launch reader keeps its cfgs).

### Phase 4: frontend seams

24. [S] Win. **Done**. `#lib/platform` (`apps/desktop/src/lib/platform/CLAUDE.md`): a plugin init script in
    `tauri_builder.rs` defines `window.__CMDR_PLATFORM__` in every webview, `isMacOS()` lives there and reads it (no UA
    sniff; `'linux'` without a backend, the demo answers for the visitor), `data-platform` on `<html>`,
    `--titlebar-height: 0` off macOS, and `withPlatform()` drives every test that used to mock `isMacOS` or stub the UA.
25. [S] Win. **Done**. Capabilities instead of OS checks: `PlatformCapabilities` (14 flags, one answer per arm of the
    `platform/` seam, typed into `bindings.ts`) arrives beside the platform in the same init script; 46 of the 72
    `isMacOS()` calls (in 28 files) now ask `hasCapability()`, settings' `macOSOnly` and `needsImageIndexing` became one
    `needsCapability`, and CSS keys the title bar on `data-overlay-title-bar`. Left as platform questions: 11 (key
    glyphs, the macOS updater flow (task 33 removed it), the old-macOS notice); for 26: 5 keymap sites; for 27: 10 copy
    variants.
26. [S] Win. **Done**. Per-platform default keymap ([#1102](https://github.com/getcmdr/cmdr/issues/1102), with
    [#1110](https://github.com/getcmdr/cmdr/issues/1110) and [#1114](https://github.com/getcmdr/cmdr/issues/1114)):
    every command's `shortcuts` is three lists, one per OS in its own spelling, written with `sameOnAll` / `cmdOrCtrl` /
    `macOnly` / `concatShortcuts` or a literal, and nothing converts a combo at read time (`lib/commands/DETAILS.md` §
    "Default keys per platform"). Windows and Linux follow Total Commander first, then their file managers
    (`docs/notes/windows-port/keymap-decisions.md`, the table that shipped); macOS kept every key and gained `⌥←` / `⌥→`
    for back and forward. Three reviewed keymap snapshots, conflict, vocabulary, reachability, and invariants tests on
    all three OSes (no `Ctrl+Alt+<letter>` or `Super` default off macOS; the global go-to-latest hotkey's
    `Ctrl+Alt+Super+J` is the one decided exception), and a parity test tying the menu bar's accelerators to the
    registry (`cmdr-menu` gained `Platform::Windows`). Also: the AltGr guard and `Ctrl+<` by key position; the native
    text keys per OS; Ctrl+click toggles off macOS; the Linux bar's mnemonics masked while a dialog is up; the search
    dialog's and the viewer's keys as fixed registry commands (macOS recent searches moved to `⌘Y`); a user's own
    binding beats a default; Quit and `F11` full screen off macOS, with Hide, Hide others, and Show all behind
    `appMenu`; a per-OS reserved-keys table for Settings' warning; `Win` as Windows's name for `Super`; and every hint
    and message naming the live key, or a keyless variant. What needs a real machine is on task 55's list. The mechanism
    and its rejected alternatives: `apps/desktop/src/lib/commands/DETAILS.md` § "Default keys per platform". Needs 24.
    - **macOS hand checks David owes**: with the search dialog open, `⌘H` hides Cmdr and `⌘Y` shows recent searches;
      Toggle full screen from the palette enters AppKit's native full screen (same Space, the green button's state, and
      Escape and the View menu item both leave it).
27. [M] Win (Linux needs it). **Done**. Platform-variant copy ([#1103](https://github.com/getcmdr/cmdr/issues/1103)): a
    value that reads differently per OS is one whole-value `{platform, select, macos {…} windows {…} other {…}}` block,
    which `t()`, `getMessage()`, the native menu table, and the old-WebKit guard all resolve to the running OS's branch,
    and every i18n check reads branch by branch (`apps/desktop/src/lib/intl/DETAILS.md` § "Platform variants"). The
    words are `docs/style-guide.md` § "Platform vocabulary"; the 14 `.mac`/`.other` pairs are one key each,
    `cmdr/no-platform-copy-branch` keeps `isMacOS()` off copy, and macOS-only surfaces are capability-gated instead of
    reworded. The resolved English, one row per key, is `docs/notes/windows-port/copy-final.json` (299 rows applied in
    `en`, the 13 full locales, and the three overlays; macOS text unmoved in every language apart from the two accepted
    English changes). David reviewed the English (2026-11-12). Left: the rows in
    `docs/notes/windows-port/copy-decisions.md` § Open rows, each naming what it waits on (#1126, task 59); the rows
    that waited on #1102 shipped with task 26.
28. [L] Win. **Done**. Task 27's per-OS branches are translated in the 13 full locales and the `es-419` forks:
    `i18n-stale` is clean in every catalog (so releases aren't blocked on it), no `windows` or `other` branch whose
    English differs from its `macos` one is still English (two `Windows` product names and German and Swedish `System`
    aside), and every full locale's `terms.json` rules the Windows concepts in `docs/i18n/concepts.json` (`es-419` has
    no `terms.json` by design and inherits `es`; its `style.md` rules the one fork question, `Papelera de reciclaje`;
    verified 2026-11-18, a branch-by-branch scan against `en`). A per-OS string that changes from here on is edited in
    `en` directly (`apps/desktop/src/lib/intl/DETAILS.md` § "Changing a per-OS string"), and the translator loop is
    `docs/guides/i18n-translation.md` § New feature. The macOS text is guarded by a written rule, no test: a `macos`
    branch's translation stays unless it's wrong, and a fix there is an ordinary translation edit with its reason in the
    commit message (`docs/i18n/translator-instructions.md`, Per-OS values). David approved the macOS texts that moved on
    purpose during the pass on 2026-11-18; `docs/notes/windows-port/macos-copy-accepted-review.md` is the record. Left
    for a translator pass: `fr` and `vi` `terms.json` lack a `powershell` ruling (both catalogs write `PowerShell`).
29. [S] Windows, lint half is a win. **Done**. `lib/path/grammar.ts` holds the grammar (drive and UNC roots, a shared
    case table with Rust in `cmdr-fs`'s `path_grammar_cases.json`), `LocationInfo.case_sensitive` comes from the
    `volumes/` seam (macOS `false`, Linux `true`, Windows `false`; no frontend site consults it yet,
    `lib/path/DETAILS.md` § "Case sensitivity is the volume's"), `shownPath` is the one `\` formatter behind
    `displayPath`, `t()` params, Copy path, and the breadcrumb, Go to path sends `fromUserInput`, and
    `cmdr/no-raw-path-arithmetic` steers path-named `split('/')` / `lastIndexOf('/')` / `+ '/'` to `lib/path` (57 hits
    on 55 lines: 28 lines fixed, 27 opted out with a reason).

### Phase 5: release and services for N platforms

30. [M] Win (Linux release falls out). **Done**. `scripts/release-assets/manifest/manifest.go` holds one row per
    published file (os, arch, kind, file, updater key, optional, Rust targets); `guard` turns it into step outputs
    (`release-assets.sh plan`) that `sbom`, `vet-update-archives`, `publish`, and `attest` iterate, `publish` writes
    `latest.json` and picks the checksummed files from it, and `release-finish`, `release-mirror`, and
    `release-evidence` import it. `latest.json` gains a `downloads` list (os, arch, kind, file, size) that the website
    reads (`apps/website/src/lib/release-downloads.ts`); `dmgSizes` stays through the release after the first with
    `downloads`. A Windows release is rows (`nsis` with `windows-x86_64`, `msix`, a Rust SBOM) plus `-vetted windows`
    and an attest step, which the plan enforces. A dry run of those jobs against v0.61.0 as published
    (`scripts/release-assets/pipeline_test.go`) holds every output to golden files, `latest.json` to v0.61.0's bytes
    with `downloads` appended (`scripts/release-assets/DETAILS.md`).
31. [M] Win. Downloads and analytics for N platforms: `/download/:version/:os/:arch` (old route stays as the macOS
    alias), artifact-aware UA classification in the api-server and dashboard funnel, an explicit `os` field through
    heartbeat, feedback, and reports (a Cloudflare D1 migration), the dashboard split by OS. Needs 30.
32. [S] Win. Website download page for three OSes (the Linux plan's Part 3, built once). David reviews copy and design.
    Needs 30.
33. [S] Win (Linux too). Updater trust parity off macOS: the Rust-side check, advisory list, and managed-install logic
    run everywhere; the plugin only hands off the installer; the UI handles "the app quits to install". **Done**.
    `mod updater` builds on every OS and the frontend runs one flow (no OS branch, no `@tauri-apps/plugin-updater`, no
    `updater:` capability). `install_kind.rs` decides what may update (a macOS bundle, an NSIS install, a running
    AppImage; a `.deb`/`.rpm` or an MSI never checks), and the `updater/platform/` seam holds the per-OS last step.
    Windows and Linux verify minisign against both keys and refuse a signature whose `version:` isn't the offered one
    (`bind_signed_version`, the rollback guard macOS gets from `Info.plist`); `install_update` stages the installer
    after a fresh policy read and reads the release's signed advisory list (`latest.json`'s optional top-level
    `securityAdvisories: { url, signature }`); the restart toast's Restart quits through the quit gate and hands the
    staged installer to the plugin (`restore_update` + `install`, re-verified under the same key), and its second line
    says "Cmdr closes to install it, then opens again." off macOS. What it needs from the release side is written into
    the manifest (`File.SignsVersion()`, the key rule in `Validate`, `scripts/release-assets/DETAILS.md` § Update
    payloads off macOS) and left for task 52 to wire. Not yet verified on a real Windows or Linux machine: the plugin's
    install arms are read from `tauri-plugin-updater` 2.14.1's source, and Rust tests cover everything up to them
    (`updater/DETAILS.md` § Windows and Linux).

### Phase 6: Windows implementation

Each lands behind a seam from Phases 1–5, verified on the Windows lane (task 5).

34. [M] Windows. Multi-root path model in the app: drives and UNC shares as volumes, the `"root"` volume id and `/`
    assumptions (`file_system/mod.rs:126`, `priority/roots.rs:80`, `tools/resources/volumes.rs:469`). Needs D1, 20.
    **Platform-independent part done**: `cmdr_fs::volume::path_grammar` (`AppPath`/`AppRoot`, `PathFlavor::HOST` behind
    a `platform/` seam, `to_native`/`from_native` for `\\?\C:\…` and `\\?\UNC\…`, pure and tested on Linux) runs the
    frontend's case table, and `volumes::local_volume_id_for_path` is the seam the missing-volume defaults in
    `commands/rename.rs` and `commands/file_system/path_checks.rs` now ask (every arm answers `"root"` today). Left: the
    drive and UNC volume id scheme (open), registering a volume per drive (with 35 and 39), and the remaining
    single-root sites, most of which only change once those volumes exist (the root `LocalPosixVolume` at `/`, the
    index's `ROOT_VOLUME_ID`, the boot-volume priority roots, the MCP volumes resource's non-macOS row). The app's root
    volume now comes from `volumes::root_volume` (a `volumes/platform/` pin beside `local_volume_on_mount` and
    `is_local_volume`, so `file_system/mod.rs` and `smb_mount_gate` name no concrete local volume); Windows' arm is the
    stub in § "Windows build state". Open: whether "the system drive under `root`" survives the id scheme. Decided
    (2026-11-18): consumers name the host's local volume `cmdr_local::LocalVolume`, and app paths become a typed
    `AppPath` the `Volume` trait takes (#1225); both are milestones of `docs/specs/app-path-plan.md`.
35. [L] Windows. `LocalWindowsVolume` in `cmdr-local`, a sibling of `LocalPosixVolume`: `FindFirstFileExW` reader,
    space, no-replace and atomic-replace rename (❗ `std::fs::rename` replaces on Windows), streams, file vs dir
    symlinks, junctions, reparse tags, ADS-backed xattrs, write-access probe, metadata caps, the conformance suite
    green. Needs 9, 10, 34. **Started** (2026-11-18): `LocalWindowsVolume` is the sibling in `cmdr-local/src/platform/`
    (the POSIX volume is `cfg(unix)` beside it), and `cmdr-local` builds and passes clippy and nextest on the Windows
    builder. Real: app paths in and out through `\\?\` (`local_windows/paths.rs`), listing and stat through `std::fs`
    (the reader's `reading/platform/windows.rs` arm, hidden attribute included), the no-replace rename (`MoveFileExW`,
    no flags) and the atomic replacing one (`std::fs::rename`, `supports_atomic_replace_rename`), create file and folder
    (no-clobber), delete (a junction or directory symlink as the link), the conflict scan, and space
    (`GetDiskFreeSpaceExW`), with hostile cells on real NTFS (a taken name, a file another program holds open, a
    read-only file, a path past `MAX_PATH`, a junction). Left, in data-safety order: streams (read, write, continue,
    resume, durability via `FlushFileBuffers`), metadata (dates, attributes, caps, `apply_metadata`), symlink creation
    (file vs dir) and reparse tags, ADS-backed xattrs, the write-access probe, the copy scan, the mount table and
    filesystem type, the `FindFirstFileExW` lister, and registering a volume per drive (with 34). The ignored
    conformance cells in `local_windows_conformance_test.rs` are the checklist. App paths in and out of both local
    volumes (one crossing per OS through `AppPath::to_native`): `docs/specs/app-path-plan.md`.
36. [M] Windows. `cmdr-operations` arms: `CopyFileExW` strategy (with ReFS/Dev Drive block clone), Recycle Bin via
    `trash` with identity, read-only as "locked" in `clear_blockers`, sharing violations as a typed refusal. Needs 35.
    Plus the honest `Unknown`-identity rule (`cmdr_fs::file_identity` module docs): a move onto a taken name with
    unknown identity goes through the conflict flow instead of counting as already in place (`move_op/mod.rs` and
    `conflict_preflight.rs`), a copy asks instead of duplicating (`copy/mod.rs`, `free_space.rs`, `scan_dry_run.rs`,
    `routing.rs`), the per-file guard in `copy/single_item.rs` refuses an overwrite instead of skipping, and a compress
    onto an existing archive asks instead of refusing (`archive_edit/fresh_plan.rs`). Needs a typed refusal with its own
    copy ("can't tell whether these are the same file"); the `unknown_identity_today_*` tests flip with it. Plus the
    typed-OS-error gaps task 10 left for the Windows arms (35 and 36 between them):
    - A backend-built error carries its class only through `host_code`, and Windows has no Win32 code for `Busy`,
      `WouldBlock`, `LinkLoop`, `Protocol`, `BadMessage`, `FileTooLarge`, `TooManyLinks`, `Remote`, or
      `DeviceNotConfigured`, so `io_of_class` yields `raw_os_error: None` there: WebDAV's `423 Locked` and NFS's
      `JUKEBOX` lose their transient class and read as serious. And `Io`'s host code is `ERROR_CRC` (23), so a generic
      NFS fault would print a CRC error in the technical detail. The honest fix: a backend-built `VolumeError` carries
      its `OsErrorClass` directly (wire format unchanged), and only a real OS error carries a code.
    - A product bug the same fix closes (found in the Windows builder's test logs): on Windows, the NFS and ADB backends
      read the server's or the phone's Linux-numbered errno through Windows error codes, so their errors get the wrong
      class there.
    - Deciders still on raw errnos, each a local-disk or OS-call site: `cmdr-local` `local_posix.rs` (the no-replace
      rename's fallback, ~131; the write-access probe, ~947), `local_posix/links.rs` (~33), `reading/bulk.rs` (~164);
      `cmdr-fs` `durable.rs` (~18); the app's `network/smb_connect_failure.rs` (~219, its log detail); and `cmdr-smb`
      `volume/mapping.rs` (`local_errno`).

    **Started** (2026-11-18): `cmdr-operations` builds and passes clippy on the Windows builder. Real: the write probe
    (`local_disk/platform/windows.rs`, tested on NTFS), free space, the folder volume, and file and directory syncs that
    open the way `FlushFileBuffers` needs (`cmdr_fs::durable::{open_to_sync, open_dir_to_sync}`). A copy takes the
    chunked loop. Left, in data-safety order: the local engine taking app paths at `rename_local_exclusive` (#1225,
    planned in `docs/specs/app-path-plan.md`: today every local copy and move refuses at its landing), a Windows file id
    and mount table so a folder delete or move isn't refused as a mount (`file_identity`, task 35's mount arm), the
    `Unknown`-identity rule above, the `CopyFileExW` arm, the Recycle Bin, read-only as Locked, and sharing violations
    as a typed refusal. Stubs: § "Windows build state".

37. [S] Windows. Cloud Files placeholders (OneDrive and friends: sync badges, cloud trash) and Mark of the Web on
    extract (`cmdr-operations` + `cmdr-archive`). Needs 36.
38. [S] Windows. Identity and system bundle: secrets via `windows-native-keyring-store` (❗ a blocker: off macOS and
    Linux secrets fall back to a plaintext file today), device id (`MachineGuid`, hashed like the Linux
    `/etc/machine-id` arm in `licensing/device_id.rs`), OS version, memory, the instance-lock `MessageBoxW`, managed
    policy from the registry. Needs 18. **Done** (2026-11-18), each behind its seam:
    - Device id: `licensing/device_id/platform/windows.rs` reads `MachineGuid` from the 64-bit registry view and hashes
      it like Linux's machine id.
    - Secrets: Credential Manager (`secrets/platform/windows/`, `windows-native-keyring-store` 1.1 on `keyring-core`),
      no file fallback on any OS. Details and what happens to an old dev build's `secrets.json`: `secrets/DETAILS.md` §
      Windows.
    - OS version: `RtlGetVersion` plus the registry's names (`platform/windows/version.rs`), for example
      `Windows 10 Pro 22H2 (build 19045.6466)` on the builder. Memory: `GlobalMemoryStatusEx` + `GetPerformanceInfo`
      (`system_memory/platform/windows.rs`).
    - The "already running" alert: `MessageBoxW` on its own thread (`instance_lock/platform/windows.rs`).
    - Managed policy: `SOFTWARE\Policies\Cmdr` in HKLM then HKCU, with a live registry watch
      (`managed_policy/DETAILS.md` § The Windows registry keys).
    - On the builder (Windows 10 Pro 22H2, build 19045.6466, `52047a9377`, 2026-11-18):
      `cargo clippy -p cmdr --all-targets -D warnings` clean (297 s warm); `cargo nextest run -p cmdr --lib` over
      `secrets`, `platform`, `instance_lock`, `managed_policy`, and `system_memory`, 145 of 151 pass, among them the
      message box shown and dismissed, the registry policy read and its watch on scratch keys, the
      kernel-versus-registry build check, and the memory counters. The four Credential Manager round trips fail over SSH
      with `ERROR_NO_SUCH_LOGON_SESSION` (key-based OpenSSH logons get no credential vault) and pass, with no `CmdrTest`
      credential left behind, when the same test binary runs as SYSTEM from a scheduled task. The other two failures are
      the pid cells below.
    - Left for later tasks: an ADMX template and the Windows half of `/trust` (the key path is a public contract once
      Windows ships), and the instance lock's holder pid, which reads as unknown on Windows (the two
      `instance_lock::tests` pid cells fail there, as before this task: `LockFileEx` locks the bytes the pid sits in).
39. [M] Windows. Drives: drive letters and labels, the device-arrival watcher (`CM_Register_Notification`), mapped and
    UNC drives, eject. Needs 20, 34. **Eject seam in place** (2026-11-18): eject is every-OS code, and what differs sits
    in `file_system/volume/eject/platform/` (the tool, ejectability, whether an SMB disconnect unmounts). The Windows
    arm is the stub in § "Windows build state"; the real one is `CM_Request_Device_EjectW` for a removable drive, the
    Restart Manager for holders, and `WNetCancelConnection2W` once 45 makes OS SMB mounts.
40. [S] Windows. System look: accent color, text scale, high contrast and transparency, UI language and formats, all
    live-updating (`UISettings`, `WM_SETTINGCHANGE`). Light/dark already arrives through WebView2. Needs 19, 23.
41. [S] Windows. File icons via `SHGetFileInfoW` / `IShellItemImageFactory`. Needs 23.
42. [S] Windows. Clipboard (`CF_HDROP`, `Preferred DropEffect` for cut) and drag-out of plain paths (OLE). Needs 18.
43. [M] Windows. Drag-out of virtual files (MTP, SMB, archives) via `CFSTR_FILEDESCRIPTORW` / `FILECONTENTS`. Needs 42.
44. [S] Windows. Open with, default app, text editors, terminal (`SHAssocEnumHandlers`, `wt -d`). Needs 18.
45. [S] Windows. SMB OS mounts via `WNetAddConnection2W` and Credential Manager SMB credentials. Needs 22.
46. [S] Windows. Small backend arms: `cmdr-nfs` identity (`GetComputerNameExW`, configurable uid/gid), `cmdr-adb`
    `adb.exe` discovery, special folders (`SHGetKnownFolderPath`), thread QoS (EcoQoS, not background mode), WinHTTP
    system proxy. Needs 8, 16. **NFS identity done** (2026-11-18): `cmdr-nfs`' `dial/platform/windows.rs` asks as
    `nobody` with no groups, the machine name from `GetComputerNameExW(ComputerNameDnsHostname)`, and the app's
    `this_mac_identity` reads it through `NfsIdentity::of_this_process`; a place sets its own uid and gid in the add
    form, as on every OS.
47. [S] Windows. Crash minidumps (`crash-handler` + `minidumper`) and Windows-shaped crash cards in the api-server.
    Needs 23.
48. [XS] Windows. **Done**. WebView2 specifics: every `cmdr-media` URL comes from `mediaUrl` via `convertFileSrc`
    (`http://cmdr-media.localhost/<token>` there, pinned by `media-view.test.ts`; the handler reads only the path), and
    `tauri.windows.conf.json` restates the CSP with that origin, held to the shared one by `tauri_config.rs`.
    `instance-id.ts` knows Roaming (`%APPDATA%`) for data and Local (`%LOCALAPPDATA%`) for the cache, in lockstep with
    the backend through the case table `os_app_dirs_cases.json` (`docs/tooling/instance-isolation.md` § "Data dir").
49. [S] Windows (taskbar progress is a win: Dock progress too). Shell integration: an "Open in Cmdr" Explorer verb,
    taskbar progress for long operations (`Window::set_progress_bar`), jump lists of recent folders. **Progress part
    done**: `shell_progress/` folds the write queue into one bar (Dock tile, taskbar button, Linux launcher entry), its
    Windows arm unit-tested on every host; real-machine checks are in task 55. Left: the Explorer verb and jump lists,
    both needing a Windows install to verify.
50. [S] Windows, later (D3 step 3). `cmdr-update-trust` Authenticode check (`WinVerifyTrust` plus a pinned signer) for
    the signed direct download's updater artifact. Needs 33, 51.
51. [XS] Windows, David, later (D3 step 3). Azure Artifact Signing account and org validation for Rymdskottkärra AB,
    once Windows sales cover it; an agent writes the click-path. Then `signCommand` with OIDC `azure/login` in task 52's
    job and an Authenticode verify in `vet-update-archives`.
52. [M] Windows. Windows release build: a `windows-latest` job that builds once and packages twice, the MSIX (via
    Microsoft's `winapp` CLI) and the unsigned NSIS installer, plus the minisign `.sig` for the NSIS updater artifact in
    both signing modes and the `windows-x86_64` manifest row. Task 33's updater refuses any off-macOS signature without
    a `version:` field, so every row whose `File.SignsVersion()` is true is signed with
    `tauri signer sign --app-version <v>`: `release-finish` already does (`signerArgs`) and `latest-json` already
    refuses a `.sig` without it; the CI `sign` job (`ci` mode) still needs the flag. `Validate` already holds the key to
    one the app reads (`windows-x86_64[-nsis]`, `linux-x86_64[-appimage]`). Also publish the advisory list as a release
    asset with its own `.sig` and write the optional top-level `securityAdvisories: { url, signature }` into
    `latest.json` (`latest.go`); without it Windows and Linux show no security notice (the update still installs). Once
    it's written, drop `securityAdvisories` from `NOT_WRITTEN_YET` in `crates/cmdr-update-manifest/src/tests.rs`
    (`scripts/release-assets/DETAILS.md` § Update payloads off macOS). Needs 6, 30.
53. [S] Windows. Release vetting and distribution: `deny.toml` triple, third-party notices and SBOM, checksums for both
    packages, and a winget manifest for the NSIS installer. Needs 52.
54. [S] Windows. E2E smoke on `windows-latest`: `tauri-plugin-playwright` over TCP, three-way helpers, a Windows fixture
    root, a small spec subset. Needs the app to run on Windows (34–36).
55. [M] Windows. Windows QA pass on a real machine: an agent drives the checklist and fixes what it finds; David QAs
    look, feel, and timing-sensitive UI.
    - **Keyboard (from task 26; unit tests cover the data, these need the real OS)**:
      - What WebView2 reports for AltGr (`AltGraph`, `ctrlKey`, `altKey`) on German and Hungarian layouts, for
        LeftCtrl+LeftAlt+E, and for right Alt with empty modifiers (Multi-rename's `Alt+<letter>` would then need the
        left Alt).
      - The browser keys WebView2 keeps until #1115 lands: `Shift+F5`, `Ctrl+F5`, `F5`, `F3`, `F7`, `Ctrl+R`, `Ctrl+J`,
        `Ctrl+G`, `Ctrl+Shift+G`, `Ctrl+Shift+C`, `Ctrl+0` / `Ctrl++` / `Ctrl+-`, and `Alt+←/→` reach Cmdr.
      - `F6` and `Shift+F6` (Move, Rename), `Ctrl+M`, and `Ctrl+Shift+M` in windowed hosting: if `F6` moves focus out,
        #1115 needs a per-key `IsBrowserAcceleratorKeyEnabled(FALSE)` for it. Also `Ctrl+PageUp/PageDown` and
        `Ctrl+Shift+PageUp/PageDown` (Chromium's tab keys).
      - `F11` toggles full screen (and WebView2's own `F11` stays off), and Escape leaves it.
      - `Alt+F4` closes the main window through the quit gate (work in flight holds it and asks), and File > Exit Cmdr
        does the same.
      - `Ctrl+Alt+→/←` (Next and Previous tab) on Intel graphics drivers, whose legacy control panel rotates the screen.
      - The bare `Ctrl+Shift` and `Alt+Shift` layout switches during `Ctrl+Shift+<key>` and `Alt+Shift+←/→`, on a
        machine with two layouts.
      - The global go-to-latest hotkey `Ctrl+Alt+Win+J` registers and isn't one of Windows' own; try `Win+Alt+J` as a
        simpler default there.
      - The menu bar (task 63; if D5's in-page bar holds after the spike, no Win32 accelerator is in play): a lone Alt
        tap and F10 reach the page and focus the bar, without Windows' menu loop stalling IPC (`SC_KEYMENU`; a click on
        the panes still answers within a second); Alt+Space still opens the system menu; with the search dialog or
        Multi-rename open, `Alt+F` / `Alt+S` / `Alt+V` and `Alt+F` / `Alt+E` / `Alt+G` stay the dialog's and the bar
        doesn't open; with "Show menu bar" off, Alt and F10 show it until focus leaves.
      - The download toast's hotkey animation: its PC key caps read `Ctrl`, `Fn`, `Win`, and `Alt` where a Windows
        laptop has them.
      - With a text field focused, a menu CLICK on a file-moving command (File > Delete, Edit > Move here) runs, and the
        same command's key still doesn't (`routes/(main)/text-field-gate.ts`): task 63's in-page bar knows a click from
        a key (its step 6).
      - The browser demo for a Windows or Linux visitor: the search dialog's new query keeps `Ctrl+N`, which Chromium
        keeps from the page, because a fixed-key command can't be seeded (`lib/demo/browser-shortcuts.ts`). Task 63
        gives the demo the visitor's own bar (its step 8), which picks the demo-only key.
    - **Taskbar progress (from task 49; `apps/desktop/src-tauri/src/shell_progress/DETAILS.md` has the rules and a QA
      recipe)**:
      - A large copy shows a green bar on the main window's taskbar button, filling in whole percents, and the scan
        before it shows the marquee.
      - Pause (and a conflict dialog left open) turns it yellow; a failure left in the queue turns it red, full when
        nothing else runs; cancel and completion clear it within half a second.
      - With the main window hidden or minimized to the tray, nothing throws or logs a warning (`shell_progress`
        target), and the bar comes back on the button once the window shows.
      - After quitting with a failure still shown, no button keeps a stale bar (pinned taskbar icon included).
      - On Linux (whenever a Linux desktop is at hand, Ubuntu Dock or KDE): the bar appears at all, which needs Tauri's
        `<package name>.desktop` to match the installed `.desktop` file's name.
    - **WebView2 and data dirs (from task 48; unit tests cover the URL and dir rules, these need the real OS)**:
      - An image, a PDF, a Markdown document's local pictures, image search's thumbnails, and the bulk-rename review's
        previews all load over `http://cmdr-media.localhost/<token>`, with no CSP violation in the devtools console, and
        a PDF range request (scrolling a big PDF) answers `206`.
      - The data dir lands in `%APPDATA%\com.veszelovszki.cmdr` and the drive index in
        `%LOCALAPPDATA%\com.veszelovszki.cmdr\drive-index`; a `pnpm dev` build's in its `-dev` twins. Run
        `config_os_dirs_test` there too: it pins the dir model to the known folders `dirs` reads.
      - (Task 57 owns where MSIX redirects them.)
    - **First launch (verified on Windows 10 22H2 build 19045, mami, the debug `pnpm tauri build --debug` binary run in
      the signed-in console session by a one-off scheduled task, MCP over an SSH tunnel, 2026-11-18;
      `docs/notes/windows-port/build-progress.md` has the run)**:
      - [x] The main window opens with the menu bar, both panes, and the function-key bar, and "What's new" shows on a
            first launch. No panic, nothing in the Application event log.
      - [x] The first run opens `~` and `~/Downloads` (`C:/Users/<account>`), listed through `LocalWindowsVolume`.
      - [x] Up from `~` twice lands on the system drive's root and lists `C:\`; Enter on `Windows` lists `C:\Windows`
            (107 entries).
      - [x] Alive and idle (no CPU over 10 s) more than two minutes after the last action.
      - [x] The data dir is `%APPDATA%\com.veszelovszki.cmdr` and `drive-index`, `logs`, and `EBWebView` are under
            `%LOCALAPPDATA%\com.veszelovszki.cmdr`.
      - [x] Real arrow and Enter key presses (`SendKeys`) move the cursor and enter a folder; Go to `C:/` and the four
            favorites resolve to the root volume.
      - [ ] The breadcrumb at the root reads `C:\`: it shows nothing past the volume at `/` and `\Windows` one level
            down, where `~/Downloads` keeps its `/`.
      - [ ] A digit picks a favorite in the `Ctrl+D` menu (a `SendKeys` `2` left it open with nothing picked).

56. [S] Windows. MTP via `mtp-rs`'s WPD backend (D2): Windows device arrival and removal for phones, the 5–6-minute
    driver stall shown as a "the phone is busy" state with a timeout (a dropped future is safe, later calls queue behind
    it, and a delete can still finish after we gave up), and a QA pass on a real phone. Needs 5.
57. [S] Windows, partly David. Microsoft Store channel (D3 step 1): the company developer account and listing (David
    opens the account and reviews the listing copy), Store submission automated from the release pipeline, the Store
    build's own updater off, and a check of where MSIX redirects Cmdr's data and cache folders so the data dir, the
    drive index, and "Delete all Cmdr data" all find them. Needs 52.
58. [XS] Windows. The website's "Run anyway" download (D3 step 2): the unsigned NSIS installer on the download page with
    a short explainer of SmartScreen's "More info → Run anyway" step, next to the Store link. David reviews the copy.
    Needs 32, 52.

### Phase 7: native feel

Phases 1–6 make Cmdr run correctly on Windows. This phase makes it feel like a Windows app, which is the goal David set
on 2026-11-11: not "a macOS app ported to Windows" and not a "cross-platform app" (a spork: good at everything, great at
nothing), but native controls, native conventions, native words and keys, and every macOS goodie in its Windows form
(`docs/design-principles.md` § "Platform-native, not generic"). Tasks 26 (keymap) and 27 (copy) serve the same goal.
None of it blocks a first Windows alpha; all of it is expected before Windows is called done.

Linux rides along as a decision, not as leftover: each task below states what Linux does for that aspect (follows macOS,
follows Windows, splits by desktop, or its own way), so the Linux version can grow into it without rework. The Linux
choices, checked against the GNOME and KDE guidelines in task 59 (2026-11-18; sources, dates, and what stayed
unverified: `docs/notes/windows-port/platform-conventions.md`):

- **Shortcuts**: Windows (Ctrl-based), minus what GNOME and KDE reserve (task 26's per-desktop survey). GNOME's HIG also
  keeps Alt+letter for access keys, which task 61 gives every Linux desktop.
- **Window chrome**: Windows (native decorations, already decided).
- **Paths, Trash, permissions**: macOS (POSIX).
- **Dialog button order**: splits by desktop, GNOME like macOS (its HIG) and KDE like Windows (Qt's `KdeLayout`; the KDE
  HIG is silent). Other desktops follow GNOME, since most are GTK-based.
- **Control styling**: its own, one neutral theme following the system accent and dark mode.
- **Words**: its own, desktop-specific where GNOME and KDE differ.
- **Screen reader**: its own (Orca, which the KDE HIG names), on the same ARIA groundwork.
- **Menu model**: splits by desktop (D5, task 63): KDE and other desktops follow Windows (an in-window bar), GNOME gets
  its HIG's primary menu.

59. [S] Win. **Done**. Platform conventions as data: `PlatformConventions` (dialog button order, which button Enter
    presses, the cancel keys, access keys, the menu model, the screen readers to expect, and `linuxDesktop`: GNOME, KDE,
    or other from `XDG_CURRENT_DESKTOP`), one typed enum per question in `bindings.ts`, arrives beside the capabilities
    in the same init script. One cross-target table answers all five desktops and each OS arm picks its row, so a Rust
    test pins the frontend's `conventions-on.json` (the demo's and the test helpers' copy) on any machine. Primitives
    ask `convention('dialogButtonOrder')`, ❌ never the platform (`apps/desktop/src/lib/platform/DETAILS.md` §
    Conventions); `ModalDialog` asks for the button order and access keys (task 61). The guidelines confirmed the table
    above, with KDE's order from Qt since its HIG is silent, and corrected task 64: Windows' leading screen reader is
    JAWS. The menu model carries today's bar until task 63. Evidence: `docs/notes/windows-port/platform-conventions.md`.
    Needs 25.
60. [M] Windows. A Windows theme for the `lib/ui` primitives, following Fluent 2: buttons, dropdowns, checkboxes,
    switches, text fields, scrollbars, focus visuals, tooltips, and context menus, as tokens keyed on the platform so
    components stay one implementation. David reviews the design. Linux: its own neutral theme, same token set.
    Needs 59.
61. [S] Windows. **Done**. Dialog conventions, read from task 59's conventions inside `ModalDialog`, ❌ never the OS
    (`apps/desktop/src/lib/ui/DETAILS.md` § Platform conventions, with the per-dialog Return table). Button order: the
    footer is written in macOS order and Windows and KDE show it reversed, with Tab following through the focus trap's
    reading order; alignment stays trailing everywhere. Return (David's decision, 2026-11-18): a dialog marks its
    default button (`Button`'s `isDefault`), and Return presses it unless the dialog claims the key; Windows and Linux
    keep a focused button's own press, while on the Mac Return on a footer button other than the default does nothing
    (it never falls through to the default) and ⌘. cancels like Escape. The dialogs that gave their safe answer focus so
    a reflex Enter was safe (quit, tool question, rollback, data wipe) follow Apple's HIG: no default button, the
    destructive one styled as such, the safe answer focused only off the Mac. Access keys on Windows and every Linux
    desktop: Alt plus the letter presses or focuses the control, letters allocated from the rendered labels by the Linux
    menu bar's rule (no catalog marker, so no translator work and no new strings). How holding Alt shows them (David's
    decision, 2026-11-18): Windows 11's keytips on Windows, underlined letters on every Linux desktop as GTK and Qt draw
    them, and a label without its letter (Chinese, Japanese) carries it as "保存 (B)". Of task 59's 14 `Alt+<letter>`
    keys, the 12 inside dialogs (multi-rename's 11 options, About this's Stop) stay the commands they are and count as
    their dialog's access keys (`keyScope` keeps their letters off every other control); they can't become label-derived
    letters because the keymap is per OS, not per language. The viewer's two (Alt+R, Alt+C) aren't in a dialog and stay
    as they are: the viewer has no other access keys to clash with. All five desktop rows are pinned by
    `ModalDialog.conventions.svelte.test.ts`, a11y included; `CMDR_MOCK_CONVENTIONS=windows pnpm dev` (or `kde`,
    `gnome`, `otherLinux`) shows another desktop's row. Linux: per desktop. Needs 59.
62. [S] Windows. Typography and motion: Segoe UI Variable through the system font stack, the Windows type ramp, Windows
    animation curves and durations, and the system's animation setting. Linux: follows the desktop's font and
    reduced-motion settings. Needs 40.
63. [L] Windows and Linux. The menu model. **Provisional** (D5): blocked on the menu spike (`tools/menu-spike/`,
    `docs/notes/windows-port/menu-spike.md`), which David runs on a Windows 10 and a Windows 11 laptop before any
    custom-menu build; the plan below is the in-page outcome, kept as researched. Recommended by the lead (2026-11-18):
    off macOS, Cmdr draws its menus in the page from `cmdr-menu`'s data, and muda keeps only macOS's bar and every
    right-click menu. Windows, KDE, and other Linux show an in-window menu bar (`menuModel: 'windowMenuBar'`), Fluent on
    Windows and the neutral theme on Linux; a tap of Alt or F10 focuses it, Alt plus a title's letter opens a menu,
    letters and keytips come from the dialogs' `access-keys.ts`, and a modal surface makes it inert. GNOME shows a "Main
    menu" button instead (`'primaryMenu'`, new): the bar's menus as submenus plus the HIG's standard group, F10 opens
    it. A "Show menu bar" setting swaps the two everywhere off macOS (on Windows a hidden bar shows on Alt or F10, on
    KDE a hamburger button stands in; no default key, since KDE's `Ctrl+M` is Multi-rename). The palette stays the way
    to every command. Evidence, per-desktop walk-throughs, and rejected options (muda's Win32 bar, a command bar,
    palette only, a bar on GNOME): `docs/notes/windows-port/menu-model.md`. Steps:
    1. [XS] `MenuModel::PrimaryMenu`, the GNOME row, `bindings.ts`, `conventions-on.json`, and the conventions docs
       (`platform-conventions.md`'s `menuModel` line points to the menu-model note).
    2. [S] A desktop command answering the bar for a window the way `cmdr-listing-wasm`'s `DemoMenuBar` does (rows,
       resolved labels, shown keys, enabled and checked state, live labels), plus a change event, all from `cmdr-menu`
       and `MenuItemInputs`, ❌ no host-side rule.
    3. [M] A shared `lib/ui` menu bar, promoted from `lib/demo/menu-bar.svelte.ts` onto `Menu.svelte`: ARIA `menubar`,
       Alt tap and F10 focus, Alt+letter, arrow walking, Esc returning focus, letters from `access-keys.ts` (reserving a
       scope's own Alt keys, as the viewer's `Alt+R` / `Alt+C`), per-platform look tokens (task 60), `forced-colors`,
       and inert under the menu-dialog gate. The viewer window gets its own bar from `menu_structure.rs`'s rows.
    4. [S] The main-menu button (GNOME's default, and the KDE and Windows stand-in when the bar is hidden), plus the
       "Show menu bar" setting and View row.
    5. [S] Hosts: `menu_bar_builder.rs` builds the bar on macOS only; Windows answers a zero-`lParam` `SC_KEYMENU`
       itself so a lone Alt or F10 doesn't start the system menu loop; `bar_parity_test.rs` narrows its registered-key
       checks to macOS and keeps the shown-key ones.
    6. [XS] The text-field gate lets a bar CLICK on a file-moving command run (task 55's entry).
    7. [S] Linux cleanup: delete `menu/mnemonic_mask/` and its IPC, `menu-mnemonics-capture.svelte.ts`,
       `get_menu_bar_title_mnemonics` and Settings' title-mnemonic warning, and `menu-mnemonic-mask.spec.ts`; the Linux
       E2E lane gets a bar spec in their place. Update `menu/` and `cmdr-menu`'s C+D.md and `lib/ui/DETAILS.md`'s
       Windows-bar gotcha.
    8. [S] The demo draws the visitor's own model with the shared bar (macOS bar, Windows bar, or the Linux bar) and
       picks the demo-only key for the search dialog's new query (task 55's entry). Reused unchanged: the bar data, ids,
       labels, enabled fold, sort checks, every context menu, and the snapshots. QA: task 55 (keys), task 64 (Narrator,
       NVDA, JAWS, and Orca read the bar as a menu bar). Needs 26, 59, 61.
64. [M] Windows. Screen reader parity: a JAWS, NVDA, and Narrator pass (task 59's `screenReaders`) over the main flows
    (panes, dialogs, Settings, the viewer, operations in progress), fixing what the ARIA work for VoiceOver doesn't
    carry through WebView2 and UI Automation, plus the Windows contrast themes (`forced-colors`). The findings become
    part of task 55's checklist. Linux: the same pass with Orca, later.
65. [S] Windows. System surfaces: native file pickers, toast notifications under Cmdr's app identity, a dark title bar
    in dark mode, and Mica or another system backdrop on Windows 11. Linux: portals for pickers and notifications. Needs
    52 for the app identity.
66. [S] Windows, David. A native-feel checklist for task 55: per surface, what "native" means on Windows with reference
    screenshots from Windows' own apps, so the QA pass has something to check against. An agent drafts it; David owns
    it.

### Later, not in the first Windows release

- OCR via `Windows.Media.Ocr`, CLIP on ONNX Runtime + DirectML.
- Explorer shell-extension verbs in Cmdr's own context menu (`IContextMenu`), preview handlers, ARM64 builds.
