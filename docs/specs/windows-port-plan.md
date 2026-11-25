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
- **D7. The Group Policy key** (David, 2026-11-18): `SOFTWARE\Policies\Rymd\Cmdr`, under `HKLM` and `HKCU`, the machine
  winning per value. `Rymd` is the vendor level (Rymdskottkärra AB, which will ship a second product), the
  `Policies\<vendor>\<product>` shape Chrome and Firefox use. Settled for good: admins' GPOs name the path once they
  deploy, so it can't move. Nothing shipped on Windows before it, so there's no older path to read or migrate.
  Canonical: `apps/desktop/src-tauri/src/managed_policy/DETAILS.md` § The Windows registry keys.

- **D5. The menu model** (David, 2026-11-18, after running the menu spike on Windows 11): off macOS, Cmdr draws its
  menus in the page from `cmdr-menu`'s data. Windows, KDE, and other Linux desktops get an in-window menu bar
  (`windowMenuBar`), GNOME a "Main menu" button opened by F10 (`primaryMenu`, new), and a "Show menu bar" setting flips
  between the two. muda keeps macOS's bar and the right-click menus on macOS and Linux. Task 63; evidence and rejected
  options: `docs/notes/windows-port/menu-model.md`; the spike's results: `docs/notes/windows-port/menu-spike.md` §
  Decision.
  1. **One style on Windows 10 and 11**: Windows 11's Fluent look, matched precisely. The exact tokens come from WinUI's
     open-source theme resources (`microsoft/microsoft-ui-xaml`: `MenuFlyoutPresenter`, `MenuFlyoutItem`, `MenuBar`
     padding, item height, corner radii, border stroke, shadow, the Segoe UI Variable type ramp, the accelerator text
     color). Acrylic through `backdrop-filter` blur plus a tint.
  2. **The right-click menus go in-page on Windows too**, sharing one menu component with the bar (today they're native
     Win32 popups).
  3. **Access keys are underlines**, no WinUI keytips, on the bar, in menus, and in dialogs. They show only after Alt or
     other keyboard use, as in Win32, and always when Windows' "Underline access keys" is on (`SPI_GETKEYBOARDCUES`).
  4. **ARIA done properly**: `menubar` / `menu` / `menuitem` (plus `menuitemcheckbox` and `menuitemradio`),
     `aria-haspopup`, `aria-expanded`, `aria-disabled`, roving focus, and `aria-keyshortcuts`. `forced-colors: active`
     styles go in with the build; David tests high contrast and Narrator himself later.
  5. **Accepted limit**: an in-page menu can't extend past the window's edge, so near an edge it flips or scrolls.
  - Why: the native modes (muda's bar, and an in-page strip opening native popups) work but felt Win32-like. Windows
    11's own look (Notepad, Explorer) is WinUI 3 XAML, and hosting that from Rust through XAML Islands isn't worth it:
    no `windows-rs` WinUI 3 bindings, it needs the Windows App SDK runtime, and it adds a third input owner beside
    WebView2 and the host window. VS Code, Slack, and the new Teams draw in-page menus on Windows too.

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

- **D8. Volume ids on Windows** (David, 2026-11-18, option (d) of `docs/notes/windows-port/volume-id-options.md`):
  Windows runs the rule macOS and Linux already run. A volume's id is the disk's identity; its root is only its address.
  Task 34.
  - A local drive or folder mount keys on its volume GUID (`\\?\Volume{…}\`, through `local_volume_id`, `vol-`); the
    system drive keeps `"root"`. A mapped drive (`Z:`), the same share by its UNC path, and Cmdr's own SMB connection to
    it are ONE `smb-` volume keyed on `(server, port, share)`, with `Z:/` as its canonical root and the UNC spelling as
    an alias root. A `subst` drive, which has no GUID, keys on its path (`path-`). macOS and Linux don't change.
  - Plus a saved last root per volume, so a restored tab or an undo after a drive-letter swap finds the disk it meant
    instead of whatever now sits on that letter.
  - Why: identity-keyed state (the drive index, favorites, `lastUsedPaths`, the operation log's volume) follows the
    disk, a share gets the answer macOS already gives an OS mount plus the direct backend, and tasks 35 (the mount table
    falls out of the GUID sweep), 39 (an arrival names the GUID), and 45 (a mapped share is the `smb-` id) get simpler.
  - ❌ Rejected: (a) the root path as the id: a letter swap hands one disk's index, favorites, and rollbacks to the
    other, and one share gets three volumes. (b) the GUID with the letter as display only: fights D1 (the letter is in
    every app path) and has no answer for UNC shares. (c) path ids with a GUID keying only the index: fixes the cheapest
    store and leaves every store holding user intent on the letter, with a Windows-only second identity in shared code.

Open, waiting on David (each task entry has the detail):

- **macOS hand checks** from the keymap work (task 26).

## Bugs the audits found on the way

- **Linux: image indexing writes invented data.** Off macOS the production media backend is `FakeVisionBackend`
  (`crates/cmdr-index/src/media_index/scheduler/lifecycle.rs:231`), and the setting isn't hidden
  (`apps/desktop/src/lib/settings/definitions/indexing.ts:91`), so turning it on fills `media.db` with OCR text and tags
  derived from filenames. Fixed in [#943](https://github.com/getcmdr/cmdr/issues/943) (task 1): off macOS the scheduler
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
check shrinks it). Milestones and what the builder ran: `docs/notes/windows-port/build-progress.md`. Every test failure
there by cause: `docs/notes/windows-port/windows-test-failures.md`. The first real-desktop check of the menus, browser
keys, and look (tasks 34, 60, 62, 63, 67), with its open problems:
`docs/notes/windows-port/real-windows-check-2026-11-19.md`.

Honest stubs: Windows arms that compile and answer "unsupported", "unknown", or nothing, each waiting for the task that
owns its real body. Remove a line when its body lands.

- `cmdr_fs::metadata` (`metadata/platform/windows.rs`), tasks 35 and 36: the mode, xattrs as alternate data streams
  (#1375), and the DACL answer `NotApplicable`. Real: dates, and the read-only (a FILE's, as Locked) and hidden
  attributes as user flags a copy reads and puts back (`flags_of`, `add_flags`; #1352), the Locked a move asks about
  included (`removal_lock`, `clear_blockers`, #1331).
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
- `cmdr_local::LocalWindowsVolume` (`platform/local_windows.rs`), task 35: no ADS-backed xattrs (the contract doesn't
  map onto streams cleanly: #1375). It keeps the trait's conservative default, and each conformance cell it can't pass
  yet is `#[ignore]`d with its piece (`local_conformance_test.rs`). Real since 2026-11-22: streams, the copy scan,
  `node_identity`, metadata (dates, read-only and hidden as flags, the caps), and links (symlink and junction targets,
  reparse tags told apart, file vs folder symlinks made without elevating), and `write_access_at` (the volume's
  read-only flag and the DACL), and a lister off `read_dir`'s find data (`crates/cmdr-local/DETAILS.md` § "The Windows
  sibling").
- `cmdr_local::reading` Windows arm (`reading/platform/windows.rs`), task 35: no allocation size and mode `0`; only
  `FILE_ATTRIBUTE_HIDDEN` is real. No owner or group by decision (a SID per entry, and nothing shows the owner yet:
  `crates/cmdr-local/DETAILS.md` § "The Windows sibling").
- `cmdr_operations::local_disk` Windows arm (`local_disk/platform/windows.rs`), tasks 35 and 36: `set_mode` and
  `symlink` refuse as unsupported (a copied link fails its item, #1376; a copied folder's mode narrowing is skipped),
  and there's no purgeable-space figure. Real: the write probe (`FILE_WRITE_DATA` open), free space
  (`GetDiskFreeSpaceExW`), the folder volume (`LocalWindowsVolume`), and a read-only file as Locked (`is_locked_item`,
  `locked_against_removal`).
- `cmdr_operations` local copy strategy on Windows (`transfer/copy_strategy.rs`), task 36: every file takes the chunked
  loop (cancellable, live progress, staged). No `CopyFileExW` yet, so a copy carries no attributes beyond read-only and
  hidden (the flags, `cmdr_fs::metadata`) and no alternate data streams (Mark of the Web included), and gets no ReFS
  block clone or SMB copy offload.
- `cmdr_operations::delete::cloud_trash::metadata_is_online_only` on Windows, task 37: never online-only, the cautious
  branch, so a Cloud Files placeholder trashes (and hydrates) like any file.
- `cmdr_operations` extract quarantine on Windows (`transfer/volume/extract_quarantine.rs`), task 37: an extract carries
  no Mark of the Web.
- `cmdr_fs::file_identity` on Windows (`file_identity/platform/windows.rs`), tasks 35 and 36: the path readers are real
  (`FileId::at`, `same_node_at`, `device_at`, through a handle), but every `Metadata` reader (`FileId::of`, `device_of`,
  `link_count`) answers `None`, since stable std's `Metadata` carries no id, serial, or link count there. So a walk's
  hardlink dedup counts each name (the total over-counts), the journal's `NodeIdentity` is absent, and a delete walk's
  per-folder mount test skips the device shortcut and asks the mount table for every folder. Each `Metadata` consumer
  moves to a path reader as it needs one; the guards that hold a path already ask by path, and so do the copy ledger's
  node (`WrittenIdentity::node_of_stat`, which falls back to `FileId::at`) and `LocalWindowsVolume::node_identity`.
- `cmdr-operations` tests on Windows: 65 test files that drive `LocalPosixVolume`, POSIX modes, or errnos are
  `#![cfg(unix)]` (three single cells too), and `lib.rs` allows `dead_code` and `unused_imports` in a Windows test build
  for the fixtures only they use. Each file un-gates as `LocalWindowsVolume` grows.
- Tests ignored on Windows by issue: 24 app-path tests in `cmdr-fs` (#1225, app paths through `std::path`) and one
  `sqlite_util` deletion test (#1226). `cmdr-index`'s suite isn't green there yet: 158 of 1,911 fail, filed as #1232.
- The drive table on Windows (`volumes/platform/windows/`), task 39: read at startup and on each switcher refresh, with
  no arrival or removal watcher, so a drive plugged in later shows on the next refresh and one pulled stays registered
  until then. A mapped share carries no `SmbMountInfo`, so nothing upgrades it to a direct session (task 45).
  `live_volume_space`, `mount_identity_at`, and `network_mount_under` answer nothing (task 35; the mount table they'd
  read is `cmdr_local::mounts`, real since 2026-11-21).
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
    purpose during the pass on 2026-11-18; `docs/notes/windows-port/macos-copy-accepted-review.md` is the record.
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
31. [M] Win. **Done, deployed** (migration `0033` applied to production D1 and verified on 2026-11-18). Downloads and
    analytics for N platforms: `/download/:version/:os/:arch` (`?kind=` for an OS's other file;
    `/download/:version/:arch` stays the macOS DMG alias), every non-DMG filename read from the release's own
    `latest.json` `downloads`, and an OS served only once `PUBLIC_DOWNLOAD_OSES` (`apps/api-server/wrangler.toml`,
    `macos` today) lists it. UA classification judges a hit against the file's OS (`classifyUaFamily(ua, os)`). The app
    sends `os` (`crate::platform::os_name()`) on the heartbeat, feedback, crash reports, and error reports, and the
    api-server stores it leniently (`parseClientOs`; older rows read from `os_version`). Migration `0033_client_os.sql`
    adds `downloads.os` / `kind`, `os` in the `downloads_daily_unique` key, and `os` on the four report tables. The
    dashboard splits downloads (table and day pie) and daily actives (tooltip) by OS; the funnel gains `downloadsByOs`,
    active installs `byOs`. `apps/api-server/src/telemetry/DETAILS.md` §§ The OS field, Download tracking.
    - **Deploy**: `deploy-api-server.yml` applied `0033` to production D1 before deploying the Worker (verified
      2026-11-18), and the same landing deployed the dashboard and the website (the privacy policy's download lines name
      the OS and file type).
    - **Making Windows downloads public** (after task 52 ships the files): add `windows` to `PUBLIC_DOWNLOAD_OSES` and
      deploy, then task 32 links `/download/latest/windows/x86_64`. Not in this task: the website's
      `getcmdr.com/download/latest/<os>/<arch>` nginx redirect (task 32), an OS on `install_summary` (new installs and
      retention by OS). The privacy policy's per-OS wording landed separately (2026-11-22: Credential Manager, the
      `MachineGuid` device hash, panic-only crash reports, "OS version"), public at once. Needs 30.
32. [S] Win. Website download page for three OSes (the Linux plan's Part 3, built once). **Done** (not landed or
    deployed at writing; David reviews copy and design later, screenshots of every state from the build). One gate,
    `offeredPlatforms` in `apps/website/src/lib/download-platforms.ts`: an OS is offered when `PUBLIC_DOWNLOAD_OSES`
    (`apps/api-server/wrangler.toml`, read at build time, macOS always) lists it AND the published `latest.json` carries
    its website file (NSIS, AppImage, `.deb`; the MSIX is the Store's). So adding `windows` to that var is the one
    switch for the API and the site together, and today's site offers macOS only. Every button, the blog's inline
    dropdown, the download section's per-OS cards ("Also for …" switches), pricing's platform line, the home schema, the
    llms files, and `/trust#windows` read it. An inline picker detects the visitor's OS and arch on the device (nothing
    sent), points the buttons at their OS when it's offered, and tells a Windows or Linux visitor whose build isn't out
    that it's on its way (Linux: build from source), pointing at the existing newsletter signup. The Windows card
    carries a one-line SmartScreen "More info, then Run anyway" hint; task 58 adds the Store link and the fuller
    explainer. nginx passes `/download/latest/<os>/<arch>` through (the API gates it). Fixed on the way: a non-Mac
    visitor's arch picked a Mac build (a Windows PC's x64 offered the Intel DMG and marked it ✓), and Safari's masked
    "Apple GPU" renderer offered Intel Macs the arm64 DMG. After review: the build fails without the universal DMG or on
    a misread toml, `deploy-website` waits until the API serves each newly public OS (15 minutes, else the old site
    stays), nginx 404s `/download-fixture`, touch-screen Linux user agents (Android tablets in desktop mode) get a
    neutral note, and the copy promises only the monthly newsletter. Tests:
    `apps/website/e2e/download-platforms.spec.ts` (unit and built-site, plus `/download-fixture` for the shipped
    states); no visual baseline changed. `apps/website/DETAILS.md` § Download platforms. Needs 30.
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
    `commands/rename.rs` and `commands/file_system/path_checks.rs` ask. **Done (2026-11-18, D8)**: the id scheme
    (`cmdr_fs::volume::windows_volume_ids`, the system drive staying `root`), a volume per drive, folder mount, and
    mapped share from the Windows arm's drive table (`volumes/platform/windows/`, with each Win32 answer verified on the
    builder), a mapped share's UNC spelling as an alias root of its one `smb-` volume, path resolution for any drive and
    UNC path, the root index mount-rooted at the system drive (`cmdr_fs::mount_layout::boot_volume_root`, a non-system
    drive indexing under its own id from its own root), app-spelled priority roots and index paths, the MCP volumes
    row's real mount path, the path bar reading like Explorer's address bar (`C:\`, `C:\Windows`), and each volume's
    last roots saved across sessions (`location/last_roots.rs`) so a restored tab and an undo follow a drive to its new
    letter, for an id never seen at two roots at once. Volumes/`DETAILS.md` § "Windows: a volume per drive" has the
    shape. Left for later tasks: arrival and removal (39), a mapped share's direct-SMB upgrade (45), and the mount
    table's identity and space readers (35); the stub line in § "Windows build state". Decided (2026-11-18): consumers
    name the host's local volume `cmdr_local::LocalVolume`, and app paths become a typed `AppPath` the `Volume` trait
    takes (#1225); both are milestones of `docs/specs/app-path-plan.md`.
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
    read-only file, a path past `MAX_PATH`, a junction). **Streams, copy scan, and node identity done** (2026-11-22):
    both local volumes share `cmdr-local`'s `streams.rs` and `copy_scan.rs`, and the copy ledger's node falls back to
    `FileId::at`. **Metadata, links, and the write-access probe done** (2026-11-22, PR #1374): read-only and hidden ride
    as the Locked and Hidden flags, so a copy and a cross-drive Move anyway keep them (#1352); symlink and junction
    targets read, reparse tags told apart (a OneDrive placeholder or an app execution alias is the item), and file vs
    folder symlinks made without elevating (`NotSupported` without Developer Mode); `write_access_at` reads the volume's
    read-only flag and the DACL. Decided against: owner names (no consumer, a SID per entry) and, for now, ADS-backed
    xattrs (#1375). **The lister done** (2026-11-22): rows come off `read_dir`'s own find data (`std`'s
    `FindFirstFileExW`, basic info) with no stat per entry, a listing of 20,000 files three times faster (246 ms against
    732), and `FIND_FIRST_EX_LARGE_FETCH` bought nothing (`docs/notes/windows-port/windows-lister-2026-11.md`). Left:
    the engine's own Windows `symlink` stub in `cmdr-operations` (#1376), and registering a volume per drive (with 34).
    The ignored conformance cells in `local_conformance_test.rs` are the checklist. App paths in and out of both local
    volumes (one crossing per OS through `AppPath::to_native`): `docs/specs/app-path-plan.md`. File identity is real by
    PATH (2026-11-21, #1291, PR #1304): `cmdr_fs::file_identity::FileId::at` / `same_node_at` open a handle and read
    `FileIdInfo` (`file_identity/platform/windows.rs`), which the copy's same-file guards and the source binding ask;
    the `Metadata` readers (`device_of`, `link_count`, walk dedup, the journal's `NodeIdentity`) still answer `None` and
    move over as each consumer needs it. The mount table is real (2026-11-21, `cmdr_local::mounts`' Windows arm): drive
    letters and folder mounts, "is this a mount point" by `GetVolumeNameForVolumeMountPointW` (a junction is none), the
    app's drive list reading the same table; and the device reads by path (`file_identity::device_at`, the volume
    serial), so a same-drive move renames. So a move and a folder Overwrite are refused as `MountedVolumeInside` only
    over a real mount; on the desktop a move onto another drive and a folder Overwrite land (unverified on a real folder
    mount, which would take a `mountvol` change). Left of the mount half: the ledger's and journal's node identity from
    `Metadata` (the ledger records `Unverifiable` on Windows), and the app's `live_volume_space`, `mount_identity_at`,
    and `network_mount_under` arms. Bugs it owns: the six `cmdr-operations` cells that fail on Windows on `main`
    (`docs/notes/windows-port/windows-test-failures.md` § "By milestone", item 9). ❗ The engine's Windows `symlink`
    (`cmdr-operations/src/local_disk/platform/windows.rs`) is a stub that always refuses, so no link lands on Windows
    until this task makes links real. **Listing watch done** (2026-11-21, #1330): `LocalWindowsVolume` arms the `notify`
    (`ReadDirectoryChangesW`) watch, with coverage by drive kind (fixed, removable, and `subst` `EveryWriter`; mapped
    and UNC `ThisMachineOnly`), the `\\?\` crossing both ways, case-only renames, and lost events re-reading through
    `vendor/notify` (overflow and a deleted root, notify 9.0's fix backported; the index's notify backend gets it too,
    #1128): `apps/desktop/src-tauri/src/file_system/DETAILS.md` § "Windows listing watches" (verified in the running app
    on the builder). Left: an open watch blocks renaming an ANCESTOR of the watched folder (NTFS, whatever the share
    mode) and likely vetoes a removable drive's eject, so the watch has to let go on demand (#1340, partly task 39's
    device notifications); and a deleted folder on a drive other than `C:` re-lands on itself (#1341).
36. [M] Windows. `cmdr-operations` arms: `CopyFileExW` strategy (with ReFS/Dev Drive block clone), Recycle Bin via
    `trash` with identity, read-only as "locked" in `clear_blockers`, sharing violations as a typed refusal. Needs 35.
    Plus the honest `Unknown`-identity rule (`cmdr_fs::file_identity` module docs): a move onto a taken name with
    unknown identity goes through the conflict flow instead of counting as already in place (`move_op/mod.rs` and
    `conflict_preflight.rs`), a copy asks instead of duplicating (`copy/mod.rs`, `free_space.rs`, `scan_dry_run.rs`,
    `routing.rs`), and a compress onto an existing archive asks instead of refusing (`archive_edit/fresh_plan.rs`). The
    `unknown_identity_today_*` tests flip with it. **Done** (2026-11-21, #1291, PR #1304): the per-file guard in
    `copy/single_item.rs` refuses an overwrite it can't tell apart from its source with the typed `CantTellIfSameFile`
    (its own copy), ❌ never a skip; the rest of the rule can reuse that refusal. Plus the typed-OS-error gaps task 10
    left for the Windows arms (35 and 36 between them):
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
    chunked loop. A local copy lands on the real desktop, Overwrite included (2026-11-21, PR #1304,
    `docs/notes/windows-port/build-progress.md`). Left, in data-safety order: the rest of the local engine's app paths
    (#1225, `docs/specs/app-path-plan.md` milestone 10a; a same-drive move's destination reaches `move_files`
    unanchored, `docs/notes/windows-port/real-windows-check-2026-11-19.md` problem 7), the rest of the
    `Unknown`-identity rule above, and the `CopyFileExW` arm. Stubs: § "Windows build state". **Delete done**
    (2026-11-21): the Recycle Bin (`delete/trash/platform/windows/`, `IFileOperation` one item per call, a progress-sink
    veto plus an up-front check of the drive and the bin's size limit (`bin.rs`) so an item no bin takes refuses as
    `NoTrashForVolume` and never deletes for good, the `$R…` item journaled so the trash undo renames it back and drops
    its `$I` record; `delete/DETAILS.md` § "Windows: the Recycle Bin"), the switcher's `supports_trash`, the volume's
    `has_trash`, and `bin.rs`'s refusal from one answer (`cmdr_local::recycle_bin`: a fixed disk with a volume GUID and
    its bin on, #1332), read-only as Locked (a permanent delete on any drive and an Overwrite refuse a read-only file as
    `FileLocked`, since `std::fs::remove_file` deletes through it: `local_disk::locked_against_removal`), and a sharing
    or lock violation on trash as `TrashRefusalKind::InUse` with a Retry (a permanent delete already refused as
    `FileInUse`). On the desktop: `docs/notes/windows-port/real-windows-check-2026-11-19.md` § "Task 36's delete on the
    desktop". A move of a read-only file asks the Locked question, same-drive and cross-drive, and the cross-drive sweep
    and `LocalWindowsVolume::delete` refuse a read-only original nobody agreed to (#1331,
    `crates/cmdr-operations/DETAILS.md` § "Moving Locked items"); a cross-drive Move anyway lands the copy read-only
    again (#1352, task 35's metadata). "Go to Recycle Bin" opens the bin in File Explorer (the app's `open_trash_view`,
    #1332; `delete/DETAILS.md` § "Windows: the Recycle Bin"). Seen beside it: no listing watch on a Windows drive
    (#1330, task 35's, now done).

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
    - Managed policy: `SOFTWARE\Policies\Rymd\Cmdr` in HKLM then HKCU (D7), with a live registry watch
      (`managed_policy/DETAILS.md` § The Windows registry keys).
    - On the builder (Windows 10 Pro 22H2, build 19045.6466, `52047a9377`, 2026-11-18):
      `cargo clippy -p cmdr --all-targets -D warnings` clean (297 s warm); `cargo nextest run -p cmdr --lib` over
      `secrets`, `platform`, `instance_lock`, `managed_policy`, and `system_memory`, 145 of 151 pass, among them the
      message box shown and dismissed, the registry policy read and its watch on scratch keys, the
      kernel-versus-registry build check, and the memory counters. The four Credential Manager round trips fail over SSH
      with `ERROR_NO_SUCH_LOGON_SESSION` (key-based OpenSSH logons get no credential vault) and pass, with no `CmdrTest`
      credential left behind, when the same test binary runs as SYSTEM from a scheduled task. The other two failures are
      the pid cells below.
    - The Windows half of `/trust` is drafted, hidden until a release carries a Windows download
      (`apps/website/DETAILS.md` § Patterns, "Copy for an OS that hasn't shipped"). David approved the copy
      (2026-11-22); the Store bullet is out until task 57 settles the Store facts.
    - The Group Policy template (2026-11-18): `rymd.admx` + `cmdr.admx` and their en-US ADMLs in
      `apps/desktop/packaging/group-policy/`, generated from `managed_policy/policies.json` by `scripts/group-policy/`
      and held to it by `desktop-group-policy-fresh`. Schema-valid under libxml2 and .NET (on mami, from a temp folder);
      not yet loaded in the Group Policy editor or Intune (task 55). Details:
      `apps/desktop/packaging/group-policy/DETAILS.md`.
    - Left for later tasks: the instance lock's holder pid, which reads as unknown on Windows (the two
      `instance_lock::tests` pid cells fail there, as before this task: `LockFileEx` locks the bytes the pid sits in).
39. [M] Windows. Drives: drive letters and labels, the device-arrival watcher (`CM_Register_Notification`), mapped and
    UNC drives, eject. Needs 20, 34. **Eject seam in place** (2026-11-18): eject is every-OS code, and what differs sits
    in `file_system/volume/eject/platform/` (the tool, ejectability, whether an SMB disconnect unmounts). The Windows
    arm is the stub in § "Windows build state"; the real one is `CM_Request_Device_EjectW` for a removable drive, the
    Restart Manager for holders, and `WNetCancelConnection2W` once 45 makes OS SMB mounts.
40. [S] Windows. System look: accent color, text scale, high contrast and transparency, UI language and formats, all
    live-updating (`UISettings`, `WM_SETTINGCHANGE`). Light/dark already arrives through WebView2. Needs 19, 23.
    - **Built** (2026-11-19), all through the existing channels, live from one settings window
      (`system_look/platform/windows/`: `WM_SETTINGCHANGE`, `WM_THEMECHANGED`, `WM_SYSCOLORCHANGE`, and `UISettings`'
      change events posted onto its thread), never polled. Accent: `UISettings` with WinUI's fill shades
      (`SystemAccent.fill`, `AccentDark1` / `AccentLight2`), which the Windows control tokens fill with
      (`--color-accent-fill`). Contrast themes: `SPI_GETHIGHCONTRAST` into `increase_contrast` and `reduce_transparency`
      (the colors stay WebView2's `forced-colors`). Transparency effects: `AdvancedEffectsEnabled` into
      `reduce_transparency`. Language and formats: `GetUserPreferredUILanguages` and `GetUserDefaultLocaleName` into
      `get_os_locales`, live through `intl`'s watcher on the same window (`apps/desktop/src-tauri/src/intl/DETAILS.md` §
      Windows). Text size: reports `1.0`, because WebView2 already zooms the page with it, live (Chromium's
      `UwpTextScaleFactor`); the trade (the whole page zooms, where WinUI grows text alone) is David's call
      (`docs/notes/windows-port/type-and-motion.md`). Unverified until someone changes the settings on a real Windows:
      that each Settings page broadcasts or fires its `UISettings` event, the shades on screen, a contrast theme's look,
      and the Text size zoom.
41. [S] Windows. File icons via `SHGetFileInfoW` / `IShellItemImageFactory`. Needs 23.
42. [S] Windows. Clipboard (`CF_HDROP`, `Preferred DropEffect` for cut) and drag-out of plain paths (OLE). Needs 18.
    **Plain text read done** (2026-11-19): `clipboard/platform/arboard_text.rs` answers `read_text` on Windows and Linux
    through `arboard` (the in-page text-field Paste needs it); the file arms stay `unsupported.rs`'s.
43. [M] Windows. Drag-out of virtual files (MTP, SMB, archives) via `CFSTR_FILEDESCRIPTORW` / `FILECONTENTS`. Needs 42.
44. [S] Windows. Open with, default app, text editors, terminal (`SHAssocEnumHandlers`, `wt -d`). Needs 18.
45. [S] Windows. SMB OS mounts via `WNetAddConnection2W` and Credential Manager SMB credentials. Needs 22.
46. [S] Windows. Small backend arms: `cmdr-nfs` identity (`GetComputerNameExW`, configurable uid/gid), `cmdr-adb`
    `adb.exe` discovery, special folders (`SHGetKnownFolderPath`), thread QoS (EcoQoS, not background mode), WinHTTP
    system proxy. Needs 8, 16. **Done** (2026-11-19), each arm behind its seam:
    - NFS identity (2026-11-18): `cmdr-nfs`' `dial/platform/windows.rs` asks as `nobody` with no groups, the machine
      name from `GetComputerNameExW(ComputerNameDnsHostname)`, and the app's `this_mac_identity` reads it through
      `NfsIdentity::of_this_process`; a place sets its own uid and gid in the add form, as on every OS.
    - `adb.exe`: the search (already `adb.exe` and `%LOCALAPPDATA%\Android\Sdk`) adds WinGet's and Scoop's per-user
      command folders (only folders the user or an admin controls: `docs/security.md` § "Helper tools"), and
      `adb start-server` runs with `CREATE_NO_WINDOW` (`cmdr-adb/src/server/platform/`), so a GUI Cmdr flashes no
      console window.
    - Special folders: the favorites seed reads the Desktop, Documents, and Downloads known folders (`dirs`, which calls
      `SHGetKnownFolderPath`), so OneDrive's folder backup and a moved folder seed right; the special-folder icon keys
      already classify both spellings.
    - Thread QoS: `cmdr_fs::thread_qos/platform/windows.rs`, `THREAD_PRIORITY_BELOW_NORMAL` plus EcoQoS (the
      execution-speed power throttle), ❌ never background mode.
    - System proxy: `cmdr-http` reads the user's Internet settings (`WinHttpGetIEProxyConfigForCurrentUser`) and walks
      WPAD, the setup script, then the manual proxy and bypass list; PACs run in WinHTTP's async resolver
      (`WinHttpGetProxyForUrlEx`, typed SOCKS and HTTPS answers) behind the shared PAC cache, and WPAD is discovered
      once per network in the background, both dropped on a network change (`crates/cmdr-http/DETAILS.md` § "The Windows
      answer").
    - Verified on the builder: `docs/notes/windows-port/build-progress.md` (2026-11-19, task 46). Not verified: a real
      proxy, PAC server, or WPAD network configured in Windows' settings (the builder's stay untouched), Windows 11's
      EcoQoS read-back, and a WinGet / Scoop `adb` install. Left: the `cmdr-sftp` agent's Pageant support (§ "Windows
      build state").
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
    `tauri signer sign --app-version <v>`: `release-finish` does (`signerArgs`), `windows-publish` does in `ci` mode,
    and `latest-json` refuses a `.sig` without it. `Validate` already holds the key to one the app reads
    (`windows-x86_64[-nsis]`, `linux-x86_64[-appimage]`). Also publish the advisory list as a release asset with its own
    `.sig` and write the optional top-level `securityAdvisories: { url, signature }` into `latest.json` (`latest.go`);
    without it Windows and Linux show no security notice (the update still installs). Once it's written, drop
    `securityAdvisories` from `NOT_WRITTEN_YET` in `crates/cmdr-update-manifest/src/tests.rs`
    (`scripts/release-assets/DETAILS.md` § Update payloads off macOS). Needs 6, 30.
    - **Ship no `Policies` keys inside the MSIX**: a packaged app reads the real policy hive through a merged view with
      the package's own keys, so a shipped key would mix into what admins set (Microsoft Learn's MSIX registry
      virtualization page, read 2026-11-18). Task 57 holds the read-only half.
    - **Wired, off by default** (2026-11-18): the manifest rows (NSIS as `windows-x86_64`, MSIX, the Windows Rust SBOM,
      task 53's Group Policy zip, and the signed advisory list), `ShippedOSes = [macos]` as the one switch (keyed by a
      `Since` version, prereleases skipping Windows), the `windows-build`, `windows-publish`, and `windows-attest` jobs
      as a leg that never holds or fails the macOS release (`scripts/release-assets/manifest/legs.go`), `vet-nsis`
      (stub, packed app, and uninstall entry), `securityAdvisories` in `latest.json`, a Windows-on dry run in CI, and
      `scripts/windows-package/` (MSIX via winapp, unsigned, from the unbundled exe so its updater stays off; no
      registry keys). `docs/guides/releasing.md` § Windows releases has the flow and David's steps (Partner Center
      account, the product identity into `apps/desktop/packaging/msix/identity.json`, copy review, the switch).
      `NOT_WRITTEN_YET` keeps `securityAdvisories` until the release that turns Windows on.
53. [S] Windows. Release vetting and distribution: `deny.toml` triple, third-party notices and SBOM, checksums for both
    packages, and a winget manifest for the NSIS installer. Needs 52.
    - **The Group Policy template**: attach `apps/desktop/packaging/group-policy/` to each release as one zip
      (`cmdr-group-policy-<version>.zip`, the folder's layout: two ADMX files plus `en-US/` with their ADMLs), as a
      manifest row with its checksum, and link the latest one from `/trust#windows-policy` once that section is public.
      The release reads the committed files (`desktop-group-policy-fresh` already holds them to `policies.json`).
54. [S] Windows. E2E smoke on `windows-latest`: `tauri-plugin-playwright` over TCP, three-way helpers, a Windows fixture
    root, a small spec subset. Needs the app to run on Windows (34–36).
    - **The Rust half is in (2026-11-21)**: the Windows lane's `tests` job runs every member's suite on `windows-latest`
      against a shrink-only known-failures list (`scripts/windows-test-baseline/ci-known-failures.tsv`, 239 entries on
      2026-11-21), required for Rust-touching landings like clippy (`docs/tooling/ci.md` § Windows lane). The E2E smoke
      above is what's left.
55. [M] Windows. Windows QA pass on a real machine: an agent drives the checklist and fixes what it finds; David QAs
    look, feel, and timing-sensitive UI. Open from the first real-Windows check (2026-11-19,
    `docs/notes/windows-port/real-windows-check-2026-11-19.md`): the check's three low findings, now #1305 (the "Skip
    Mac-only hidden files" toggle on Windows, a decision), #1306 (the MCP bridge runs nothing), and #1307 (Settings slow
    under load). Fixed (2026-11-21, PR #1304): #1292 (the Copy dialog refused every local copy: the frontend expanded
    `~` against Tauri's native `homeDir()`) and #1291 (an Overwrite copy skipped silently: no file identity).
    - **Keyboard (from task 26; unit tests cover the data, these need the real OS)**:
      - What WebView2 reports for AltGr (`AltGraph`, `ctrlKey`, `altKey`) on German and Hungarian layouts, for
        LeftCtrl+LeftAlt+E, and for right Alt with empty modifiers (Multi-rename's `Alt+<letter>` would then need the
        left Alt).
      - The browser keys WebView2 keeps until task 67 (#1115) lands: `Shift+F5`, `Ctrl+F5`, `F5`, `F3`, `F7`, `Ctrl+R`,
        `Ctrl+J`, `Ctrl+G`, `Ctrl+Shift+G`, `Ctrl+Shift+C`, `Ctrl+Shift+P` (the menu spike's page opened WebView2's
        Print dialog on it), `Ctrl+0` / `Ctrl++` / `Ctrl+-`, and `Alt+←/→` reach Cmdr.
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
      - The menu bar (task 63; D5's bar is in-page, so no Win32 accelerator is in play): a lone Alt tap and F10 reach
        the page and focus the bar, without Windows' menu loop stalling IPC (`SC_KEYMENU`; a click on the panes still
        answers within a second); Alt+Space still opens the system menu; with the search dialog or Multi-rename open,
        `Alt+F` / `Alt+S` / `Alt+V` and `Alt+F` / `Alt+E` / `Alt+G` stay the dialog's and the bar doesn't open; with
        "Show menu bar" off, Alt and F10 show it until focus leaves.
      - The download toast's hotkey animation: its PC key caps read `Ctrl`, `Fn`, `Win`, and `Alt` where a Windows
        laptop has them.
      - With a text field focused, a menu CLICK on a file-moving command (File > Delete, Edit > Move here) runs, and the
        same command's key still doesn't (`routes/(main)/text-field-gate.ts`): task 63's in-page bar knows a click from
        a key (its step 9).
      - The browser demo for a Windows or Linux visitor: the search dialog's new query keeps `Ctrl+N`, which Chromium
        keeps from the page, because a fixed-key command can't be seeded (`lib/demo/browser-shortcuts.ts`). Task 63
        gives the demo the visitor's own bar (its step 11), which picks the demo-only key.
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
    - **Group Policy (from task 38; the schema check and the registry reads are covered, these need a real policy
      store)**. On a machine where changing local policy is fine (a VM or a fresh test box, ❌ not mami while it's
      shared):
      - Copy `rymd.admx`, `cmdr.admx`, and `en-US\` from `apps/desktop/packaging/group-policy/` into
        `C:\Windows\PolicyDefinitions` (the local store), open `gpedit.msc`, and find Administrative Templates › Rymd ›
        Cmdr under both Computer and User Configuration, each policy with its explain text, the "Highest version" text
        box, and the "Hosts" list. No "resource not found" or namespace warning when the editor opens.
      - Enable "Turn off usage stats" for the computer, run `gpupdate /force`, and
        `reg query HKLM\SOFTWARE\Policies\Rymd\Cmdr` shows `DisableUsageStats REG_DWORD 0x1`; a running Cmdr locks the
        setting within a second (the watch). Disable it: `0x0`, unlocked. Not configured: the value is gone.
      - "Allowed cloud AI hosts" with two entries writes `...\Rymd\Cmdr\AllowedCloudAIHosts` with values `1` and `2`,
        and Cmdr's Settings lists both hosts in its summary of what the organization manages. Disable it: whatever is
        left of the subkey reads as no restriction.
      - The same policy set to Enabled for the user and Disabled for the computer: the computer's wins.
      - Intune (needs a tenant): import `rymd.admx` + `rymd.adml`, then `cmdr.admx` + `cmdr.adml` (Devices ›
        Configuration › Import ADMX), both reach Available, and an "Imported Administrative templates" profile shows the
        Cmdr policies.
      - Remove the files from `PolicyDefinitions` and the test GPO's settings afterwards.
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
    - **The Store build only reads policy keys, never writes them**: policies are the admin's, and a packaged app's
      registry view is merged and virtualized (Microsoft Learn's MSIX registry virtualization page, read 2026-11-18).
      Task 52 keeps `Policies` keys out of the package.
    - **Done without the account** (2026-11-24): the route and its sources
      (`docs/notes/windows-port/store-distribution.md`), the MSIX as a CI artifact (`windows-store-package.yml`), the
      `StorePackage` and `SideloadedPackage` install kinds (by the package's origin), the MSIX's own release switch
      (`StorePackageSince`), and the Windows triple in `deny.toml` and the notices (53a). Left: the account (#1348), the
      first submission by hand, `msstore` automation, and the data-dir virtualization fix (the note's § Capabilities).
58. [XS] Windows. The website's "Run anyway" download (D3 step 2): the unsigned NSIS installer on the download page with
    a short explainer of SmartScreen's "More info → Run anyway" step, next to the Store link. David reviews the copy.
    Needs 32, 52.

Added after the list was numbered, so it carries the next free number (a separate list, so a Markdown formatter keeps
its start number):

67. [XS] Windows. WebView2's browser keys off in every window (#1115, David's decision of 2026-11-12). Not in the tree
    yet (checked 2026-11-18: nothing in `apps/desktop/src-tauri/` sets `AreBrowserAcceleratorKeysEnabled`, and Tauri
    2.12.5 doesn't expose it; tauri-apps/tauri#16397, which adds `browserAcceleratorKeys`, is still open). The menu
    spike showed the cost: `Ctrl+Shift+P`, which the spike's page didn't claim, opened WebView2's Print dialog. In the
    main window the page's `preventDefault` on a bound key wins (the spike measured that for `F5`, `Ctrl+R`, and
    `Alt+←`), so the exposure is any window or state that doesn't claim a key: Settings, the viewer, a text field, a
    gated command.
    - Scope: a new `on_webview_ready` hook on the `cmdr-platform` plugin (`platform_plugin` in
      `apps/desktop/src-tauri/src/tauri_builder.rs`, which reaches every webview, the JS-created ones too) runs a
      `#[cfg(windows)]` `with_webview` that casts the core's settings to `ICoreWebView2Settings3` and calls
      `SetAreBrowserAcceleratorKeysEnabled(false)`, as the menu spike's `set_browser_keys` did
      (`git show c15f567151:tools/menu-spike/src/win.rs`). Use #16397's config field instead if it has shipped.
    - Landmines: the setting applies from the next navigation, so the first page of each window must be covered (set it
      before the first load, or check that it holds on the first page); `F6`, `Shift+F6`, `Ctrl+M`, and `Ctrl+Shift+M`
      may sit outside the global switch (#1115's comment), which then needs the per-key
      `IsBrowserAcceleratorKeyEnabled(FALSE)` in an `AcceleratorKeyPressed` handler (the spike's `install` shows the
      handler's wiring).
    - Tests: a Rust test can't reach WebView2; the check is the real machine (mami or a laptop).
    - Done when: #1115's list (`F5`, `Ctrl+R`, `Ctrl+F`, `Ctrl+P`, `Ctrl+Shift+P`, `Ctrl+J`, `Ctrl+G`, `Shift+F5`,
      `Ctrl+F5`, `Alt+←/→`, `F7`) does only what Cmdr binds, and nothing where it binds nothing, in the main window,
      Settings, and the viewer. Needs the app running on Windows (done).
    - **Built** (2026-11-18): `platform::prepare_webview`, called from the `cmdr-platform` plugin's `on_webview_ready`,
      turns the switch off in every webview (`apps/desktop/src-tauri/DETAILS.md` § Every webview's native setup).
      Clippy-clean for `x86_64-pc-windows-gnu` (the Mac cross-check). Not yet run on a signed-in Windows desktop: the
      done-when keys, the first page of each window, and `F6` stay on task 55's keyboard list.

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
    switches, text fields, scrollbars, focus visuals, and tooltips, as tokens keyed on the platform so components stay
    one implementation. Menus (the bar, its flyouts, and the context menus) are task 63's step 3, which sets the token
    shape and the source rule this task follows: WinUI's own theme resources (`microsoft/microsoft-ui-xaml`,
    `controls/dev/CommonStyles/*_themeresources.xaml`), matched precisely, one look on Windows 10 and 11 (D5). David
    reviews the design. Linux: its own neutral theme, same token set. Needs 59.
    - **Built** (2026-11-18), awaiting David's design review: the control look tokens in `app-controls*.css`
      (`apps/desktop/src/DETAILS.md` § Control look tokens), covering buttons, text fields, checkboxes, switches, radio
      buttons, sliders, progress bars, the select and combobox, tooltips, `ModalDialog`'s chrome, focus rings, and
      scrollbars, light, dark, and contrast themes. macOS and Linux keep the house look, proven unchanged by a
      catalog-wide computed-style diff. Sources, the gaps CSS can't close (the accent shade ramp, the select's chevron,
      WinUI's control animations, overlay scrollbars), and what stays house-styled:
      `docs/notes/windows-port/control-look-tokens.md`. Left for later: Linux's neutral theme (Linux keeps the house
      look until then). The accent's `Dark1` / `Light2` shades arrived with task 40 (`--color-accent-fill`).
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
    menu bar's rule (no catalog marker, so no translator work and no new strings). How they show (David's decision,
    2026-11-18, revised by D5 the same day): underlined letters on Windows and every Linux desktop, as Win32, GTK, and
    Qt draw them, and a label without its letter (Chinese, Japanese) carries it as "保存 (B)". Shipped: Windows draws
    underlines with Win32's keyboard-cue rule (`underlinesAfterKeyboardUse`: shown once the keyboard is in use in that
    dialog, always under Windows' "Underline access keys"), every Linux desktop while Alt is held
    (`underlinesWhileAltHeld`); `apps/desktop/src/lib/ui/keyboard-cues.ts` (task 63 steps 1, 4, and 5). Of task 59's 14
    `Alt+<letter>` keys, the 12 inside dialogs (multi-rename's 11 options, About this's Stop) stay the commands they are
    and count as their dialog's access keys (`keyScope` keeps their letters off every other control); they can't become
    label-derived letters because the keymap is per OS, not per language. The viewer's two (Alt+R, Alt+C) aren't in a
    dialog and stay as they are: the viewer has no other access keys to clash with. All five desktop rows are pinned by
    `ModalDialog.conventions.svelte.test.ts`, a11y included; `CMDR_MOCK_CONVENTIONS=windows pnpm dev` (or `kde`,
    `gnome`, `otherLinux`) shows another desktop's row. Linux: per desktop. Needs 59.
62. [S] Windows. Typography and motion: Segoe UI Variable through the system font stack, the Windows type ramp, Windows
    animation curves and durations, and the system's animation setting. Linux: follows the desktop's font and
    reduced-motion settings. Needs 40.
    - **Built** (2026-11-19), awaiting David's design review: `apps/desktop/src/app-type-motion-windows.css`
      (`apps/desktop/src/DETAILS.md` § Typography and motion). The Windows look's `--font-system` is
      `'Segoe UI Variable Text', 'Segoe UI'` (Chromium resolves only the variable font's named instances), read by
      `html, body` so inherited text follows; `--font-size-lg` takes the ramp's BodyLarge 18 (12, 14, and 20 were
      already Caption, Body, and Subtitle); `--transition-fast` / `-base` are WinUI's 83 ms `BrushTransition`, `-slow`
      250 ms on the fast-out-slow-in curve. Windows' Animation effects and Linux's reduced-animation settings reach the
      page as `prefers-reduced-motion` with no backend reader; Linux keeps the house stack's `system-ui`. macOS and
      Linux unchanged: 0 differing pixels across the demo's Mac shots. Unverified until a real Windows run: that
      WebView2 resolves the font name, and the reduced-motion mapping. Gaps: optical size (every size draws the Text
      cut) and WinUI's theme animations; Windows text scale is WebView2's own zoom (task 40). Evidence:
      `docs/notes/windows-port/type-and-motion.md`.
63. [L] Windows and Linux. The menu model (D5, final). Off macOS, Cmdr draws its menus in the page from `cmdr-menu`'s
    data. Windows, KDE, and other Linux show an in-window menu bar (`menuModel: 'windowMenuBar'`): Windows 11's Fluent
    look, matched precisely, on Windows 10 and 11 alike, and the neutral theme on Linux. A tap of Alt or F10 focuses it,
    Alt plus a title's letter opens a menu, letters come from the dialogs' `access-keys.ts` and show as underlines, and
    a modal surface makes it inert. GNOME shows a "Main menu" button instead (`'primaryMenu'`, new): the bar's menus as
    submenus plus the HIG's standard group, F10 opens it. A "Show menu bar" setting swaps the two everywhere off macOS
    (on Windows a hidden bar shows on Alt or F10, on KDE a hamburger button stands in; no default key, since KDE's
    `Ctrl+M` is Multi-rename). On Windows the right-click menus move in-page too, onto the same menu component; macOS
    and Linux keep muda's native popups. The palette stays the way to every command. Evidence, per-desktop
    walk-throughs, and rejected options: `docs/notes/windows-port/menu-model.md`; the spike that decided it:
    `docs/notes/windows-port/menu-spike.md`. Needs 26, 59, 61.
    - **Order and parallel lanes**: wave 1 runs steps 1, 2, 3, and 5 side by side (conventions data, a backend command,
      CSS tokens, a backend setting: no shared files), plus step 7's backend half. Wave 2 is step 4, alone (it touches
      `Menu.svelte`, `access-keys.ts`, and `ModalDialog.svelte`, which step 1's test edits also touch, so it starts once
      step 1 lands). Wave 3 runs steps 6, 7's frontend half, and 11 in parallel. Wave 4 runs steps 8, 9, and 10 in
      parallel. Step 12 goes last. Reference shots for the look: Notepad's menu bar and a File Explorer context menu on
      Windows 11, light and dark, taken on David's laptop.
    - **Kept from the menu spike** (deleted by step 12; its source stays in git history at `c15f567151`, for example
      `git show c15f567151:tools/menu-spike/src/win.rs`): `src/win.rs` `subclass` (the window subclass that swallows a
      `SC_KEYMENU` and lets Alt+Space's `lParam` 0x20 through: step 6's guard); `set_browser_keys` and `install`'s
      `AcceleratorKeyPressed` wiring (task 67's global switch and its per-key fallback); the `--selftest` harness
      (`SendInput` key script plus a BMP screen capture per step, which runs in mami's console session with the lid
      closed: the cheapest way for an agent to see step 6 and 7 on real Windows before David does). From `ui/app.js`:
      the lone-Alt tracking (`altAlone`, AltGr excluded) and `rememberFocus` / `restoreFocus`, as a cross-check for
      step 4. Not worth keeping: mode A's key bridge (`on_webview_key`), mode B's `popup_with_arrows` and its
      `WH_MSGFILTER` hook, `src/model.rs` (step 2's command replaces it), and `ui/style.css`.
    1. [XS] **Conventions data.**
       - Scope: `MenuModel::PrimaryMenu` and the GNOME row; Windows' `accessKeys` row flips from `keytips` to
         `underlines` (`apps/desktop/src-tauri/src/platform/conventions.rs`, its test, `bindings.ts`,
         `conventions-on.json`); a new `contextMenus` convention (`native` on macOS and every Linux desktop, `inPage` on
         Windows), so step 7 asks the convention, ❌ never the platform. With no row left on it, the `Keytips`
         convention value goes; `access-keys.ts` keeps its keytip renderer only as the fallback where the Highlight API
         is missing (WebKitGTK). Docs: `apps/desktop/src/lib/platform/DETAILS.md` § Conventions,
         `apps/desktop/src/lib/ui/DETAILS.md` § Platform conventions, and `platform-conventions.md`'s values list.
       - Landmines: `ModalDialog.conventions.svelte.test.ts`'s Windows block pins keytips and must flip with the row;
         the cross-target table test pins `conventions-on.json`, so regenerate it rather than hand-editing.
       - Tests: the conventions Rust tests (`cargo nextest run conventions`), `pnpm check svelte-tests-related`.
       - Done when: `CMDR_MOCK_CONVENTIONS=windows pnpm dev` shows underlines in a dialog on held Alt, and GNOME's row
         answers `primaryMenu`.
    2. [S] **The bar's answer, from the backend.**
       - Scope: a desktop command answering the bar for a window the way `cmdr-listing-wasm`'s `DemoMenuBar` does (rows,
         resolved labels, shown keys, `aria-keyshortcuts` spelling, enabled and checked state, check-versus-radio kind,
         live labels), plus a change event, all from `cmdr-menu` and `MenuItemInputs`, ❌ no host-side rule. A pick
         comes back as an id and dispatches on the `menu` road, as a native bar click does today.
       - Landmines: the viewer has its own rows (`menu_structure.rs`); live labels (like "Select all of the same kind")
         must come from the same live source the context menu uses, ❌ never a debounced copy; keep the payload small
         (it's sent on every change), so diff or debounce on the Rust side.
       - Tests: Rust tests that the answer equals `DemoMenuBar`'s for the same inputs, per platform; a snapshot of the
         Windows bar.
       - Done when: the frontend can draw every row the native bar has today, with live state, and no row is decided
         outside `cmdr-menu`.
    3. [S] **The Windows 11 look**, as tokens (task 60 follows its shape).
       - Scope: menu tokens keyed on the platform, read from WinUI's theme resources on `microsoft/microsoft-ui-xaml`'s
         `main` (`controls/dev/CommonStyles/MenuFlyout_themeresources.xaml`,
         `controls/dev/MenuBar/ MenuBar_themeresources.xaml`,
         `controls/dev/CommonStyles/CornerRadius_themeresources.xaml`, `Common_themeresources_any.xaml`,
         `controls/dev/Materials/Acrylic/AcrylicBrush_themeresources.xaml`): flyout padding
         (`MenuFlyoutPresenterThemePadding`), item margin and padding (`MenuFlyoutItemMargin`,
         `MenuFlyoutItemThemePadding`), minimum height, `OverlayCornerRadius` for the flyout and `ControlCornerRadius`
         for a row, the border stroke (`SurfaceStrokeColorFlyoutBrush`), the flyout shadow, the Segoe UI Variable type
         ramp (body for rows), and the accelerator text color (`TextFillColorSecondaryBrush`). Light and dark. Acrylic
         as `backdrop-filter` blur plus the acrylic tint and luminosity; a solid fallback under the
         `reduce-transparency` class (`DisplayAccessibility.reduceTransparency`, which task 40 fills from Windows'
         transparency effects). `forced-colors: active` styles from the same files' `HighContrast` dictionaries
         (`Canvas`, `CanvasText`, `Highlight`, `HighlightText`, `GrayText`), with no backdrop. Each token cites its
         resource key in a comment.
       - Landmines: the files hold both a current and a `_perf2026` variant, and several values are `ThemeResource`
         chains: resolve each to its final value and name the file it came from; the WinUI flyout is a separate window
         blurring the desktop, ours blurs the app beneath it (accepted); Segoe UI Variable isn't on Windows 10 by
         default, so the stack falls back to Segoe UI there (task 62 owns the stack).
       - Tests: a Vitest check that every Windows menu token is defined in light, dark, and forced-colors; David
         compares the bar and a flyout side by side with Notepad (design review, his call).
       - Done when: David signs off the look against the reference shots.
    4. [M] **One menu component** for the bar, its flyouts, and the context menus, in `lib/ui`, promoted from
       `lib/demo/menu-bar.svelte.ts` onto `Menu.svelte`.
       - Scope: ARIA `menubar`, `menu`, `menuitem`, `menuitemcheckbox`, `menuitemradio`, `aria-haspopup`,
         `aria-expanded`, `aria-disabled`, `aria-keyshortcuts`; roving focus (`tabindex` -1/0) on the bar and in each
         menu; Alt tap and F10 focus the bar, Alt+letter opens a menu, arrows walk (Left and Right cross to neighbor
         menus, entering and leaving submenus), Home and End, type-ahead by access letter, Enter or Space picks, Escape
         closes one level and then leaves the bar with focus back where it was (the Win32 two-Escape walk the spike's
         mode C showed). Letters from `access-keys.ts` (reserving a scope's own Alt keys, as the viewer's `Alt+R` /
         `Alt+C`). The keyboard-cue rule, for menus and dialogs alike: on Windows, underlines show once Alt or another
         key has been used in that surface (opened by Alt, F10, or Alt+letter: shown; opened by a click: hidden until a
         key is pressed), and always while step 5's setting is on; Linux keeps today's hold-Alt rule. Placement: a
         flyout flips to the other side or scrolls when it would pass the window's edge (D5's accepted limit), and a
         context menu opens at the pointer or at the row's anchor (`context-menu-anchor.ts`). Inert under the
         menu-dialog gate. The look comes from step 3's tokens.
       - Landmines: a lone Alt must not count when another key came between press and release, and AltGr (Ctrl+Alt on
         Windows) is never a menu key; Alt+Space, Alt+F4, and Shift+F10 stay theirs; focus must return to the exact
         element (a pane row, a text field) after Escape or a pick; WebView2 sends no `keyup` for Alt if the window lost
         focus mid-press, so blur resets the Alt state; a menu must close on window blur, a scroll of its anchor, and a
         dialog opening.
       - Tests: Vitest component tests for the keyboard walk, roles and states (axe-core), the cue rule, and edge
         placement at each window corner; `CMDR_MOCK_CONVENTIONS=windows pnpm dev` for a look on the Mac.
       - Done when: the demo's bar runs on the shared component with identical behavior, and the tests pin every key
         above.
    5. [XS] **Windows' "Underline access keys" setting.**
       - Scope: `SPI_GETKEYBOARDCUES` read in `system_look/platform/windows.rs` into `DisplayAccessibility` (a new
         field, false elsewhere), live on `WM_SETTINGCHANGE` through task 40's observer (or a one-setting arm of
         `observe_changes` if task 40 hasn't landed, which task 40 then extends).
       - Landmines: the setting is per user and changes without a restart; `SPI_GETKEYBOARDCUES` and
         `SPI_GETMENUUNDERLINES` are the same value, so read one; every `unsafe` needs its `SAFETY` line.
       - Tests: a Rust unit test of the mapping; the toggle itself on a real machine (Settings › Accessibility ›
         Keyboard › Underline access keys).
       - Done when: flipping the Windows setting shows or hides underlines without a restart.
    6. [S] **The bar in the windows, and the hosts.**
       - Scope: the main window and the viewer draw the bar (step 4) from step 2's answer on Windows, KDE, and other
         Linux; `menu_bar_builder.rs` builds the native bar on macOS only; Windows answers every `SC_KEYMENU` itself
         (ported from the spike's `subclass`: a lone Alt or F10's zero `lParam`, and an Alt+letter no title took, which
         would otherwise beep) so the system menu loop never starts, letting Alt+Space through; `bar_parity_test.rs`
         narrows its registered-key checks to macOS and keeps the shown-key ones.
       - Landmines: a lone Alt release that WebView2 leaves unhandled reaches `DefWindowProc` as `SC_KEYMENU` 0, which
         the guard swallows: the page must still see the keyup and focus the bar; F10 must be `preventDefault`ed by the
         page or Windows sees it too; the bar takes vertical space, so the pane height math and the E2E screenshots
         shift.
       - Tests: the Linux E2E lane gets a bar spec (Alt, F10, Alt+letter, Escape back to the pane); on Windows, the
         spike's selftest pattern on mami, then task 55's menu entry.
       - Done when: the Windows build shows the in-page bar, no native bar, and task 55's menu-bar checks pass.
    7. [S] **In-page context menus on Windows.**
       - Scope: each of the nine popup commands (`show_file_context_menu`, `show_tab_context_menu`,
         `show_breadcrumb_context_menu`, `show_date_sort_menu`, `show_function_key_bar_context_menu`,
         `show_network_host_context_menu`, `show_parent_row_context_menu`, `show_queue_row_context_menu`,
         `show_viewer_context_menu`) gains an answer-the-rows path where `contextMenus` is `inPage`, mirroring
         `cmdr-listing-wasm/src/context_menu.rs`'s `popup` and `contextClick`; a pick goes back through the same handler
         a native pick reaches today (same `MenuState` context, same `contextMenu` road), ❌ never a parallel one. The
         frontend opens them with step 4's component. The backend half can start in wave 1.
       - Landmines: `show_file_context_menu` is a sync command on the main thread and stores the target in
         `MenuState.context` before the popup: the in-page path must set the same context before the rows go out, and a
         pick must name the menu it came from so a stale context can't act on another row; the "same kind" label is
         computed live by the caller, keep it so; Shift+F10 and the Menu key open the file menu at the cursor row's
         anchor; WebView2's own context menu stays suppressed (`context-menu-ownership.ts`); text fields keep today's
         edit menu (out of scope, David's call later).
       - Tests: Rust tests that each in-page answer matches the native menu's rows; Vitest for opening, picking, and
         dismissal; the demo's context-menu tests keep passing.
       - Done when: every right-click and Shift+F10 menu on the Windows build is in-page, Fluent-styled, and runs the
         same commands as before.
       - **Frontend built** (2026-11-18), unchecked on a Windows machine: each `show_*` wrapper forks in
         `popContextMenu`, the root layout mounts one in-page menu per window, and the Menu key joins Shift+F10. David
         reversed the text-field landmine the same day: on Windows a text field gets the in-page edit menu too (Undo,
         Redo, Cut, Copy, Paste, Select all), so every menu looks the same.
         `apps/desktop/src/lib/context-menu/DETAILS.md` has it and what a WebView2 run checked (2026-11-19).
    8. [S] **The main-menu button and "Show menu bar".**
       - Scope: the main-menu button (GNOME's default, and the KDE and Windows stand-in when the bar is hidden), the
         "Show menu bar" setting and its View row; on Windows a hidden bar shows on Alt or F10 until focus leaves.
       - Landmines: new strings go to the catalog in English with `@key` descriptions (no translations); GNOME's File
         submenu drops Quit.
       - Tests: Vitest for the setting's three layouts; the Linux E2E bar spec gains the hidden case.
       - Done when: each desktop's default matches `menu-model.md` and the setting flips it live.
       - **Built** (2026-11-19), unchecked on real Windows and real GNOME: `appearance.menuBar` (`desktopDefault` |
         `shown` | `hidden`: a literal default, since the analytics defaults manifest has no desktop axis) drawn as one
         switch in Settings > Appearance > Zoom and density, View > Show menu bar (`windows_and_linux`, running
         `view.toggleMenuBar`), the `hiddenMenuBar` convention and `hideableMenuBar` capability, and
         `lib/ui/command-menu/menu-layout.ts` picking bar, hidden bar (Windows: `<MenuBar revealOnly>`), or the
         main-menu button (`MainMenuButton.svelte`, at the end of the right pane's tab row and the viewer's toolbar; F10
         opens it where no bar takes it). `apps/desktop/src/lib/ui/DETAILS.md` § Command menus has it. The viewer's View
         menu carries the same row, so a viewer whose bar is hidden gets it back from its own main-menu button. The
         setting is in the heartbeat's config shape (`CATEGORICAL_STRING_KEYS`). Gap: the browser demo's strip ignores
         the setting, so a Windows or Linux visitor's View > Show menu bar is greyed, as Quit is, until step 11's
         remainder moves the demo onto the app's own bar. Out of scope, written up for later: the bar's titles read
         plain catalog values that several locales took from macOS (de "Ablage", "Darstellung"), so they need per-OS
         values (`docs/notes/windows-port/menu-titles-per-os.md`). macOS unchanged: the row is `windows_and_linux` (the
         macOS bar snapshot didn't move), `hideableMenuBar` is false there, and the demo's Mac shots (main window, View
         menu, the palette searching "menu", Settings > Zoom and density; light and dark) differ by 0 pixels from the
         merge base (`magick compare -metric AE`, 2026-11-19).
    9. [XS] **The text-field gate** lets a bar CLICK on a file-moving command run, and the same command's key still
       doesn't (task 55's entry; `routes/(main)/text-field-gate.ts`). Tests: a Vitest case per road. Done when: both
       cases are pinned.
       - **Built** (2026-11-19): `pick_menu_bar_item(menuId, via)` (`BarPickVia::{Click, Key}`) sends a CLICKED row's
         command down the `BarClick` road (`barClick` dispatch source), which the gate lets through. Every key stays
         refused: the command's own (`keyboard` in the main window, `Relay` from another window's chord) and the menu's
         own keys (Alt or F10 then letters, Alt+letter then a letter, arrows then Enter or Space, in the bar or the
         main-menu button), which the menu stack reports as `key` (review of #1281: Alt, `f`, `l` in a field had opened
         File > Delete). A page's pick also names its window, which Edit rows, Close, and the viewer's rows act on
         instead of focus. Pinned by `text-field-gate.test.ts` § "a bar the page draws", the "Skip confirmation" cases
         in `command-dispatch.characterization.test.ts`, `MenuBar.svelte.test.ts` and `MainMenuButton.svelte.test.ts` (a
         click versus each key), `page_bar.rs`'s road and window tests, and the Linux E2E bar spec (real clicks and
         keys). macOS keeps today's refusal (a native bar can't tell).
    10. [S] **Linux cleanup.**
        - Scope: delete `menu/mnemonic_mask/` and its IPC, `menu-mnemonics-capture.svelte.ts`, and the mask's own
          leftovers in `menu-bar.spec.ts` (step 6 renamed `menu-mnemonic-mask.spec.ts` to it; Settings' title-mnemonic
          warning and `get_menu_bar_title_mnemonics` went with step 6's review: the bar reserves a window's Alt keys
          instead); update `menu/` and `cmdr-menu`'s C+D.md and `lib/ui/DETAILS.md`'s Windows-bar gotcha.
        - Landmines: knip and the IPC binding checks catch leftovers.
        - Tests: the Linux E2E lane (step 6's spec replaces the deleted one).
        - Done when: nothing in the tree references the mask.
        - **Built** (2026-11-19): `menu/mnemonic_mask/`, `set_menu_mnemonics_suspended` and its wrapper,
          `menu-mnemonics-capture.svelte.ts` and its two Settings callers, and the gate's mnemonic push are gone, and so
          are `menu/mnemonics.rs` (the GTK bar's letter allocator, which only the native bar builder called: an identity
          on macOS) and the Linux `gtk` dependency only the mask used. `menu-bar.spec.ts` held no mask leftovers. Kept
          on purpose: `menu_structure.rs` (the macOS viewer bar; its rows mirror `cmdr_menu::page::viewer`'s, and
          merging the two would touch the macOS bar, out of a deletion step's scope) and the AppKit title lookups in
          `display_accelerators.rs` and `context_menu_header.rs` (macOS only: AppKit has no Tauri id, documented in
          `menu/DETAILS.md`). macOS unchanged: the bar builder's labels are byte-identical, and `cmdr-menu`'s bar tests
          pass with no test or data change.
    11. [S] **The demo** draws the visitor's own model with the shared component (macOS bar, Windows bar, or the Linux
        bar, and on Windows the in-page context menus) and picks the demo-only key for the search dialog's new query
        (task 55's entry). Landmines: the demo has no backend, so it keeps `DemoMenuBar`'s wasm answer, which step 2's
        tests hold equal to the desktop's. Tests: the demo's menu tests and `smoke.mjs`. Done when: a Windows visitor
        sees the Windows bar.
        - Status: the bar, the right-click menus, and the look follow the visitor (`new DemoMenuBar(platform)`, the
          visitor's own conventions row, no look pin; `lib/demo/DETAILS.md` § Platform). Still open: the new-query key
          (a fixed-key command can't be seeded, so picking one needs a seam in the shortcut store, David's call).
          Windows and Linux visitors get their OS's title bar above each window (`lib/demo/title-bar.ts`, no
          `overlayTitleBar`, as the desktop keeps the OS's decorations there) and the main volume named as the desktop
          names it (`C:`, `Root`).
        - Paths stay POSIX for every visitor (`/Users/you`, `~/Downloads`, Copy path's `/Users/you/…`). Windows paths
          for a Windows visitor would take: (1) the engine's demo volume rooted at an app path `C:/` with the tree under
          `C:/Users/you` (`cmdr-listing-wasm`'s `tree.rs` takes the root and home from `lib/demo/demo-tree.ts`, whose
          entries are POSIX strings today), (2) nothing in the frontend: its grammar already follows the platform
          (`hostPathFlavor` in `#lib/path/grammar.ts`), so breadcrumbs and Go to folder would follow. Landmine: that
          means a Windows visitor's frontend reads today's POSIX paths in Windows flavor (`/Users/you` has no drive root
          there); the panes work, but no test pins which grammar helper a POSIX path survives, (3) `demoPath`'s `~`
          expansion and every `~`-expanded argument in `listing.ts` against the new home, (4) `getNameRules` answering
          Windows' format (reserved names, forbidden characters) for that visitor, with the write engine refusing what
          the desktop's Windows volume refuses, (5) Copy path's text in Windows spelling (`\`), as the desktop's
          clipboard command writes it, and (6) the volume answers (`volumes.ts`: `path`, `volumeRoot`, favorites) and
          the website suite's path assertions per visitor. Not started: David's call.
        - **Open, the demo side's one remaining item**: the desktop now draws the bar in the page (step 6), so the demo
          should answer that bar's commands from the WASM engine and drop its own strip for a Windows or Linux visitor.
          It takes: (1) `get_menu_bar`, `get_viewer_menu_bar`, and `pick_menu_bar_item` answered from `DemoMenuBar` in
          `lib/demo/answers.ts` (today `getMenuBar` answers an empty bar so the app's bar draws nothing) and taken off
          `coverage.ts`'s `IN_PAGE_MENUS`, plus a `menu-bar-changed` emit when a row's state moves; (2) the demo's own
          strip (`lib/demo/menu-bar.svelte.ts`) kept for a macOS visitor only; (3) the demo's menu tests and `smoke.mjs`
          moved onto the app's bar, and View > Show menu bar live there (today greyed). The macOS demo stays as is.
    12. [XS] **Delete `tools/menu-spike/`** once steps 6 and 7 and task 67 have taken what they need (list above): the
        folder, its line in `AGENTS.md`'s `tools/` list, and the README pointer in `menu-spike.md` (the note stays as
        the decision's record). Done when: no doc links the folder.
        - **Built** (2026-11-19): the folder (its own `Cargo.lock` and `osv-scanner.toml` with it) and its `AGENTS.md`
          line are gone; `menu-spike.md`, `keymenu.rs`, and `src-tauri/DETAILS.md` point at the source in git history.
    - **Task 63's open items**: (1) the real-Windows check of steps 3–9 (the bar, Alt and F10, `SC_KEYMENU`, the in-page
      context menus, the hidden bar; task 55's menu entry, on mami or a laptop); (2) David's design review of the Fluent
      look against Notepad and File Explorer (step 3's done-when); (3) the demo remainder (step 11, above); (4) per-OS
      menu titles (`docs/notes/windows-port/menu-titles-per-os.md`).
    - Reused unchanged: the bar data, ids, labels, enabled fold, sort checks, every context menu's rows, and the
      snapshots. QA: task 55 (keys), task 64 and David (Narrator, NVDA, JAWS, and Orca read the bar and the menus as
      menus; high contrast).
64. [M] Windows. Screen reader parity: a JAWS, NVDA, and Narrator pass (task 59's `screenReaders`) over the main flows
    (panes, dialogs, Settings, the viewer, operations in progress), fixing what the ARIA work for VoiceOver doesn't
    carry through WebView2 and UI Automation, plus the Windows contrast themes (`forced-colors`). The menus arrive with
    their roles and `forced-colors` styles from task 63 (its steps 3 and 4); this pass checks them with each reader
    rather than building them, and David runs the high-contrast and Narrator part himself. The findings become part of
    task 55's checklist. Linux: the same pass with Orca, later.
65. [S] Windows. System surfaces: native file pickers, toast notifications under Cmdr's app identity, a dark title bar
    in dark mode, and Mica or another system backdrop on Windows 11. Linux: portals for pickers and notifications. Needs
    52 for the app identity.
66. [S] Windows, David. A native-feel checklist for task 55: per surface, what "native" means on Windows with reference
    screenshots from Windows' own apps, so the QA pass has something to check against. An agent drafts it; David owns
    it.

### Later, not in the first Windows release

- OCR via `Windows.Media.Ocr`, CLIP on ONNX Runtime + DirectML.
- Explorer shell-extension verbs in Cmdr's own context menu (`IContextMenu`), preview handlers, ARM64 builds.
