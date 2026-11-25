/**
 * When a wait's face appears: the loading view over a slow folder (`LoadingIcon`) and a
 * remote place's connect view (`RemoteConnectView`) follow this ONE rule, through the
 * global `.wait-reveal` class (`app-utilities.css`), so the two can't drift apart.
 *
 * ❗ Invisible until {@link WAIT_REVEAL_AFTER_MS} after the wait began, then a
 * {@link WAIT_REVEAL_RAMP_MS} fade (none under `prefers-reduced-motion`: it just appears).
 * The pane itself still empties at the earlier `LISTING_LOADING_DELAY_MS` cover, so the old
 * rows never sit there looking current; only the spinner and its words wait longer.
 */

/**
 * 800 ms: most slow folders and most connects finish inside it, and a spinner that flashes
 * for a few hundred milliseconds reads as a glitch rather than as progress. Past it, the
 * wait is long enough that saying so (and offering Cancel) helps. David's call, 2026-11-25.
 */
export const WAIT_REVEAL_AFTER_MS = 800

/** The fade once it shows: the pace the loading view always had (its old 400 ms fade spent its first half transparent). */
export const WAIT_REVEAL_RAMP_MS = 200

/**
 * The inline style for a `.wait-reveal` element that mounts `mountedAfterMs` into its wait:
 * it holds still for the rest of {@link WAIT_REVEAL_AFTER_MS}, then fades in.
 */
export function waitRevealStyle(mountedAfterMs: number): string {
  const delayMs = Math.max(0, WAIT_REVEAL_AFTER_MS - mountedAfterMs)
  return `--transition-wait-reveal-delay: ${String(delayMs)}ms; --transition-wait-reveal-ramp: ${String(WAIT_REVEAL_RAMP_MS)}ms`
}
