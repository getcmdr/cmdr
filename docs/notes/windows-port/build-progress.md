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
- **Next blockers**: `cmdr-local` (46 errors: `LocalPosixVolume` is POSIX through and through; whether it grows Windows
  arms or gets a `LocalWindowsVolume` sibling is task 35's call) blocks `cmdr-operations`, `cmdr-multi-rename`,
  `cmdr-restic`, `cmdr-listing-wasm`, and the app. `numkong` and `usearch` build fine under MSVC; `cmdr-index` (and so
  `index-query`) fails in its own code with 11 errors, mostly its SMB and MTP index transports, gated to macOS and Linux
  while `indexing/handle/mod.rs` calls them unconditionally, plus the `platform_case` collation's missing Windows arm
  and `scanner/refusal.rs` comparing `libc::EPERM`.
