/**
 * A Search dialog that throws gets its session reset, so the next ⌘F opens.
 *
 * Regression anchor: ERR-TWEGW / ERR-Y5QXR (cmdr-reports#114, #115). The dialog threw
 * Svelte's `each_key_duplicate` and its `DialogBoundary` closed it, but the session
 * (query, results, coverage note) survives a close by design, so every reopen rendered
 * the same state and threw again in the same millisecond, for the rest of the session.
 *
 * The broken state is seeded as a coverage note naming one folder twice: a list the
 * note keys by path, the shape that crashed, without needing a backend to produce it.
 *
 * Shared IPC fixture: `test-search-dialog-harness.ts`.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, unmount, flushSync, tick } from 'svelte'
import SearchDialog from './SearchDialog.svelte'
import { getCoverageNote, getQuery, resetSearchAfterCrash, setCoverageNote, setQuery } from './search-state.svelte'
import { resetSearchDialogTest, useSearchDialog } from './test-search-dialog-harness'

const { addToast, logError } = vi.hoisted(() => ({
  addToast: vi.fn(),
  logError: vi.fn(),
}))

vi.mock('#lib/tauri-commands/index.js', async () => (await import('./test-search-dialog-harness')).tauriCommandsMock())
vi.mock('../../routes/viewer/media-view', async () => (await import('./test-search-dialog-harness')).mediaViewMock())
// The coverage note reads the silenced-drives setting, whose repair writes it back.
vi.mock('#lib/settings/index.js', async () => ({
  ...(await import('./test-search-dialog-harness')).settingsMock(),
  setSetting: vi.fn(),
}))
vi.mock('#lib/indexing/index.js', async () => (await import('./test-search-dialog-harness')).indexingMock())
vi.mock('#lib/icon-cache.js', async () => (await import('./test-search-dialog-harness')).iconCacheMock())
vi.mock('#lib/ui/toast/index.js', () => ({ addToast }))
vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: logError }),
}))

import Host from '../../../test/fixtures/search-dialog-boundary-fixture.svelte'

useSearchDialog(SearchDialog)

/** A session that throws on render: the coverage note names one folder twice. */
function seedBrokenSession(): void {
  setQuery('*.log')
  setCoverageNote({
    uncoveredScopes: ['/Users/test/projects', '/Users/test/projects'],
    unresolvedScopes: [],
    volumeId: 'root',
  })
}

describe('SearchDialog crash reset', () => {
  let target: HTMLDivElement
  let component: Record<string, unknown> | null = null

  beforeEach(async () => {
    await resetSearchDialogTest()
    addToast.mockReset()
    logError.mockReset()
    target = document.createElement('div')
    document.body.appendChild(target)
  })

  afterEach(() => {
    if (component) {
      void unmount(component)
      component = null
    }
    target.remove()
  })

  /** Mounts the boundary host with the dialog open, the way `+page.svelte` does. */
  function render(onCrash: (view: { open: boolean }) => void): { view: { open: boolean } } {
    const view = $state({ open: true })
    component = mount(Host, {
      target,
      props: {
        view,
        onCrash: () => {
          onCrash(view)
        },
      },
    }) as Record<string, unknown>
    flushSync()
    return { view }
  }

  /** Lets the boundary's deferred re-arm run, then reopens the dialog. */
  async function reopen(view: { open: boolean }): Promise<void> {
    await new Promise((r) => setTimeout(r, 0))
    view.open = true
    flushSync()
    await tick()
  }

  it('a crash resets the session, so the next open renders instead of throwing again', async () => {
    seedBrokenSession()
    const { view } = render((v) => {
      resetSearchAfterCrash()
      v.open = false
    })
    expect(logError).toHaveBeenCalledTimes(1)
    expect(view.open).toBe(false)
    expect(getQuery()).toBe('')
    expect(getCoverageNote()).toBeNull()

    await reopen(view)

    expect(logError).toHaveBeenCalledTimes(1)
    expect(view.open).toBe(true)
    expect(target.querySelector('.search-overlay')).not.toBeNull()
    expect(target.querySelector('[data-testid="window-alive"]')).not.toBeNull()
  })
})
