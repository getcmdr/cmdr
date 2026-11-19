# The trait flip: handoff to the tier agents

Tiers 0 and 1 of #1225's milestone 4 (`docs/specs/app-path-plan.md` § "4. Flip the trait") are done: the `Volume` trait
takes and returns app paths, `cmdr-fs`, `cmdr-archive`, and the 18 tier-1 crates check, pass clippy and their tests, and
the codemod that converts the rest is `scripts/app-path-flip/README.md` (read it first: usage, rules, residue shapes).
This note is what a tier agent needs on top of it.

## The base

- **Branch**: `worktree-agent-a4073ece33b857976` (tier 1 merged onto tier 0, plus the codemod's eager-helper and
  `from_ref` rules). Every tier-2 agent branches from its tip; the lead merges a tier before the next one starts, and
  later tiers branch from that merge.
- **Your own scratchpad subfolder** for everything you build or write outside the tree: the codemod binary, its
  `-report` JSON, `zigcc.sh`, and the cross-check `CARGO_TARGET_DIR`s. ❌ Never `/tmp` or a path another agent might
  pick: parallel agents overwrite each other's reports and target dirs.
- **Expected state**: the workspace doesn't compile as a whole until every tier is done. Build only your crates (`-p`),
  ❌ never the workspace: other agents share the disk and CPU. A crate outside your set failing is expected.
- On it: artifact 1 (`refactor(cmdr-fs): the Volume trait takes and returns app paths…`), artifact 2
  (`feat(scripts): app-path-flip…`), and the worked example, `cmdr-archive`, as four commits (codemod pass 1, its
  residue, codemod pass 2, and the guard test plus the bridge counts). Copy its shape.

## The loop, per crate

Flip your crates in dependency order (a crate after every crate it depends on, dev-deps included). Foreground only.

1. `go -C scripts/app-path-flip run . -p <crate> -report <scratch>/<crate>-pass1.json` (build it once with
   `go -C scripts/app-path-flip build -o <scratch>/flip .` and run the binary if you'll call it often). Commit all it
   changed: `refactor(<crate>): flip to app-path volume calls (codemod pass 1) (refs #1225)`, the body giving the rule
   counts from its summary. ❌ No hand edit in this commit: the audit re-runs the codemod and diffs.
2. Fix its residue list by hand (README § "Passes, residue, and commits" has each shape and its fix). Commit:
   `refactor(<crate>): app-path flip residue (pass 1) (refs #1225)`, one bullet per fix saying why the codemod couldn't.
   ❌ No `assert*!` line, no expected value, no logic: a fix the bridge can't express is a stop-and-ask.
3. Run the codemod again. A crate that dev-depends on itself only builds its tests once its lib checks, so the second
   run usually converts the tests: that's pass 2, back to step 1. Stop when a run reports `checks: true` and edits
   nothing.
4. Run it once more per feature set CI builds the crate with (`-cargo-args "--features testing"`,
   `-cargo-args "--all-features"`, and alone each feature that gates code without `testing`, like `--features golden`:
   the lib phase sees it only there); any edits are another pass. ❗ `-cargo-args` is ONE quoted string, and never
   repeat `--all-targets` in it (the codemod already passes it): cargo then fails before rustc runs, and the run reads
   `0 diagnostics, 0 edits` with `checks: false`, which looks like a clean no-op. Only `checks: true` is green.
5. Cross-check the `cfg` code this Mac never compiles, and fix what it finds as residue (README § residue, last bullet).
   The Mac recipe: a `zigcc.sh` in your scratchpad that drops cc-rs's `--target=…` argument and runs
   `~/.local/share/mise/installs/zig/0.16.0/bin/zig cc -target ${ZIGTARGET:-x86_64-windows-gnu} "$@"`, then:
   - `CC_x86_64_pc_windows_gnu=<scratch>/zigcc.sh AWS_LC_SYS_PREBUILT_NASM=1 CARGO_TARGET_DIR=<scratch>/target-win cargo check -p <crate> --all-targets --target x86_64-pc-windows-gnu`
   - `ZIGTARGET=x86_64-linux-gnu CC_x86_64_unknown_linux_gnu=<scratch>/zigcc.sh CARGO_TARGET_DIR=<scratch>/target-linux cargo check -p <crate> --all-targets --target x86_64-unknown-linux-gnu`
   - A crate that builds for the browser (`cmdr-fs`, `cmdr-archive`, the listing crates):
     `cargo check -p <crate> --lib --target wasm32-unknown-unknown`.
   - More crates than `cmdr-fs` pull C dependencies; `docs/notes/windows-port/build-progress.md` lists what else the
     Windows cross-check needs (`CXX_…`, `AR_…`, `numkong`). What tier 1's merge ran on every crate: `CC_`, `CXX_`
     (`zig c++`), and `AR_` (`zig ar`) for the target triple, `NK_TARGET_<ISA>=0` for numkong's 11 x86 ISAs (`ALDER`,
     `DIAMOND`, `GENOA`, `GRANITEAMX`, `HASWELL`, `ICELAKE`, `SAPPHIRE`, `SAPPHIREAMX`, `SIERRA`, `SKYLAKE`, `TURIN`),
     and `AWS_LC_SYS_PREBUILT_NASM=1`.
6. Verify: `cargo clippy -p <crate> --all-targets -- -D warnings` (and per feature set), `cargo nextest run -p <crate>`
   with the pins unchanged (a pin that goes red is a stop-and-tell-the-lead, ❌ never an edit to it), and
   `pnpm check app-path-std-bridge file-length`.
7. Record the bridge counts in the last residue commit: `app-path-std-bridge` fails listing every new bridge call; add
   each of your files to `scripts/check/checks/app-path-std-bridge-allowlist.json` § `files` with its count (the plan's
   "the allowlist records the counts": this one-time seeding is the plan's, not a loosening). The log the check names
   has every site; count per file. A `file-length` failure from the preludes: bump the entry with a reason saying so
   (`cmdr-fs`'s `volume_impl.rs` and `conformance.rs` are the model).

## When a rule doesn't apply

- Residue the README lists: fix by hand, in the bridge's terms (`as_std_legacy`, `legacy_unchecked`, `BridgedPath`,
  `.app()`), as `cmdr-archive`'s residue commits do.
- The same residue shape at three sites or more: tell the lead with the sites rather than hand-fixing them all. A rule
  change goes into the codemod on the base branch, and every crate already flipped re-runs it.
- A fix that would change what a body computes, or a pin that would need editing: stop and tell the lead with the
  evidence.

## Known gaps

- **No silent typed arithmetic**: the app path's methods that `Path` also has (`parent`, `file_name`, `extension`,
  `file_stem`, `with_file_name`, `join`, `starts_with`, `strip_prefix`, `relative_to`, `rebase`) live on the
  `cmdr_fs::app_path::PathArithmetic` trait, so `x.file_name().is_some()` on a typed `x` doesn't compile (E0599) and the
  codemod drops its receiver to `as_std_legacy()`. ❌ Never import `PathArithmetic` during the flip: importing it is
  adopting the typed answers, which is each crate's own conversion milestone, where it flips its pins on purpose.
  `cmdr-archive`'s `a_typed_root_has_no_path_shaped_arithmetic_in_reach` fails to compile if the trait is imported or
  the methods go back on the inherent impl. The one exception (the lead's call, tier 4):
  `fuzz/fuzz_targets/app_path_native.rs` imports it, since it checks the app path module's own properties (a typed
  `file_name`), as that module's tests do.
- **Line numbers in the residue list** are from before the final `cargo fmt`; `cargo check` gives the current ones.
- **Helpers a trait method tail-calls**: a lazy one (an `async fn`, or a body that is only
  `Box::pin(async move { … })`), nested or in another crate, gets the call moved into the future (README § "Lazy
  helper"); an eager one (a statement before its `Box::pin`), in this crate, takes the typed path and the prelude
  itself, down a forwarding chain (README § "Eager helper"). Moving an eager call into an `async move` would delay its
  work to the first poll, so that never happens. A helper whose future borrows the param stays residue.
- **Tier 2's `cmdr-operations`, by shape** (counted on the tier-1 merge, 567 files, 305 of them tests): 109
  `impl Volume` and 2 `ScanSource`/`PatchSource` impls; of the non-impl helpers taking a `&Path`/`&[PathBuf]`, 208 are
  `async fn` and 7 are body-only `Box::pin(async move { … })` (lazy), and none is eager, so the eager rule is mainly for
  the app; 182 `slice::from_ref(&x)` (most feed the engine's own `&[PathBuf]` APIs, which stay std, so the `from_ref`
  row fires only where a slot is a `Volume` batch); 381 `TestDir` uses, 2,219 `Path::new("…")` and 820
  `PathBuf::from("…")` literals, 535 `assert_eq!`/`assert_ne!` lines naming a path-ish local, 53 borrowed-return methods
  (`root`, `stands_in_for_own_disk`, `local_backing_file`), 175 OS `cfg` lines (cross-check!), 6 `join!`s (E0716 hoist),
  6 `macro_rules!`, 3 `AsRef<Path>` generics (residue).
- **Trees flipped before the import fix**: run `go -C scripts/app-path-flip run . -audit-imports <crate dir>` and lift
  any hit that took an item's `#[cfg]` (README § "Imports").
- **`app_path!` takes only canonical literals**: relative literals (`Path::new("dir/file.txt")`, an archive's inner
  paths) and ones with a trailing `/` ride `legacy_unchecked`, as the plan says.
- **Imports**: one `use` line per name (rustfmt doesn't merge them), so a file can gain
  `use cmdr_fs::app_path::AppPath;` beside `use cmdr_fs::app_path::AppPathBuf;`.
- **Flipped beside the trait**: `ScanSource::scan_stat`/`scan_list` and `PatchSource::patch_stat` take `&'a AppPath`.
  Their impls forwarded a std path into a typed `Volume` read (`self.get_metadata(&AppPath::legacy_unchecked(path))`), a
  `Cow` temporary the returned future borrows (E0515), and an `async move` around it would delay work some backends do
  before their future (git's reads). Typed, an impl hands the path straight on. The codemod treats these impls like a
  `Volume` impl's (signature, prelude, lazy helper), and `scan_walk`/`patch_mutation` bridge where they call back,
  inside their own futures.
- **Not flipped beside the trait**: `PatchSource::patch_display_path` and `MakesDirectories` (sync, or `&str`: no future
  borrows a path), `tally_by_listing`, `ScanBoundary::subtree`, `fold_batch`, `root_anchored`, the `conformance`
  helpers: still `std::path`, so a backend's calls to them stay as they are. `ScanConflict` and every DTO keep
  `String`/`PathBuf` (the plan's rule for types that serialize).

## Tiers and crates

From the plan; each agent owns whole crates, on its own branch off the tier before.

- Tier 0, the lead: `cmdr-fs` (done, with `cmdr-archive` as the worked example).
- Tier 1, four in parallel (done, merged in `worktree-agent-a4073ece33b857976`): (A) `cmdr-local`, `cmdr-git`,
  `cmdr-viewer`, `cmdr-media-meta`, `cmdr-search`; (B) `cmdr-smb`, `cmdr-sftp`, `cmdr-webdav`, `cmdr-s3`, `cmdr-nfs`,
  `cmdr-adb`, `cmdr-mtp`; (C) `cmdr-index`, `index-query`; (D) `cmdr-listing`, `cmdr-brief-columns`,
  `cmdr-operation-log`, `operation-log-dump`. On the merge, every crate re-ran the codemod (default, `testing`,
  `--all-features`) as a no-op, is clippy-clean (default and all features), checks for Windows and Linux (zig) and, for
  `cmdr-listing`, `cmdr-viewer`, and `cmdr-brief-columns`, wasm32, and passes `-audit-imports` (its six hits are
  deliberate `cfg`-gated imports). Nextest, passed of run (skipped are the Docker integration suites): `cmdr-local` 150
  (2 skipped), `cmdr-git` 131 (9), `cmdr-viewer` 396 (1), `cmdr-media-meta` 92, `cmdr-search` 315, `cmdr-smb` 239 (188),
  `cmdr-sftp` 129 (101), `cmdr-webdav` 118 (64), `cmdr-s3` 400 (102), `cmdr-nfs` 167 (47), `cmdr-adb` 167, `cmdr-mtp`
  198 (1), `cmdr-index` 1,999 (44), `cmdr-listing` 384, `cmdr-brief-columns` 31, `cmdr-operation-log` 173 (2);
  `index-query` and `operation-log-dump` have no tests.
- Tier 2: (E) `cmdr-operations` (108 impls; its tests in subagent batches by directory, sequential on one branch) and
  (F) `cmdr-restic`, in parallel. (E) is done on `worktree-agent-a6eb35c648eaa8989`: four codemod passes (the fourth for
  the Linux target, `non_utf8_name_tests.rs`), residue 16 + 3 + 7 (the golden scenarios for wasm) sites, no-op re-runs
  for default, `testing`, `golden`, and `--all-features`, Windows and Linux (zig) checks with `--all-features`, wasm32
  `--lib --features golden`, clippy clean (default and all features), nextest 2,594 passed (30 skipped), 981 bridge
  calls over 205 files. New codemod rules on that branch: the `select!` and match-arm hoist, the closure hoist, fully
  qualified macro signatures, the nested prelude, and the unused prelude. The literal rule's `app_path!` in code a
  feature gates without `cmdr-fs/testing` (`cmdr-operations`'s `golden`) compiled only because `--all-targets` unifies
  `testing` in; tier 3 added the codemod's lib phase (README § "Lib phase"), which bridges those, and the seven golden
  sites tier 2 fixed by hand are its output now (`-cargo-args "--features golden"`).
- Tier 3: (G) `cmdr-multi-rename`, then `cmdr-listing-wasm`. Done: `cmdr-multi-rename` needed no edit (its volumes reach
  only `cmdr-operations`' std APIs); `cmdr-listing-wasm` took two codemod passes, one residue site, and 11 bridge calls
  over six files. Both: no-op re-runs (default, `testing`/`--all-features`), Windows and Linux (zig), wasm32 (`--lib`,
  plus `--tests` for `cmdr-listing-wasm`), clippy clean, nextest 143 and 114 passed. New codemod rules: the lib phase
  and the typed-first assert argument.
- Tier 4: (H) the app, `cmdr` (33 impls, 629 test call sites), one agent, on `worktree-agent-aac5515068e4d32d3`. Done:
  - **Codemod passes** (no hand edit in any): pass 1 (default, 7 rounds, about 1,100 edits over 140 files), pass 2 (the
    clippy phase, 4), pass 3 (`--features virtual-mtp`, 32: its E2E paths), pass 4 (`--target x86_64-pc-windows-gnu`, 1:
    an assert in `volumes/platform/windows/mod.rs`). No-op re-runs: default, each of `playwright-e2e`, `virtual-mtp`,
    `smb-e2e`, `mimalloc` alone, and `--all-features` on the Mac; default and `virtual-mtp,smb-e2e,mimalloc` for
    Windows; default and `--all-features` for Linux.
  - **The Linux pass runs in the Docker image**, ❌ not zig: the app links GTK (`glib-sys` needs pkg-config and a Linux
    sysroot). The `cmdr-rust-linux` image has Go, so the codemod runs there as is
    (`go -C scripts/app-path-flip run . -p cmdr …`, the repo at `/repo`, a target volume of its own). Windows needs a
    stand-in `x86_64-w64-mingw32-windres` on `PATH` for the build script (`build-progress.md` § the app).
  - **New codemod rules** (each its own commit, pinned by `rules_app_test.go`): the plain-comparison operands and the
    generic std bound (`rules_operands.go`, both after the prelude), and the borrowed-return field retype preferring the
    impl's own file (the app's two `WaitsForCancel` doubles: the first match had retyped the other file's field).
  - **Residue, by hand, three sites** (`0047693086`, replay these on regeneration): `location/facts.rs` `note_live_root`
    (`let root_std = volume.root().as_std_legacy();` before `to_string_lossy`, E0716); `network/smb_pane_upgrade.rs`
    (`let root_std = volume.as_ref().map(|v| v.root().as_std_legacy());` before `plan`, passed as
    `root_std.as_deref()`); `search/execute/tests/live_drive.rs` `drive_with_a_covered(data: &Path)` (the codemod's
    `Path` import made the qualified `&std::path::Path` warn `unused_qualifications`).
  - **Outside the flip**: `examples/docker_smb_test.rs` spreads `..Default::default()` into its `ClientConfig` (E0063
    since `smb2` 0.43; it kept `--features smb-e2e` and `--all-features` from checking at all).
  - **Bridge counts**: 578 calls over 131 app files in `app-path-std-bridge-allowlist.json`; `file-length` takes
    `file_system/volume/manager/roots.rs` at 810 lines with a reason.
  - **Verified**: clippy `-D warnings` clean (Mac workspace via `pnpm check clippy`, app default and `--all-features`;
    Linux in Docker), the Windows workspace checks (zig, `--exclude cmdr-fsevent-stream`, the macOS-only member),
    nextest 6,805 of 6,805 (481 skipped), `-audit-imports` (five hits, all hand-written `cfg`-gated imports from before
    the flip), `pnpm check --fast`, `app-path-std-bridge`, `file-length`.
  - **Fixed beside it**: Windows with `playwright-e2e` had no `platform_backends` arm in `cmdr-index`
    (`media_index/scheduler/lifecycle.rs`); the fix is text-identical to PR #1270's. The fuzz target
    `app_path_native.rs` imports `PathArithmetic` (the exception above), so `pnpm check fuzz-compile` passes.

## The worked example, in numbers

`cmdr-archive` (one `impl Volume`, 14,000 lines): pass 1 made 47 edits (17 signatures, 10 preludes, 3 borrowed-return, 1
constructor wrap, 3 field unwraps, 2 async-helper moves, 6 bridged values, 5 imports) and left 2 residue sites (one
private sync helper, `open_stream`); pass 2 made 90 (80 bridged test arguments, 3 `app_path!` literals, 2 owned bridges,
1 assert operand, 1 typed receiver dropped to std, 3 imports) and left none. Residue in all: 2 sites, 8 lines. Result:
100 bridge calls over eight files, clippy clean, 241 of 241 tests (the guard test included).
