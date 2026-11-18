# app-path-flip

The compiler-driven codemod that moves a crate onto the `Volume` trait's app-path signatures
(`docs/specs/app-path-plan.md` § "4. Flip the trait"). It runs `cargo check` on one crate, rewrites only the spans rustc
points at, by error code and type pair, and repeats until the crate checks or no rule applies. Then it runs
`cargo clippy` the same way, applying clippy's own fix for the lints a rewrite can cause, and `cargo fmt -p` on the
crate. Every rewrite is type-preserving through the lossless, non-normalizing bridge (`AppPath::legacy_unchecked`,
`as_std_legacy`, `BridgedPath`), so no value changes in transit and every body keeps its `std::path` arithmetic.

It goes when the trait flip has landed and no branch from before it is still in flight (the plan's milestone 5).

## Running it

From the repo root, one crate at a time, foreground:

```sh
go -C scripts/app-path-flip run . -p <crate> [-cargo-args "--features testing"] [-report /tmp/report.json]
```

- `-p` repeats; crates flip in the order given. Flip a crate only after every crate it depends on (dev-deps included).
- `-cargo-args` is passed to every `cargo check` and `cargo clippy` (space-separated; `--all-targets` is already there,
  and passing it again fails cargo, which the run reports as 0 diagnostics and `checks: false`). Run once with the
  default features, then again with every feature combination CI builds for the crate (`--features testing`,
  `--all-features`), since code behind a feature is invisible otherwise. A re-run over a converted tree is a no-op.
- `-report` writes the rule counts, per-file edit counts, and the residue as JSON. Stdout has the same summary.
- `-no-fmt` skips the final `cargo fmt -p`. `-max-rounds` (default 40) caps each phase.
- It touches only the crate's own files (diagnostics elsewhere are ignored) and only code rustc complains about.

**Deterministic**: the rules read only rustc's diagnostics and the files, edits apply in one fixed order, and rustfmt
formats the result, so a re-run on the same base gives the same tree. The audit re-runs it and diffs.

## The rules

Each is pinned by a unit test in `rules_test.go` / `rules_more_test.go` on a small fixture.

- **E0053 signature**: a `Volume` (or `ScanSource`, `PatchSource`) impl method's param or return type rustc says is
  incompatible with the trait is retyped (`&Path` → `&AppPath`, `&[PathBuf]` → `&[AppPathBuf]`, `PathBuf` →
  `AppPathBuf`). A reverse mismatch (a link target the trait keeps as `Path`) is left alone. While a method's signature
  is being retyped, errors in its body are skipped for that round: they're stale (rustc checked the body against the old
  signature).
- **Body prelude**: an error inside a flipped method's body that names a typed param gives the body
  `let path_std = path.as_std_legacy(); let path: &Path = &path_std;` (a `Vec<PathBuf>` twin for a batch). It goes
  inside the `async move` block(s) when every use is there (a future must own what it borrows), else at the top of the
  body. A body that only hands the param on to a typed slot (`self.inner.get_metadata(path)`) compiles as is and gets no
  prelude.
- **Typed param handed back**: a prelude'd body handing its param to a typed slot gets the original back, kept as
  `let path_app = path;` before the prelude (a bridge round trip there would return a future borrowing a temporary).
- **Lazy helper** (`rules_lazy.go`): E0515 on a prelude'd body whose tail is a call tree returning the future moves the
  tree into `Box::pin(async move { prelude; <tree>.await })`, the same future. Tails: `Box::pin(self.helper(path, …))`,
  a bare `self.helper(path, …)` (the helper returns the box), nested (`Box::pin(self.noting(self.x_impl(path)))`,
  `Box::pin(self.noting(scan_walk::scan_one(self, path)))`), and `let w = <tree>; Box::pin(async move { … w … })` (the
  lets move to the block's head). Every call in the tree must be lazy: an `async fn`, or a `fn` whose body is ONLY
  `Box::pin(async move { … })`, looked up in the crate, else in every crate under `crates/`, every definition of the
  name agreeing; a tuple-struct or variant constructor (`Some(x)`, `Opening::Fresh(mode)`) counts too. Every leaf
  argument must be plain: a local, `&`/`&mut` of one, a field read through `self`, a path (`None`,
  `MacOnlyFiles::Include`), or a number, so evaluating it at the first poll gives today's value. ❌ No method call, `?`,
  `.await`, index, closure, macro, or literal string: those stay residue, as does any call in the tree with a statement
  before its `Box::pin` (moving it would delay that work).
- **Round trip**: E0515 on `&AppPath::legacy_unchecked(&path.as_std_legacy())` (or the batch
  `&paths.iter().map(|p| AppPath::legacy_unchecked(&p.as_std_legacy()).into_owned()).collect::<Vec<_>>()`) collapses it
  to `path` (`paths`) when that's a param of the enclosing fn typed `&AppPath` (`&[AppPathBuf]`): the identity. The
  codemod no longer makes these (a stale body error is now skipped in ANY fn whose signature is being retyped,
  `macro_rules!` bodies included); the rule mends trees flipped before that. Its split twin (`rules_hoist.go`): an
  earlier pass's `&AppPath::legacy_unchecked(path)` after a later pass's prelude shadowed the typed `path` gets the
  param back (the prelude goes when nothing else reads it, else the param is kept as `path_app`, as below).
- **E0716 hoist** (`rules_hoist.go`): `AppPath::legacy_unchecked(&dir)` inside a statement whose result outlives it (a
  future held in a `let`, a `tokio::join!` arm) moves to `let dir_app = AppPath::legacy_unchecked(&dir);` just before
  the statement. Only a plain argument moves (nothing in the statement runs in another order); a name the fn already
  uses stays residue.
- **E0308 by type pair** (`valueRules`): a std path into a `&AppPath` slot becomes `app_path!("…")` for a canonical
  literal (`isCanonicalLiteral`: a POSIX path of plain names, or `scheme://authority/names`), else
  `&AppPath::legacy_unchecked(…)`; a batch maps each through the bridge; an owned `PathBuf` into an `AppPathBuf` slot is
  `AppPath::legacy_unchecked(&…).into_owned()`; a typed value into a `&Path` or `PathBuf` slot is `.as_std_legacy()`
  (`.into_owned()`); the `BridgedPath` rows wrap (`BridgedPath::new`), unwrap (`.into_path_buf()`), or read (`.app()`).
  A struct literal's shorthand field (`Foo { path }`) is spelled out. `cmdr_fs::testing`'s `TestDir` and
  `MetadataFixture` deref to `Path`, so they ride the `&PathBuf` rows (`&AppPath::legacy_unchecked(&dir)`).
- **Receiver**: a typed receiver calling a `Path` method (E0599: the typed twins of `Path`'s methods sit on the
  `PathArithmetic` trait, which nothing outside `cmdr-fs`'s app path module and pins imports during the flip, so
  `x.file_name()` on a typed `x` never compiles silently), or a same-named `AppPath` method fed a `std::path` argument
  where the trait IS imported, drops the RECEIVER to `.as_std_legacy()`, ❌ never the argument to `legacy_unchecked`:
  the typed twin answers otherwise on bridge values (`crates/cmdr-fs/src/app_path/flip_shapes_test.rs`).
- **Borrowed return** (`root`, `stands_in_for_own_disk`, `local_backing_file`): the field the method returns is retyped
  to `BridgedPath` (`Option<BridgedPath>`) in its struct, and the method returns `self.field.app()`
  (`.as_ref().map(BridgedPath::app)`). A literal root becomes `AppPath::from_normalized("/").expect(…)`. A `self.x()`
  accessor is no field: residue.
- **Comparison**: `assert_eq!`/`assert_ne!` (and an in-file `==` that rustc labels) comparing a typed value with a std
  one drops the typed operand to `Path` behind as many `&` as the other side has (read from rustc's
  `required for `&A`to implement`PartialEq<&B>`` note, since its message names the pair with the shared `&`s peeled)
  (`&*path.as_std_legacy()` against a `&PathBuf`, `*path.as_std_legacy()` against a `PathBuf`; `Cow<Path>` compares with
  neither), a `BridgedPath` through `.as_path()`, and an `Option`/`Vec` of typed paths against one of `PathBuf`s to the
  same container of owned std paths. An operand an earlier pass left as `….as_std_legacy()` gets reshaped the same way.
  The expected value is untouched; any other shape (`Option<&AppPath>`) is residue.
- **Imports**: a name a rule introduced (`AppPath`, `AppPathBuf`, `BridgedPath`, `app_path!`, `Path`, `PathBuf`) is
  imported at rustc's suggested point, or at the head of the innermost module when rustc suggests none (it doesn't for
  another crate's items). Never twice in one module, and ❌ never between an item and its outer attributes or doc
  comments (it would take the item's `#[cfg]` and ungate the item): the point lifts above them. A `Path`/`PathBuf`
  import a rewrite left unused is dropped. `-audit-imports <dir>` (repeatable) lists every codemod-style import sitting
  right under an attribute line, to check a tree flipped before that fix (clean on `cmdr-fs` and `cmdr-archive`).
- **Clippy phase**: once the crate checks, the machine-applicable fix of `useless_vec`, `unnecessary_to_owned`,
  `needless_borrow`, `redundant_clone`, and a few more (`clippyFixable`) is applied: the base is clippy-clean, so those
  lints are the rewrites'.

## Passes, residue, and commits

A crate that dev-depends on itself (most do, for its `testing` feature) builds its tests only once its lib checks, so
the work comes in passes. Each pass is its own commit, so the audit can replay them in order:

1. Run the codemod. Commit everything it changed: `refactor(<crate>): flip to app-path volume calls (codemod pass N)`.
2. Fix what it lists as residue, by hand, in the bridge's terms (no logic change). Commit it:
   `refactor(<crate>): app-path flip residue (pass N)`, the body saying what each fix is and why the codemod couldn't.
3. Run the codemod again. If it edits anything (it usually does once the lib checks and the tests compile), that's pass
   N+1: back to step 1. Stop when a run reports `checks: true` with no edits.

What it leaves as residue, by shape (each seen on `cmdr-fs` or `cmdr-archive`):

- **A future borrowing the prelude from an eager helper** (E0515 `path_std` on `self.open_stream(path, 0)` or
  `self.open_read_stream_impl(path)`, a `fn … -> Pin<Box<…>>` that does work before its future starts, like
  `cmdr-local`'s `let abs_path = self.resolve(path);`): retype the helper's param to `&'a AppPath` and give the helper
  the prelude, then pass the typed param straight through (drop the caller's prelude). A helper with a second, std
  caller (`cmdr-local`'s `scan_subtree`, also called from `scan.rs` with a `&PathBuf`) gets that caller bridged.
- **An import whose only uses are `cfg`-gated** (`#[cfg(test)]` methods): the lib build warns it unused and the run
  stops. Give the `use` the same `#[cfg(…)]`.
- **A comparison with no labeled operands** (`**path != self.root` against a `BridgedPath` field): `*self.root`.
- **A borrowed return through an accessor** (`self.own_disk_home()`): retype the field it reads and return `.app()`.
- **An in-process type built in a helper** (`Vec<(AppPathBuf, …)>` handed to a `Vec<(PathBuf, …)>` parameter): bridge
  the element (`path.as_std_legacy().into_owned()`).
- Generic `P: AsRef<Path>` helpers forwarding to a volume, closures whose parameter types are inferred, macros, and
  non-canonical literals the rules route to the bridge but rustc still rejects.
- ❗ **Code behind a `cfg` this host doesn't build** (`#[cfg(target_os = "linux")]`, `#[cfg(windows)]`): rustc never
  sees it here. Cross-check (`cargo check --all-targets --target x86_64-pc-windows-gnu` and `x86_64-unknown-linux-gnu`,
  zig as the C compiler: `docs/notes/windows-port/app-path-flip-handoff.md`) and fix what it finds as residue.

The residue's own audit: it touches no `assert*!` line and no expected value, and no line outside a function that names
a `Volume` method or a bridge.
