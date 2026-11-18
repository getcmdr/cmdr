# Windows build progress

A running log of the Windows build track (#942, `docs/specs/windows-port-plan.md` § "Windows build state"), one entry
per milestone, newest last. The machine is the Windows 10 22H2 builder (`ssh mami`, MSVC; its setup lives in the infra
repo's mami-laptop notes); the fast loop is `cargo check --target x86_64-pc-windows-gnu` on a Mac with zig as the C
compiler and `AWS_LC_SYS_PREBUILT_NASM=1`: `CC_x86_64_pc_windows_gnu` names a script that drops cc-rs's `--target` flag
and runs `zig cc -target x86_64-windows-gnu`, and `AR_x86_64_pc_windows_gnu` runs `zig ar`. Two things it can't judge:
the app's build script (no `windres`) and `numkong`'s AVX-512 kernels, which zig's clang refuses.

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
