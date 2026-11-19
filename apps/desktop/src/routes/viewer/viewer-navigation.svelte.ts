/**
 * Following links between documents in one viewer window, and Back / Forward through them.
 *
 * A click in the formatted Markdown view hands its `href` here; the backend decides where it
 * goes (`viewerFollowMarkdownLink`): a web or mail link has already gone to the user's browser
 * or mail app, an anchor scrolls the view, and a file on the document's volume opens in THIS
 * window. Back / Forward (⌘[ / ⌘], the Go menu) step through those files with their scroll
 * positions (`viewer-history.ts`).
 *
 * The page owns the session swap (`deps.swapTo`): the new file opens first (`deps.open`), and
 * only a successful open tears the current one down, so a missing file costs a toast and
 * nothing else. Same getter / callback deps shape as the other `createViewer*` composables.
 */

import { asViewerError, viewerFollowMarkdownLink, viewerSetHistoryState } from '#lib/tauri-commands/index.js'
import { addToast } from '#lib/ui/toast/toast-store.svelte.js'
import { tString } from '#lib/intl/messages.svelte.js'
import type { Logger } from '#lib/logging/logger.js'
import { createViewerHistory, type HistoryDirection, type ViewerHistory } from './viewer-history'
import type { MarkdownLanding } from './markdown-view'

export interface ViewerNavigationDeps<OpenResult> {
  getSessionId: () => string
  /** The window is opening or reopening something of its own (`loading`). */
  isBusy: () => boolean
  /** How far the formatted view is scrolled, or `null` when it isn't showing. */
  getMarkdownScrollTop: () => number | null
  scrollToElement: (elementId: string | null) => void
  /** Opens `path` without touching page state; throws the typed open failure. */
  open: (path: string) => Promise<OpenResult>
  /** Tears the current file down and shows the opened one. */
  swapTo: (args: { path: string; result: OpenResult }) => Promise<void>
  log: Logger
}

export function createViewerNavigation<OpenResult>(deps: ViewerNavigationDeps<OpenResult>) {
  let history: ViewerHistory | null = null
  let landing = $state<MarkdownLanding | null>(null)
  let navigating = $state(false)
  /** Whether Go > Back / Forward are live, for the bar this window's page draws (off macOS). */
  let steps = $state({ canGoBack: false, canGoForward: false })

  /**
   * Tells the menu bar whether Go > Back / Forward are live: this window's page bar always, and
   * macOS's shared native bar only from the viewer in front, so a step that lands while another
   * window has focus keeps quiet there; the focus-gain push (`force`) catches up.
   */
  function pushHistoryState({ force = false }: { force?: boolean } = {}): void {
    if (!history) return
    steps = { canGoBack: history.canGoBack, canGoForward: history.canGoForward }
    if (!force && !document.hasFocus()) return
    viewerSetHistoryState(history.canGoBack, history.canGoForward).catch(() => {})
  }

  /** Opens `path` in place of the current file, landing at `target`. `false` when it didn't open. */
  async function show(path: string, target: MarkdownLanding): Promise<boolean> {
    // Where the document being left was, so Back returns there.
    const scrollTop = deps.getMarkdownScrollTop()
    if (scrollTop !== null) history?.rememberScroll(scrollTop)
    navigating = true
    let result: OpenResult
    try {
      result = await deps.open(path)
    } catch (e) {
      deps.log.warn('Opening a linked file didn’t work: {error}', { error: String(e) })
      addToast(openFailureCopy(e), { level: 'warn' })
      return false
    } finally {
      navigating = false
    }
    landing = target
    await deps.swapTo({ path, result })
    return true
  }

  return {
    /** Where the next rendered document lands; read by the formatted view once per render. */
    get landing() {
      return landing
    },
    /** A linked file is opening; the current one stays up meanwhile. */
    get navigating() {
      return navigating
    },
    /** Whether there's a document to step back to, as Go > Back shows it. */
    get canGoBack() {
      return steps.canGoBack
    },
    /** Whether there's a document to step forward to, as Go > Forward shows it. */
    get canGoForward() {
      return steps.canGoForward
    },

    /** The window's first document, the history's only entry. */
    start(path: string): void {
      history = createViewerHistory(path)
    },

    /** A view switch reopens the same document at its top, never at a link's old target. */
    clearLanding(): void {
      landing = null
    },

    pushHistoryState,

    /** A link in the formatted view was clicked. */
    async followLink(href: string): Promise<void> {
      const sessionId = deps.getSessionId()
      if (!sessionId || navigating) return
      try {
        const link = await viewerFollowMarkdownLink(sessionId, href)
        switch (link.kind) {
          case 'external':
            return
          case 'anchor':
            deps.scrollToElement(link.elementId)
            return
          case 'file':
            if (await show(link.path, { kind: 'element', elementId: link.elementId })) {
              history?.visit(link.path)
              pushHistoryState()
            }
            return
          case 'blocked':
            addToast(tString('viewer.markdown.link.blocked'), { level: 'info' })
            return
        }
      } catch (e) {
        deps.log.warn('Following a Markdown link didn’t work: {error}', { error: String(e) })
        addToast(tString('viewer.markdown.link.unreadable'), { level: 'warn' })
      }
    },

    /** Go > Back / Forward, or ⌘[ / ⌘]: a no-op with nothing that way. */
    async step(direction: HistoryDirection): Promise<void> {
      const target = history?.target(direction)
      if (!target || navigating || deps.isBusy()) return
      if (await show(target.path, { kind: 'scroll', top: target.scrollTop })) {
        history?.step(direction)
        pushHistoryState()
      }
    },
  }
}

/** The toast for a linked file that didn't open, by its typed failure. */
function openFailureCopy(e: unknown): string {
  const kind = asViewerError(e)?.kind
  if (kind === 'notFound') return tString('viewer.markdown.link.missing')
  if (kind === 'isDirectory') return tString('viewer.markdown.link.folder')
  return tString('viewer.markdown.link.unreadable')
}
