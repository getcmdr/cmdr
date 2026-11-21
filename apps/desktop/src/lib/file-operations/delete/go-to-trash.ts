/**
 * "Go to trash": point a pane at the trash, either in general or at the exact
 * items an operation just put there.
 *
 * macOS keeps one trash per VOLUME, so there is no single directory to open. Both
 * entries below resolve the right one through `getTrashDir`, which asks Cocoa the
 * same question the trash move asked (`crates/cmdr-operations/src/delete/trash.rs`).
 * Windows' Recycle Bin is no folder a pane can list, so both first ask
 * `openTrashView`, which opens it in File Explorer when the drive keeps one.
 *
 * The two entries differ in what they know:
 *
 * - `goToTrash` knows only where the user is standing, so it opens the trash of
 *   the focused pane's volume. That's the command-palette entry.
 * - `goToTrashedItems` knows the operation, so it reads the recorded in-trash
 *   location out of the journal and lands the cursor on the item itself. That's
 *   the trash toast's button, and it's strictly better when it's available: the
 *   journal holds the OS's own answer, de-duplicated name and all.
 */

import { getTrashDir, getOperationLogDetail, openTrashView } from '#lib/tauri-commands/index.js'
import { whenOperationSettled } from '../settled-operations'
import {
  resolveLocationOrToast,
  navigateToDirInBestPane,
  revealFileInBestPane,
  type PaneRevealAPI,
} from '#lib/file-explorer/navigation/navigate-and-select.js'
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { asMutationError } from '../mutation-error'
import { isVolumeRoot, lastName, parentOrRoot } from '#lib/path/grammar.js'

const log = getAppLogger('fileOperations')

/**
 * How many item rows to read when looking for the trashed item to land on. Only
 * the first `rollbackUnit` row is used, but interior `searchOnly` rows share the
 * page, so this reads enough to get past a handful of them.
 */
const ITEM_PAGE_SIZE = 50

/** Dedup id, so repeating the action can't stack copies of the same notice. */
const NO_TRASH_TOAST_ID = 'go-to-trash-unavailable'

/**
 * Open the trash of the focused pane's volume.
 *
 * The pane's own directory is what picks the volume: standing in an external
 * drive and asking for the trash should open THAT drive's trash, not the boot
 * volume's. A volume with no trash says so rather than navigating somewhere
 * arbitrary.
 */
export async function goToTrash(explorer: PaneRevealAPI | undefined): Promise<void> {
  if (!explorer) {
    log.debug('goToTrash: no explorer; skipping (HMR or pre-mount)')
    return
  }
  const { path } = explorer.getPaneLocation(explorer.getFocusedPane())
  if (await openedAsSystemView(path)) return
  await openTrashDirFor(explorer, path)
}

/**
 * Where the OS draws the trash itself instead of keeping it in a folder a pane can list
 * (Windows' Recycle Bin, one Shell view of every drive's bin), it opens there, in the OS's
 * file manager. `true` when that settled it: opened, or a failure already said so. `false`
 * where a pane goes to a trash folder instead, or there's no trash, which the pane path
 * answers (`openTrashDirFor`).
 */
async function openedAsSystemView(path: string): Promise<boolean> {
  try {
    return (await openTrashView(path)) === 'opened'
  } catch (error) {
    log.info("Couldn't open the trash view for {path}: {reason}", {
      path,
      reason: asMutationError(error)?.type ?? error,
    })
    addToast(tString('fileExplorer.navigation.locationUnreachableToast'), { level: 'info', id: NO_TRASH_TOAST_ID })
    return true
  }
}

/**
 * Show what `operationId` moved to the trash: navigate to the trash directory the
 * items actually landed in and put the cursor on the first of them.
 *
 * Falls back to `fromPath`'s volume trash when the journal has no in-trash
 * location to offer (Linux records none, and a row can be missing after a crash).
 * Getting the user to the right trash is the point; the cursor is a bonus.
 */
export async function goToTrashedItems(
  explorer: PaneRevealAPI | undefined,
  operationId: string,
  fromPath: string,
): Promise<void> {
  if (!explorer) {
    log.debug('goToTrashedItems: no explorer; skipping (HMR or pre-mount)')
    return
  }
  // A Recycle Bin item's recorded place is a `$R…` name in `$Recycle.Bin`, which no pane
  // should land on: the bin opens as the OS shows it.
  if (await openedAsSystemView(fromPath)) return

  const landed = await firstTrashedItemPath(operationId)
  if (!landed) {
    await openTrashDirFor(explorer, fromPath)
    return
  }

  const location = await resolveLocationOrToast(landed.dir)
  if (!location) return

  // Never throws: a trashed DOTFILE with "show hidden files" off isn't in the
  // listing, and `revealFileInBestPane` leaves the user in the trash without it.
  await revealFileInBestPane(explorer, location, landed.name)
}

/**
 * Resolve `path`'s volume trash and open it, or say the volume hasn't got one.
 *
 * Never throws: the toast fires this with `void`, where a rejection escapes as an
 * unhandled one and auto-sends an error report for a slow drive.
 */
async function openTrashDirFor(explorer: PaneRevealAPI, path: string): Promise<void> {
  let trashDir: string | null
  try {
    trashDir = await getTrashDir(path)
  } catch (error) {
    // `timedOut` is the realistic one: a slow or sleeping volume that didn't answer in 2 s.
    log.info("Couldn't look up the trash for {path}: {reason}", {
      path,
      reason: asMutationError(error)?.type ?? error,
    })
    addToast(tString('fileExplorer.navigation.locationUnreachableToast'), { level: 'info', id: NO_TRASH_TOAST_ID })
    return
  }
  if (!trashDir) {
    log.info('No trash directory for {path}; nowhere to go', { path })
    addToast(tString('fileOperations.trash.noTrashHere'), { level: 'info', id: NO_TRASH_TOAST_ID })
    return
  }
  const location = await resolveLocationOrToast(trashDir)
  if (!location) return
  await navigateToDirInBestPane(explorer, location)
}

/**
 * Where the operation's first top-level item ended up, split into the directory to
 * open and the name to land on.
 *
 * Waits out the settle first: the journal batches item rows in memory and flushes
 * the tail in the finalize barrier, so reading at completion time hands back an
 * empty page (`settled-operations.ts`). Only `rollbackUnit` rows are the user's
 * own items; the `searchOnly` rows are interior leaves nobody asked to see.
 */
async function firstTrashedItemPath(operationId: string): Promise<{ dir: string; name: string } | null> {
  const settled = await whenOperationSettled(operationId)
  if (!settled) {
    log.warn('Operation {operationId} never settled; reading its items would come back empty', { operationId })
    return null
  }

  let detail: Awaited<ReturnType<typeof getOperationLogDetail>>
  try {
    detail = await getOperationLogDetail(operationId, ITEM_PAGE_SIZE, 0)
  } catch (error) {
    // The volume's trash is still the right place to go; only the cursor is lost.
    log.info("Couldn't read where {operationId} put its items; opening the volume trash: {error}", {
      operationId,
      error,
    })
    return null
  }
  const landed = detail?.items.find((item) => item.rowRole === 'rollbackUnit' && item.destPath !== null)
  if (!landed?.destPath) return null

  // An item right under a volume root (`/x`, `C:/x`) has no folder to reveal it in.
  const dir = parentOrRoot(landed.destPath)
  if (isVolumeRoot(dir)) return null
  return { dir, name: lastName(landed.destPath) }
}
