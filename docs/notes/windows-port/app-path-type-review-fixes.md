# The typed app path: integration and the second review's fixes

The integration of #1225's milestone 1 (behavior pins) and milestones 2–3 (the type) onto `main`, and the fixes for the
type's second adversarial review, on one branch (2026-11-18). The plan is `docs/specs/app-path-plan.md`; the type's
durable design is `crates/cmdr-fs/DETAILS.md` § "App paths". This note records what each finding became, so the next
milestone (the trait flip) starts from a known state.

## Merge conflicts and how they were settled

- **`volumes/platform/windows.rs`**: `main` split it into `windows/` (the drive-identity work, #942); the type branch
  had moved `AppPath` out of `path_grammar`. The new `windows/mod.rs` speaks the text grammar's `native_to_app`,
  `path_kind`, and `root_of` (no behavior change).
- **`path_grammar_cases.json`**: one textual conflict (the `about` text); both kept. The pins' `arithmetic` rows and the
  type's `escapes` rows merged cleanly.
- **`build-progress.md`, `crates/cmdr-fs/DETAILS.md`**: both entries kept.
- **`path_arithmetic_pin_test.rs`**: written against the old `path_grammar::AppPath`; it now asserts every row's
  `target` against the real type. Four rows disagreed:
  - The scheme root: the pins proposed `scheme://authority` (no `/`); the type spells it `scheme://authority/`. Kept the
    type's: every root ends in `/`, `RemoteRoot::app_root` of a server-rooted volume is already `sftp://u@h:22/`, and
    `Eq` holds both spellings equal. Three rows carry a `note`; the plan's milestone 1 record says why.
  - `join("C:/x", "a\\b")`: the type refuses `Backslash` (the name is plain; the drive can't hold it), not
    `NotAChildName`.
- **The trailing-slash delete pin**: `main`'s `80dd3735cc` fixed the data loss it pinned, so the cell went red for the
  right reason; it now pins the fixed answer (the link goes, the target stays).

## Per finding

- **N1, N8 (trailing dots; the lead's decision 1)**: `from_native` keeps every name literal in both arms;
  `from_user_input` is split into `from_external_text` (machine-handed text: MCP, deep links, the CLI; literal) and
  `from_person_typed` (the address bar and go-to dialogs only: Explorer's trim, `BlankName`). Cells from the reviewer's
  inputs, a property over generated plain paths, a fuzz assertion with its mutant and seed. Commit `5d01561df7`.
- **N4 (equality; decision 3)**: the hybrid. Host rules only for text that host's OS opens; on Windows a POSIX-kind `/x`
  reads with the app's `/`-only grammar, so `/d/a\b != /d/a/b`; macOS and Linux unchanged. Properties under both hosts'
  rules on every OS (equivalence, antisymmetry, transitivity, agreeing hashes). `12aa287767`.
- **N6 (debug-only safety; decision 5)**: arithmetic on a bridge value is right in release builds. `lenient.rs` answers
  slices as `Path` would and reads `join`, `strip_prefix`, the volume-root matches, and `rebase` through the canonical
  twin; `strip_prefix` answers `Option<Cow<RelativeAppPath>>` (new `RelativeAppPathBuf`), borrowed for canonical values.
  A value with no canonical twin (a `..`, no root) joins with a typed refusal and sits under nothing. The
  `debug_assert!` is gone. A Unix property holds every bridge value to `Path`'s `parent`, `file_name`, and
  `starts_with`. `a52b1ed588`.
- **N2, N3 (the gate; decision 2)**: a text scan can't be sound (it can't see a `let`, `.into()`, an `impl AsRef<Path>`
  parameter, or a `format!` join), so it's labeled a tripwire. The sound mechanism is the type's: no app-path type
  implements `AsRef<Path>`/`AsRef<OsStr>`/`AsRef<str>`/`Borrow<str>`/`Deref<Target = str>`/`Into<…>` (compile-asserted
  now); milestone 10f first types every carrier, then bans `as_str`/`into_string`/`Display` by type in `clippy.toml`.
  The tripwire gained `pathFromBareText` (973 sites seeded, a coarse floor) and catches a producer as a call, a function
  pointer, or a bare `escape_bytes(`. For N3, `from_native_unicode` / `from_native_os_unicode` cross without making an
  escape (typed `NotUnicode`), which milestones 9, 10a, and 10b use; the plan's 9, 10a, 10b, and 10f now agree with the
  check. `d5d5ac6359`.
- **N5 (names; decision 4)**: `AppName::new` is `AppName::from_app_spelling`, its doc saying an `OsStr`'s `to_str()`
  isn't app spelling; the check refuses OS text (`to_str()`, `to_string_lossy()`, `.display()`) handed to any
  app-spelling door on one line, tests included. `d5d5ac6359`.
- **N7, L6 (old rows; decision 6)**: `ESCAPES_LIVE` (false until 10f), `AppPathBuf::from_literal_store`, and the
  operation log's `meta.escape_mark_last_literal_op_id`, stamped once at the first open with escapes live;
  `stored_path_reading` answers `Literal` at or below it. Every other store was checked and needs none (tabs, favorites,
  recents, go-to-path only navigate; the indexes are caches that converge); the plan says why per store. `13cda08210`.
- **L-a**: covered by N6 (either argument is read as its canonical twin), pinned in `8dce388026`.
- **L-b**: the fuzz round trips compare text.
- **L-c**: unchanged by design: each consumer's Windows task writes the words for a `NotPlainSafe` refusal (plan
  question 14).
- **L-d (L5)**: filed as #1244 (Impact Low, Size XS).
- **L-e**: `from_app_spelling`'s doc and name cover the typed-text case; a rename handler goes through
  `AppNameBuf::from_typed`. No scan can see an IPC string reaching it; the type-level ban at 10f does.

## Disagreements and residuals

- **`legacy_unchecked` isn't a producer**, against review 2's list: counting it would block milestone 4 (the flip is all
  bridge calls), its escapes go back to the OS through `as_std_legacy`, and its text exits fall under 10f's ban.
- **The CLI's path arguments are literal** (`from_external_text`): the lead's decision named only the address bar and
  the go-to dialogs as person-typed. A shell argument is often scripted or pasted.
- **A full `//server/share/x` on macOS and Linux** still reads as UNC in arithmetic while `Eq` reads it as std does
  (`/server/share/x`): canonical UNC values exist there only in tests (`parse(_, Windows)`, how Linux CI runs the
  Windows arm), and the text can't say which it is. No std join makes a bridge value of that shape; a sloppy
  `format!("{dir}/{name}")` with `dir == "/"` makes `//name`, which has no share and reads as std does
  (`lenient::is_canonical` documents it; the property excludes the shape).

## Checks

Recorded in the lead's report and the plan's milestone 3 entry; the Windows builder's run is in `build-progress.md`.
