# Updates module

Frontend auto-update checker, restart toast, and the manual "Check for updates" affordances.

## File map

- `updater.svelte.ts`: orchestration. The poll loop, the `checkForUpdates()` state machine, `startUpdateChecker()`, the
  toast/onboarding gating, `runMenuTriggeredCheck()`.
- `update-state.svelte.ts`: the `updateState` `$state` singleton, re-exported from `updater.svelte.ts`.
- `update-status-text.ts`: pure `formatUpdateStatus()`, shared by Settings and both toasts.
- `update-analytics.ts`: the `update_check` event's vocabulary and its one emitter.
- `update-download-failure.ts` / `update-install-failure.ts`: the typed download and install failures that pick the log
  level and spot a policy refusal (`DETAILS.md`).
- `UpdateToastContent.svelte` (`id: 'update'`, persistent) is the restart prompt; `UpdateCheckToastContent.svelte`
  (`id: 'update-check'`, 10 s) the menu-triggered phase status.
- `blocker-notices.ts`: what an install that can't write its bundle hears. `MoveToApplicationsDialog.svelte` for where
  it sits (translocated, read-only volume), `UpdateUnwritableToastContent.svelte` (`id: 'update-unwritable'`, persistent
  warn, once per version) for who may write it (a root-owned `.pkg` install).

## Must-knows

- **Copy lives in the `updates.*` catalog**, resolved through `t()`/`tString()`; `cmdr/no-raw-user-facing-string` is
  enforced here.
- **One flow on every OS**: three backend commands (check, download, install). ❌ No OS branch here and ❌ no
  `@tauri-apps/plugin-updater`: what "install" means (bundle sync, or a staged installer that Restart hands off) is the
  backend's. Only the toast's second line differs, through the catalog's platform select.
- **Nothing may show during onboarding.** Only `showUpdateToast()` (gated by the pure `shouldShowUpdateToast()`) and
  `raiseBlockerNotice()` (handed the same gate as `screenFree()`) raise anything; ❌ never `addToast` directly.
- **`checkForUpdates()` ❌ never returns early on `ready`.** That guard let installs sit 25-38 days on a build newer
  releases had passed; only `supersedesStagedUpdate` may overwrite a staged one.
- **"Later" can't silence the restart prompt, and the toast's second line is why it needn't.** ❌ Don't shorten the 24 h
  re-nudge into nagging (4 h only for a `critical` security fix, David's call), and ❌ don't cut `readyDetail` or the
  version row's `role="img"` label.
- **The security variant reads `updateState.securityNotice`**, the install's own answer from the signed staged bundle;
  ❌ never derive it from the manifest or the release notes. `DETAILS.md` § The security variant.
- **A bundle that can't be written never downloads, and never prompts for a password**; a FAILED classification is ❌
  not permission to update, and an install refused as `unwritable` is ❌ not a failure (no error report).
- **Every exit of `checkForUpdates()` fires one `update_check` event**, with the phase read off `updateState.status`, ❌
  never off the error message. `trigger` is defaultless, so a new call site has to pick its own bucket.
- **The background check is throttled by the backend** (`updateCheckDueIn` / `recordUpdateCheck`, persisted across
  relaunches): the loop wakes every 5 min and checks only when due. Every exit of a check reports whether the server
  answered. `DETAILS.md` § The schedule.
- **The organization's policy is the backend's call.** `checkForUpdate(trigger)` answers a typed outcome; a managed one
  (`updatesDisabledByPolicy`, `heldByPolicy`) lands in `updateState.managed` as a terminal phase, ❌ never a failure,
  warn, or toast of its own; `automaticChecksDisabledByPolicy` stops the poll loop silently. `DETAILS.md` § Managed
  policy.
- **Only a real production install reaches the endpoint** (`check_for_update` answers `None` otherwise), and the
  manifest URL is hardcoded in Rust. `src-tauri/src/updater/CLAUDE.md`.

Lifecycle, state machine, the managed outcomes, and decisions: `DETAILS.md`. Read it before any non-trivial work here:
editing, planning, reorganizing, or advising.
