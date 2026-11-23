/**
 * Where the translator screenshots live so they outlive a worktree.
 *
 * The PNGs in `src/lib/intl/messages/screenshots/` are gitignored (they churn on
 * every UI change), so a capture run in a worktree used to write them only there,
 * and removing the worktree deleted them while the tracked `capture-report.json`
 * and `@key.screenshot` couplings it landed kept naming them. The fix is one
 * machine-wide store: the MAIN clone's `screenshots/` folder, found through
 * `git rev-parse --git-common-dir`, so any worktree reaches it without naming a
 * path.
 *
 * Two moves keep a worktree and the store in step:
 *  - **Publish**: a capture that finishes green copies its PNGs into the store
 *    (`i18n-capture.ts`). Only a green run, the same rule that keeps its rewrite of
 *    the tracked report, so the store never holds half a run.
 *  - **Seed**: a new worktree starts with a copy of the store
 *    (`scripts/worktree-hooks/post-create`), so a translator or the coupler's OCR
 *    in a worktree sees the images at the usual relative path.
 *
 * Copies are APFS clones where the volume allows (`COPYFILE_FICLONE`, free and
 * instant), else plain copies. Run from the main clone itself, publish is a no-op.
 */

import { constants, copyFileSync, existsSync, mkdirSync, readdirSync, realpathSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoots } from './i18n-repo-lib.ts'

/** The screenshots folder relative to a checkout's root. */
export const SCREENSHOTS_REL_DIR = join('apps', 'desktop', 'src', 'lib', 'intl', 'messages', 'screenshots')

/** The machine-wide store: the main clone's screenshots folder. */
export function screenshotStoreDir(): string {
  return join(repoRoots().mainClone, SCREENSHOTS_REL_DIR)
}

/** Whether two paths name the same folder (a run from the main clone publishes onto itself). */
function sameDir(a: string, b: string): boolean {
  try {
    return realpathSync(a) === realpathSync(b)
  } catch {
    return false
  }
}

/**
 * Copies every top-level PNG in `fromDir` into `toDir`, replacing a same-named one.
 * Subfolders (the gitignored `overflow/` review shots) stay put: they aren't
 * couplings and nothing outside the run reads them.
 * @returns how many images it copied; 0 when the two folders are the same.
 */
export function publishScreenshots(fromDir: string, toDir: string): number {
  if (!existsSync(fromDir) || sameDir(fromDir, toDir)) return 0
  mkdirSync(toDir, { recursive: true })
  let copied = 0
  for (const entry of readdirSync(fromDir, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith('.png')) continue
    copyFileSync(join(fromDir, entry.name), join(toDir, entry.name), constants.COPYFILE_FICLONE)
    copied++
  }
  return copied
}
