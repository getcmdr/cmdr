# Git hooks

The repo's git hooks keep formatting from being forgotten. CI's `oxfmt`, `rustfmt`, and `gofmt` lanes fail on an
unformatted file, and a push that skipped a formatter used to turn `main` red for something a tool fixes in seconds. The
hooks run those formatters for whoever commits or pushes, without asking them to do anything.

- `.githooks/`: the hook entry points. `pre-commit`, `post-commit`, and `pre-push` are one-liners into `run-hook`, which
  builds and runs the program below.
- `scripts/git-hooks/`: the logic, a small Go program with its own tests. `formatters.go` wraps the three tools,
  `precommit.go` and `prepush.go` hold the hooks.

## Installation

`core.hooksPath` points at `.githooks`, set by the root `prepare` script during `pnpm install`. The setting lives in the
shared git config, so one install covers the main clone and every worktree. The path is relative, so each worktree runs
its own copy of the hooks, and a worktree on a commit without `.githooks/` has none.

pnpm skips lifecycle scripts when an install has nothing to do ("Already up to date" in under a second; verified on pnpm
11.27.1, 2026-09-30), so a clone that was installed before the hooks existed needs `git config core.hooksPath .githooks`
once. `git config --get core.hooksPath` says whether the hooks are on.

`prepare` skips the setting when `$CI` is set: a hook stopping a workflow's own push (the release pipeline's Homebrew
tap bump) would break a release.

## What each hook does

- **`pre-commit`** formats the staged files and re-stages them, so the formatting is part of the commit. It prints one
  line when it changed something and nothing otherwise.
- **`pre-push`**, on a push to any branch but `main`, is the backstop for what `pre-commit` never sees: a rebase, a
  conflict resolution, a `--no-verify` commit, a generated file. Two fixers, each committing what it rewrote and then
  stopping the push with a "push again":
  - **Formatting.** The same whole-repo commands as the CI lanes. If they'd fail, it formats those files and commits
    them as `style: apply formatter output`.
  - **License notices** (`scripts/git-hooks/prepush_checks.go`, through the check runner). When the pushed range changes
    `Cargo.lock`, `pnpm-lock.yaml`, `deny.toml`, or the vendored credits, it runs `desktop-third-party-notices`
    (`--fresh`, about a minute) and commits a rewritten `THIRD-PARTY-NOTICES.md` or `third-party-packages.gen.json` as
    `chore(deps): regenerate third-party notices`. Stale notices turned `main` red 26 times in three months
    (`docs/notes/ci-health-2026-10.md`). The range is remote tip..pushed commit; for a new branch, or a remote tip this
    clone doesn't have, it's what the pushed commit adds over every remote-tracking ref
    (`git log <sha> --not --remotes`).

  Both run when a pushed commit **is** the checked-out `HEAD` commit, whatever the refs are called, since that's when
  the working tree is what's being pushed. A worktree branch goes out as `HEAD:<branch>` or `push-staging:<branch>` as
  often as under its own name; when the hook matched the checked-out branch's name instead, those pushes skipped every
  step and stale notices reached `main` (2026-10-07/08). A commit the hook adds lands on `HEAD`, so when the pushed ref
  doesn't follow `HEAD` (`push-staging:<branch>`), the stop message says to push `HEAD` next time.

  **No check lanes run at push time.** Agents push branches between milestones so CI runs the full suite on the PR while
  the next milestone develops; a local lane on every push would undo that. `pnpm check --fast` while iterating stays the
  local loop.

  **A push to `main` gets none of it**, with a one-line notice. `main` only takes commits CI already passed (`land`
  pushes one after its PR's `CI OK`, and the `main` ruleset refuses the rest), so a fixer commit there would be one CI
  never saw, and a notices run would be a minute of `cargo-about` for nothing.

  **`pnpm land` runs this hook itself, before it connects** (`git hook run pre-push`), then pushes with `--no-verify`:
  inside `git push` the notices minute outlasted GitHub's idle timeout. A plain `git push` of a lockfile change can
  still hit it. `scripts/land/DETAILS.md` § Gotcha: the pre-push hook runs before the connection.

- **`post-commit`** repairs the index after `git commit <paths>` (see the gotchas).

## Decisions

- **The push stops after a format commit, and it has to.** Git settles which commits a push sends before it runs
  `pre-push`, so a commit made inside the hook can't join the push in flight. Letting the push continue would send the
  unformatted tip and leave the fix behind. Pushing again from inside the hook isn't an option either: the hook never
  sees the flags of the original push (`--force`, `--force-with-lease`), and the outer push would still report a
  rejection after a push that landed.
- **`pre-push` checks the whole repo.** Checking only the pushed range would be faster, but the whole-repo check is
  indifferent to how the branch got its commits (a force-push needs no special case) and repairs a `main` that's already
  red. It costs about 5 s per push, with the three tools running in parallel (measured on an M-series Mac, 2026-09-30:
  `oxfmt --list-different .` 4.1 s, `cargo fmt --all -- --check` 5.1 s).
- **`pre-commit` only touches what's staged**, which keeps it under a second: `oxfmt` gets the staged paths, and each
  staged `.rs` file goes through `rustfmt` on its own.
- **A hook that can't do its job exits 0.** No Go toolchain, no `node_modules`, a hook program that doesn't build, a
  formatter that errors, a notices run that doesn't finish: the commit or push goes through and CI stays the gate. The
  one non-zero exit is a push that got a format or notices commit. `git push --no-verify` gets past it.
- **Each tool decides its own scope.** `oxfmt` gets every candidate file and applies `.oxfmtrc.json` itself. Rust scope
  and each crate's edition come from `cargo metadata --no-deps`, so only workspace members are formatted and `vendor/`
  stays byte-identical to upstream. The Go directories are the one hand-kept list (`goDirs`), mirroring
  `GetGoDirectories` in `scripts/check/checks/common.go`.

## What the hooks leave alone

- **A partially staged file** (`git add -p`, or edits made after staging): formatting and re-staging it would pull the
  unstaged edits into the commit. `pre-push` catches it later.
- **An unformatted file with uncommitted changes, at push time**: the formatters read the working tree, so their verdict
  says nothing certain about the pushed commit, and committing the file would sweep work in progress along. CI reports
  it.
- **A push that doesn't send the checked-out commit**: another branch's commit gets a one-line `pre-push: skipped ...`
  notice, since the working tree doesn't describe it. Tags and deletions put no branch content on the remote and pass
  silently, so a release's tag push goes through untouched.
- **A push during a merge, rebase, cherry-pick, or revert**: no commit can be added there (same notice).
- **Untracked files**: `oxfmt .` sees them, the push doesn't contain them.

## Gotchas

- **`git commit <paths>` commits from a temporary index.** Git prepares the real index before the hook runs and installs
  it afterwards, so what `pre-commit` re-staged never reaches it, and the file would show as both staged and modified
  right after a clean commit (verified on git 2.55.0, 2026-09-30). `pre-commit` leaves the list of files in
  `<git-dir>/cmdr-format-restage` and `post-commit` stages them in the real index.
- **`rustfmt <file>` also rewrites every out-of-line `mod` the file declares**, which for a `lib.rs` is the whole crate,
  including unstaged work. So the hooks pipe each file through `rustfmt`'s stdin, run from the file's own directory so
  the nearest `rustfmt.toml` applies (`crates/fsevent-stream` pins its own), with the crate's `--edition` the way
  `cargo fmt` passes it.
- **`pnpm exec oxfmt` can trigger a full install** in a fresh worktree (10 s). The hooks call `node_modules/.bin/oxfmt`
  directly (0.5 s).
- **Hooks inherit the PATH of whoever runs git.** A GUI git client without the mise shims on its PATH finds no `go`, and
  the hooks skip themselves.

## Working on the hooks

- The tests drive real git against throwaway repos whose hooks call back into the test binary, so every case runs the
  way git runs it. `gofmt` is the formatter under test in most of them; the Rust and `oxfmt` cases skip themselves where
  the tool is missing.
- `run-hook` builds the program into `<git-dir>/cmdr-git-hooks` on every run (about 60 ms when nothing changed), so an
  edit under `scripts/git-hooks/` takes effect on the next commit in that worktree.
- `git commit --no-verify` and `git push --no-verify` skip the hooks.
