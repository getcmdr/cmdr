# Windows build progress

A running log of the Windows build track (#942, `docs/specs/windows-port-plan.md` § "Windows build state"), one entry
per milestone, newest last. The machine is the Windows 10 22H2 builder (`ssh mami`, MSVC, `infra/mami-laptop/CLAUDE.md`
in the infra repo); the fast loop is `cargo check --target x86_64-pc-windows-gnu` on a Mac with zig as the C compiler
and `AWS_LC_SYS_PREBUILT_NASM=1`.

- **2026-11-18, baseline** (`52f7ec04ea`): the seven must-compile members (`cmdr-collation`, `cmdr-http`, `cmdr-menu`,
  `cmdr-platform-cfg`, `cmdr-recents`, `cmdr-sort`, `cmdr-update-trust`) build and pass `cargo test` on the builder, 141
  tests, in 1m 58s cold. `cmdr-fs` failed its own compile (54 errors natively, matching the CI break report) and blocked
  the other 24.
- **2026-11-18, `cmdr-fs` compiles** (`40fb822f24`): off the pending list (24 left). `cargo test -p cmdr-fs` on the
  builder: 782 passed, 1 failed, 27 ignored. The runs found four real Windows gaps: app paths go through `std::path`
  (#1225, 24 tests ignored on Windows), deleting a cached SQLite database fails (#1226, one test), and the durable sync
  reopened landed files read-only, which `FlushFileBuffers` refuses (the one failure, fixed in the next entry); the test
  helpers had the same read-only habit.
