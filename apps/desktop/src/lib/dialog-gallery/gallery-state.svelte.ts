/**
 * Which gallery dialog the main window is currently previewing.
 *
 * Read only by `DialogGallery.svelte`, which the app mounts behind the
 * `import.meta.env.DEV || __CMDR_E2E_BUILD__` gate, so the whole module tree
 * tree-shakes out of a plain production build. Nothing in the main window's own
 * graph reads it: a previewed dialog suppresses shortcuts by registering in
 * `#lib/ui/open-dialogs.svelte` like any other soft dialog, since the gallery
 * renders the SHIPPING component.
 */

import type { SoftDialogId } from '#lib/ui/dialog-registry.js'
import type { FileEntry } from '#lib/file-explorer/types.js'
import type { TransferProgressRun } from './fixtures/transfer-progress'

/**
 * The real directory the disk-backed dialogs work against, plus the live pane
 * state they need to behave for real.
 *
 * `delete-confirmation`, `transfer-confirmation`, `mkdir-confirmation`,
 * `new-file-confirmation`, and `go-to-path` all do real work on mount (scans,
 * conflict lookups, space queries, path resolution). Faking that would fake the
 * numbers the design displays, so `disk-fixture.ts` resolves this from the
 * fixture directory the dev-only Rust command creates and the pane it navigates
 * there.
 */
export interface GalleryDiskFixture {
  /** The fixture directory (`<app data dir>/dialog-gallery-fixtures`). */
  root: string
  /** A folder inside `root` the copy / move states use as their destination. */
  destinationDir: string
  /** A folder name directly inside `root`, for the mkdir conflict state. */
  existingFolderName: string
  /** A filename directly inside `root`, for the mkfile conflict state. */
  existingFileName: string
  /** A deep path inside `root`, for the "Go to path" preview. */
  nestedPath: string
  /** The focused pane, navigated to `root`. */
  paneSide: 'left' | 'right'
  /** That pane's live listing of `root` — the handle mkdir / mkfile need. */
  listingId: string
  /** The volume `root` sits on, as the pane resolved it. */
  volumeId: string
  showHiddenFiles: boolean
  /** Real top-of-listing entries from `root`, in the pane's sort order. */
  entries: FileEntry[]
}

/** What a preview needs beyond its fixture record, resolved before it opens. */
export interface GalleryPreviewContext {
  /** Present only for the disk-backed dialogs. */
  disk?: GalleryDiskFixture
  /** Present only for `transfer-progress`: the real copy the dialog starts. */
  transfer?: TransferProgressRun
}

/** The dialog + named state the Debug window asked for. */
interface OpenGalleryDialog extends GalleryPreviewContext {
  dialogId: SoftDialogId
  /** A `DialogGalleryState.id` from the entry's `states` list. */
  stateId: string
}

const galleryState = $state<{ open: OpenGalleryDialog | null }>({ open: null })

/** Opens (or swaps to) a gallery preview. Re-triggering the same state remounts the dialog. */
export function openGalleryDialog(dialogId: SoftDialogId, stateId: string, context: GalleryPreviewContext = {}): void {
  galleryState.open = { dialogId, stateId, ...context }
}

/** Closes whatever the gallery is previewing. The harness passes this as every fixture's `onClose`. */
export function closeGalleryDialog(): void {
  galleryState.open = null
}

/** The current preview, or `null`. Read reactively by `DialogGallery.svelte`. */
export function getOpenGalleryDialog(): OpenGalleryDialog | null {
  return galleryState.open
}
