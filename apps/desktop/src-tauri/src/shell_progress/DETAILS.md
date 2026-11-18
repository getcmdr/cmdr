# Shell progress: details

## The flow

1. `operations_host` hands every relevant engine fact to `note_operations` / `note_progress` / `note_completed`.
2. Each fact updates the `Aggregate` under the hub's mutex and recomputes the summary. Only a CHANGED summary (a new
   whole percent, tone, or visibility) wakes the worker, so the 200 ms progress ticks of a big copy cost a map update,
   not a thread wake.
3. The worker (`shell-progress` thread) blocks on its channel, waits out `MIN_UPDATE_GAP` (500 ms) since the last OS
   update, drains the wake-ups that landed meanwhile, reads the summary as it stands then, and calls
   `set_progress_bar` on the main window when it differs from what's shown.

- **Decision: Tauri's `set_progress_bar`, no hand-made AppKit.** Why: Cmdr owns no Dock tile view to extend (`dock/` only pins the
  tile and builds its menu), tao's macOS bar already draws paused (yellow) and error (red), and one call per OS keeps the arms to a pure mapping. The one gap,
  no indeterminate look on macOS, is a mapping choice (below), not a reason to own a Dock tile view.
- **Decision: the main window carries it on every OS.** On macOS and Linux the bar is app-wide, so any window would do;
  on Windows it's per taskbar button, and the main window's is the one people look at. The main window exists hidden
  from launch (`tauri.conf.json`), so it's always there to call.
- **Decision: fed from `operations_host`, not a frontend listener.** Business logic stays in Rust, and the bar shows even
  with no window open.

## The aggregate

A SESSION starts when the first operation goes live (queued, running, or paused) and ends when none is. Within it:

- **Per operation**: bytes done over bytes total, or files when no bytes move (a same-volume move), the status
  corner's rule (`apps/desktop/src/lib/status-corner/operation-chip.ts`). A scan measures nothing (its totals are what
  it's counting); rolling back holds the last measure (undoing isn't progress); the finishing phases (no denominator
  left) read their step as full. A two-step operation (compress, then upload) folds into one run: step 2 of 2 at half
  is 75 %.
- **Across operations**: weighed by byte total when every measured operation moved bytes, else evenly. An operation not
  yet measured (queued, scanning) weighs as much as the average measured one and counts as nothing done, so five
  queued copies behind one at 90 % don't read "almost done".
- **A completed operation stays in at full** until the session ends, so the bar doesn't fall back when it leaves the
  queue. A cancelled or failed one leaves the session: that work isn't coming.
- **Indeterminate** while nothing in the session has measured anything.
- **Never "done" while work is listed**: floored to whole percents and capped at 99.
- **Paused** when every started operation is held by a person: paused, or parked on a conflict prompt
  (`TransferWaitReason::Paused` / `Conflict`, which the engine re-announces on both edges of the wait). A stall on a
  slow share, a reconnect, or an unknown wait is NOT a pause: nobody can resume it. Queued work behind a paused
  operation doesn't count as moving (it waits on that operation's lane).
- **Error** only when nothing is live and the queue keeps a non-instant failure: a full red bar until the person
  dismisses the failure, the status corner's failure mark. Beside live work the work wins: the bar reads normal (or
  paused) and the failure neither colors nor weighs on it, as the status corner shows progress over failure
  (`operation-chip.ts`). Why: the bar answers "what's happening now", and a red bar over a moving copy reads as that
  copy failing.
- Instant operations (rename, create folder, create file) never count, as in the status corner.

## Each OS

- **macOS** (Dock tile): tao draws a bar over the icon: blue, yellow (paused), red (error). No indeterminate look (tao
  draws `Indeterminate` as `Normal` at its LAST value), so an unmeasured start shows an empty bar.
- **Windows** (taskbar button, `ITaskbarList3`): every look, including the marquee for an unmeasured start. A marquee
  has no color, so an unmeasured start that's paused shows a full yellow bar.
- **Linux** (Unity `LauncherEntry` via `libunity`, dlopened by tao): read by Ubuntu Dock, KDE, and Plank; value and
  visibility only, so paused and error draw like normal. Tauri names the entry `<package name>.desktop`, which must
  match the installed `.desktop` file for the dock to find it (unverified: the real-machine checklist).

Evidence: tao 0.37.1's `platform_impl/{macos/progress_bar.rs, windows/window.rs, linux/taskbar.rs}`, read 2026-11-18.

## QA recipe (timing-sensitive: David looks)

1. `RUST_LOG=shell_progress=debug pnpm dev`; every OS update logs `showing Shown { … }`.
2. Copy a big folder (tens of GB) to a slow target, a USB stick or an SMB share, so the bar moves slowly. The Dock tile
   shows a bar after the scan, filling in whole percents.
3. Send it to the background (F2) and pause it in the operation queue window: the bar turns yellow and holds. Resume: blue again.
4. Copy onto a folder with a clash and leave the conflict dialog open: yellow while it waits.
5. Start a second copy: the bar re-weighs (it can step back, since more work arrived).
6. Cancel everything: the bar disappears within half a second. Let one finish: it disappears too.
7. Make one fail (for example, unplug the target mid-copy): a full red bar until the failure is dismissed in the queue window.
