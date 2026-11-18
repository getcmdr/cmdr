# Status corner

The main window's word on background work: the top-right row (`StatusCorner.svelte`, mounted once by
`routes/(main)/+page.svelte`) hosting `OperationChip.svelte`, `#lib/ask-cmdr/WakeIndicator.svelte`,
`#lib/suggested-ops/SuggestedOpsIndicator.svelte`, `#lib/onboarding/FdaBadge.svelte`, and
`#lib/indexing/IndexingStatusIndicator.svelte` (the hourglass), with any `children` to their left. `operation-chip.ts`
holds the chip's pure pick-and-measure rules; `operation-failure-watch.svelte.ts` raises the failure toast.

## Must-knows

- **The corner owns placement AND order, its members don't**: it carries the `position: absolute`, the offsets, and
  `--z-sticky`, and each member is a plain inline box. The hourglass stays last. ⚠️ The more TRANSIENT a member, the
  further LEFT (wake indicator, suggestions badge, then the FDA badge right before the hourglass): the row grows
  leftward, so the other order shoves the persistent one sideways whenever the transient one shows up. ❌ No `order`
  prop; render order here says it.
- **An ambient top-right indicator belongs HERE, ❌ never in the title-bar header**, which the corner overlays: a
  header-placed one draws over the hourglass. `onOpenOnboarding` is the one prop the corner forwards (only
  `+page.svelte` can mount the wizard); the FDA badge asks `#lib/onboarding/` for the rest.
- **A member opens NO subscription at mount.** `StatusCorner.svelte.test.ts` and `StatusCorner.a11y.test.ts` mount the
  real corner and stub neither the suggestions badge nor the wake indicator, so a listener in a member's `onMount`
  breaks both suites. Each member's state lives in its own `*.svelte.ts`, started from `routes/(main)/`.
- **No positioned ancestor**: `.main-content` stays static, or the corner moves with it.
- **The row is always mounted, so it's `pointer-events: none`** with `auto` on its children, or an empty box eats clicks
  over the pane.
- **The chip is a PREVIEW, not a queue**: one operation (first running, else first paused), a verb and an 80 px bar, no
  percentage, no "+N". ❌ Not `TransferProgressReadout` — its fixed-width cells blow past the corner. A REVERSAL's verb
  comes from `#lib/file-operations/reversal-wording.ts` (`snapshot.reverses` set), ❌ never `queue.row.label` (undoing a
  copy runs as a delete, so the corner would say "Deleting" over an undo).
- **Both gates are pure, in `operation-chip.ts`** (`pickChipOperation`, `pickChipState`): add and test one there, not in
  the markup. The bar is bytes, falling back to the file count when `bytesTotal` is 0; instant ops are excluded by TYPED
  `operationType`, ❌ never a substring test.
- **Indeterminate phases get a SPINNER, never a bar**: scanning says "Scanning…"; compression finishing uses its phase
  label. Both totals are zero, so file fallback would lie. A PAUSED determinate op keeps its bar under "Paused". Spoken
  labels lead with the visible state, ❌ never the tooltip's string.
- **Render the session's `etaSecondsDisplay`, ❌ never `progress.etaSeconds`**: the raw value once read "8m 12s" in one
  window, "5m 46s" in the other.
- **The FIRST appearance waits `CHIP_SETTLE_MS`** (blink-long work never flashes the corner, and the beat closes a race
  with the foreground modal's claim); a handover is immediate. ❌ No session before it settles (DETAILS).
- **The failure toast NEVER auto-dismisses**, and past three they collapse into one summary. A stack full of persistent
  toasts silently DROPS new ones, so ❌ don't raise that cap, and keep `toastGroup: 'operation-failure'`.
- **Suppression needs BOTH foreground slots**, in the chip and in the watch: the dialog releases
  `getForegroundOperationId()` as it unmounts and the failure row lands only after, so `getForegroundFailureId()` is
  what stops a double report.
- **The toast takes its title from `queue.failureToast.title` and its count off the store**, ❌ neither the pipeline's
  title nor a prop. DETAILS § "The failure notice" has the why for both.

Layout model, member contract, the chip's states, and decisions: `DETAILS.md`. Read it before any non-trivial work here:
editing, planning, reorganizing, or advising.
