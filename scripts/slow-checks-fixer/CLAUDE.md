# Slow-checks fixer agent

A headless Claude Code run on the M1 box that picks up each red nightly "Slow checks" run on `main`, and each red CodeQL
run there. Each workflow's `report` job opens (or comments on) one issue, `slow-checks` or `codeql`, through
`.github/actions/red-run-issue`; launchd starts `run.sh` hourly as the restricted `agents` user, and when an issue names
a red run nobody has handled, the agent diagnoses each failing job and then reruns an outside blip, lands a clear fix
through `pnpm land`, or writes up what needs David. A green run closes the issue.

- `run.sh`: the launcher (finds the issue and run, mints the App token, claims the issue, readies a worktree with a warm
  `target/` and the failing jobs' logs, starts `claude -p` and watches the claim, removes it all after). `claim.sh`: the
  board claim (`free` / `take` / `held` / `release`). `prompt.md`: the agent's instructions, with placeholders `run.sh`
  fills in.

## Must-knows

- **Rule of Two**: it takes untrusted input (CI logs quoting third-party text) and can change Cmdr, so it holds nothing
  sensitive: the `agents` user, the `agents` secret store, and a GitHub App token (DETAILS § Credentials). ❌ Never hand
  it a token of David's or run it as his user.
- **It never reads issue or PR comment TEXT**: the repo is public, so anyone can write there. `run.sh` reads the issue's
  comments only to find run ids, from `github-actions` alone, and hands the agent a log file. `claim.sh` reads only a
  comment's author association and time, never its body. Only an issue authored by `app/github-actions` counts; anyone
  with triage rights can add a label.
- **It never works on a taken issue**: the card in Implementing, or a maintainer's comment since the run was reported,
  means someone else is on it. It claims the card (Implementing plus one comment) before starting, a watcher re-checks
  every two minutes, and the agent stands down when someone takes over (DETAILS § The claim).
- **Sensitive paths never auto-land**: it opens a PR labeled `sensitive-path` and waits for David. CI enforces it:
  `CI OK` stays red on an App PR touching the list until `vdavid` approves its head. The list,
  `.github/workflows/sensitive-paths.txt`, feeds both the gate and the prompt, and lives where the App can't push. ❌
  Don't move it or the gate out of `.github/workflows/` (DETAILS § The sensitive-path gate).
- **The idle check runs before the token mint**, anonymously (the repo is public), so an idle hour holds no token.
- **Its notes go in the issue BODY**, so a fixed or rerun night costs David no extra email. It comments only for the
  claim (one, from `claim.sh take`) and when something needs his call. Keep that split if you touch the prompt.
- **It never loosens anything to go green**: no skipped or ignored checks, tests, advisories, or allowlists, no deleted
  tests, no new dependencies. Those need David, so it writes a recommendation and lands nothing.
- **One rerun per run, on attempt 1 only.** A blip that repeats isn't a blip.
- **`handled-runs` claims a run before the agent starts**, so a crash doesn't retry it hourly. The next red night names
  a new run, which gets a fresh look.

Credentials, paths, install, and the design decisions: `DETAILS.md`.
