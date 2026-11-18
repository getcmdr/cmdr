# The trait flip: handoff to the tier agents

Tier 0 of #1225's milestone 4 (`docs/specs/app-path-plan.md` § "4. Flip the trait") is done: the `Volume` trait takes
and returns app paths, `cmdr-fs` and `cmdr-archive` check, pass clippy and their tests, and the codemod that converts
the rest is `scripts/app-path-flip/README.md` (read it first: usage, rules, residue shapes). This note is what a tier
agent needs on top of it.

## The base

- **Branch**: `worktree-agent-a6403541e656ed355` (tier 0). Every tier-1 agent branches from its tip; the lead merges a
  tier before the next one starts, and later tiers branch from that merge.
- **Expected state**: the workspace doesn't compile as a whole until every tier is done. Build only your crates (`-p`),
  ❌ never the workspace: other agents share the disk and CPU. A crate outside your set failing is expected.
- On it: artifact 1 (`refactor(cmdr-fs): the Volume trait takes and returns app paths…`), artifact 2
  (`feat(scripts): app-path-flip…`), and the worked example, `cmdr-archive`, as four commits (codemod pass 1, its
  residue, codemod pass 2, and the guard test plus the bridge counts). Copy its shape.

## The loop, per crate

Flip your crates in dependency order (a crate after every crate it depends on, dev-deps included). Foreground only.

1. `go -C scripts/app-path-flip run . -p <crate> -report /tmp/<crate>-pass1.json` (build it once with
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
   `-cargo-args "--all-features"`); any edits are another pass.
5. Cross-check the `cfg` code this Mac never compiles, and fix what it finds as residue (README § residue, last bullet).
   The Mac recipe: a `zigcc.sh` in your scratchpad that drops cc-rs's `--target=…` argument and runs
   `~/.local/share/mise/installs/zig/0.16.0/bin/zig cc -target ${ZIGTARGET:-x86_64-windows-gnu} "$@"`, then:
   - `CC_x86_64_pc_windows_gnu=<scratch>/zigcc.sh AWS_LC_SYS_PREBUILT_NASM=1 CARGO_TARGET_DIR=<scratch>/target-win cargo check -p <crate> --all-targets --target x86_64-pc-windows-gnu`
   - `ZIGTARGET=x86_64-linux-gnu CC_x86_64_unknown_linux_gnu=<scratch>/zigcc.sh CARGO_TARGET_DIR=<scratch>/target-linux cargo check -p <crate> --all-targets --target x86_64-unknown-linux-gnu`
   - A crate that builds for the browser (`cmdr-fs`, `cmdr-archive`, the listing crates):
     `cargo check -p <crate> --lib --target wasm32-unknown-unknown`.
   - More crates than `cmdr-fs` pull C dependencies; `docs/notes/windows-port/build-progress.md` lists what else the
     Windows cross-check needs (`CXX_…`, `AR_…`, `numkong`).
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
  the methods go back on the inherent impl.
- **Line numbers in the residue list** are from before the final `cargo fmt`; `cargo check` gives the current ones.
- **A helper that does work before its future starts** (`fn … -> Pin<Box<…>>` with a statement before its `Box::pin`)
  stays residue: moving its call into an `async move` would delay that work to the first poll. A lazy one (an
  `async fn`, or a body that is only `Box::pin(async move { … })`), nested or in another crate, is the codemod's (README
  § "Lazy helper"). Of the reported sites: `cmdr-smb`'s five (`scan_for_copy_impl`, `scan_for_copy_batch_impl`,
  `scan_for_conflicts_impl`, `write_from_stream_impl`, `continue_write_from_stream_impl`) and `cmdr-sftp`'s
  `noting`/`scan_walk`/`patch_mutation` trees are lazy; of `cmdr-local`'s nine only
  `scan_for_copy_batch_with_boundary_impl` is (the rest call `self.resolve(path)` first, or `stream_into`, which does).
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
- Tier 1, four in parallel: (A) `cmdr-local`, `cmdr-git`, `cmdr-viewer`, `cmdr-media-meta`, `cmdr-search`
  (`cmdr-archive` is done); (B) `cmdr-smb`, `cmdr-sftp`, `cmdr-webdav`, `cmdr-s3`, `cmdr-nfs`, `cmdr-adb`, `cmdr-mtp`;
  (C) `cmdr-index`, `index-query`; (D) `cmdr-listing`, `cmdr-brief-columns`, `cmdr-operation-log`, `operation-log-dump`.
- Tier 2: (E) `cmdr-operations` (108 impls; its tests in subagent batches by directory, sequential on one branch) and
  (F) `cmdr-restic`, in parallel.
- Tier 3: (G) `cmdr-multi-rename`, then `cmdr-listing-wasm`.
- Tier 4: (H) the app, `cmdr` (33 impls, 629 test call sites), one agent with subagents by directory.

## The worked example, in numbers

`cmdr-archive` (one `impl Volume`, 14,000 lines): pass 1 made 47 edits (17 signatures, 10 preludes, 3 borrowed-return, 1
constructor wrap, 3 field unwraps, 2 async-helper moves, 6 bridged values, 5 imports) and left 2 residue sites (one
private sync helper, `open_stream`); pass 2 made 90 (80 bridged test arguments, 3 `app_path!` literals, 2 owned bridges,
1 assert operand, 1 typed receiver dropped to std, 3 imports) and left none. Residue in all: 2 sites, 8 lines. Result:
100 bridge calls over eight files, clippy clean, 241 of 241 tests (the guard test included).
