/**
 * Behavior tests for the error screen's action row.
 *
 * The row is the only way out of a failed listing, so which buttons appear is
 * load-bearing: `retryHint` is the backend's "retrying might help" signal (set on
 * Serious and NeedsAction errors too, not just Transient), and `canGoBack` must
 * hide a Go back that would silently no-op.
 */

import { describe, it, expect, vi } from 'vitest'
import { mount, tick } from 'svelte'
import ErrorPane from './ErrorPane.svelte'
import { withPlatform } from '#lib/platform/test-platform.js'
import type { FriendlyError } from '../types'
import type { ErrorPaneAPI } from './types'

vi.mock('#lib/tauri-commands/index.js', () => ({
  openPrivacySettings: vi.fn(() => Promise.resolve()),
  openExternalUrl: vi.fn(() => Promise.resolve()),
  openSystemSettingsUrl: vi.fn(() => Promise.resolve()),
}))

vi.hoisted(() => vi.stubGlobal('__CMDR_PLATFORM__', 'macos'))

const askToSignInToList = vi.hoisted(() => vi.fn())
vi.mock('../network/share-list-sign-in', () => ({ askToSignInToList }))

const openErrorReportFor = vi.hoisted(() => vi.fn())
vi.mock('#lib/error-reporter/error-report-flow.svelte.js', () => ({ openErrorReportFor }))

const base = {
  title: 'Something went wrong',
  explanation: 'The folder did not open.',
  suggestion: 'Try one of these.',
  rawDetail: 'EIO',
} satisfies Omit<FriendlyError, 'category' | 'retryHint'>

describe('a folder behind a link to another server', () => {
  const friendly: FriendlyError = {
    ...base,
    category: 'needs_action',
    retryHint: false,
    linkConsent: { server: 'nas', target: 'fs01' },
  }

  it('offers the review, which asks through its callback', async () => {
    const onReviewLinkConsent = vi.fn()
    const target = mountPane({ friendly, onReviewLinkConsent })
    await tick()
    const review = [...target.querySelectorAll('button')].find((b) => b.textContent.includes('Review sign-in'))
    review?.click()
    expect(onReviewLinkConsent).toHaveBeenCalledOnce()
    target.remove()
  })

  it('offers no review on any other error', async () => {
    const target = mountPane({ friendly: { ...base, category: 'transient', retryHint: true } })
    await tick()
    expect(target.textContent).not.toContain('Review sign-in')
    target.remove()
  })
})

function mountPane(props: Partial<Parameters<typeof ErrorPane>[1]> & { friendly: FriendlyError }): HTMLDivElement {
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(ErrorPane, { target, props: { folderPath: '/Volumes/x/y', ...props } as never })
  return target
}

function buttonTexts(target: HTMLElement): string[] {
  return Array.from(target.querySelectorAll('button')).map((b) => b.textContent.trim())
}

describe('a share list with no tool to list it', () => {
  it('shows the install command in a copy box, and no Sign in (an account can’t fix it)', async () => {
    const installCommand = 'sudo apt install smbclient'
    const friendly: FriendlyError = {
      ...base,
      category: 'needs_action',
      retryHint: true,
      shareListRefusal: { type: 'missing_dependency', message: 'smbclient: not found', installCommand },
    }
    const target = mountPane({ friendly, folderPath: 'smb://testnas.local' })
    await tick()
    expect(target.textContent).toContain(installCommand)
    expect(buttonTexts(target)).not.toContain('Sign in')
    target.remove()
  })
})

describe('a share list that wants an account', () => {
  const friendly: FriendlyError = {
    ...base,
    category: 'needs_action',
    retryHint: false,
    actionKind: 'sign_in_to_server',
    shareListRefusal: { type: 'auth_required', message: '' },
  }

  // Where a cancelled sign-in sheet leaves the pane: focus on Sign in, so Enter asks again.
  it('puts focus on Sign in when asked, in an action row that keeps it (pane chrome)', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    const pane: ErrorPaneAPI = mount(ErrorPane, {
      target,
      props: { folderPath: 'smb://testnas', canGoBack: true, friendly },
    })
    await tick()
    expect(await pane.focusSignIn()).toBe(true)
    const focused = document.activeElement
    expect(focused?.textContent.trim()).toBe('Sign in')
    expect(focused?.closest('[data-pane-chrome]')).not.toBeNull()
    target.remove()
  })

  // The sheet takes focus into its fields as it opens, so its own restore has nowhere to go back to:
  // a cancel from this screen's Sign in lands back on Sign in all the same.
  it('lands back on Sign in when the sheet its Sign in opened is cancelled', async () => {
    askToSignInToList.mockResolvedValue({ kind: 'cancelled' })
    const onRetry = vi.fn()
    const target = mountPane({ friendly, folderPath: 'smb://testnas', onRetry })
    await tick()
    const signIn = [...target.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Sign in')
    signIn?.click()
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(signIn)
    })
    expect(onRetry).not.toHaveBeenCalled()
    target.remove()
  })

  it('has no Sign in to focus on any other error', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    const pane: ErrorPaneAPI = mount(ErrorPane, {
      target,
      props: { folderPath: '/Volumes/x/y', friendly: { ...base, category: 'serious', retryHint: true } },
    })
    await tick()
    expect(await pane.focusSignIn()).toBe(false)
    target.remove()
  })
})

describe('ErrorPane action row', () => {
  it('offers Try again whenever the backend set retryHint, not only on transient errors', async () => {
    // `retry_hint: true` is set on Serious errnos too (the `serious` helper in
    // `friendly_error/errno.rs` documents "retry hint on"), and on the NeedsAction
    // `emptyRootICloud`, whose doc comment promises a Try again button so the user
    // can re-list after granting access. Gating on `category === 'transient'` as
    // well silently swallowed all six.
    for (const category of ['transient', 'serious', 'needs_action'] as const) {
      const target = mountPane({ friendly: { ...base, category, retryHint: true } })
      await tick()
      expect(buttonTexts(target), `category=${category}`).toContain('Try again')
    }
  })

  it('hides Try again when the backend did not set retryHint', async () => {
    const target = mountPane({ friendly: { ...base, category: 'serious', retryHint: false } })
    await tick()
    expect(buttonTexts(target)).not.toContain('Try again')
  })

  it('always offers Go to home folder, even for an error with no CTA of its own', async () => {
    const target = mountPane({ friendly: { ...base, category: 'serious', retryHint: false } })
    await tick()
    expect(buttonTexts(target).some((t) => t.startsWith('Go to home folder'))).toBe(true)
  })

  it('offers Go back only when the tab has somewhere to go back to', async () => {
    const friendly: FriendlyError = { ...base, category: 'serious', retryHint: false }

    const without = mountPane({ friendly, canGoBack: false })
    await tick()
    expect(without.textContent).not.toContain('Go back')

    const with_ = mountPane({ friendly, canGoBack: true })
    await tick()
    expect(with_.textContent).toContain('Go back')
  })

  // David's QA (2026-11-26): a bucket the key may not list showed only Back and Home, so its
  // refusal screen had no way up to the bucket list.
  it('offers Go to parent folder where the folder has a way up, and runs it', async () => {
    const friendly: FriendlyError = {
      ...base,
      category: 'needs_action',
      retryHint: false,
      listingReason: 'objectStoreRefused',
    }

    const without = mountPane({ friendly, canGoUp: false })
    await tick()
    expect(buttonTexts(without).some((t) => t.startsWith('Go to parent folder'))).toBe(false)

    const onGoUp = vi.fn()
    const with_ = mountPane({ friendly, canGoUp: true, onGoUp })
    await tick()
    const button = [...with_.querySelectorAll('button')].find((b) =>
      b.textContent.trim().startsWith('Go to parent folder'),
    )
    button?.click()
    expect(onGoUp).toHaveBeenCalledOnce()
  })
})

describe('ErrorPane Send error report…', () => {
  // The other side said no on purpose, so there's no bug to report.
  it.each(['phoneRefused', 'serverRefused', 'remotePermissionDenied', 'objectStoreRefused'] as const)(
    'offers no report when %s',
    async (listingReason) => {
      const target = mountPane({ friendly: { ...base, category: 'needs_action', retryHint: false, listingReason } })
      await tick()
      expect(buttonTexts(target)).not.toContain('Send error report…')
    },
  )

  it('still offers a report for a local permission refusal, and for an untyped failure', async () => {
    const local = mountPane({
      friendly: { ...base, category: 'needs_action', retryHint: false, listingReason: 'permissionDenied' },
    })
    const untyped = mountPane({ friendly: { ...base, category: 'serious', retryHint: false } })
    await tick()
    expect(buttonTexts(local)).toContain('Send error report…')
    expect(buttonTexts(untyped)).toContain('Send error report…')
  })

  it('offers a report naming the typed listing reason and the title it shows', async () => {
    const target = mountPane({
      friendly: { ...base, category: 'transient', retryHint: true, listingReason: 'networkDown' },
    })
    await tick()
    Array.from(target.querySelectorAll('button'))
      .find((b) => b.textContent.trim() === 'Send error report…')
      ?.click()
    expect(openErrorReportFor).toHaveBeenCalledWith({
      kind: 'listingFailed',
      reason: 'networkDown',
      title: 'Something went wrong',
    })
  })
})

/**
 * The details key is the error screen's while one is focused, even where the same chord means
 * something else: off macOS it's `Ctrl+Shift+D`, which a dev build also uses for the debug window.
 * The pane claims it in a capture-phase listener, so the document's bubble-phase dispatcher (which
 * would open the debug window) never sees it. With no error screen up, the chord is the
 * dispatcher's (`routes/(main)/global-keydown.test.ts`).
 */
describe.each([
  ['macos', { key: 'd', metaKey: true }],
  ['windows', { key: 'D', ctrlKey: true, shiftKey: true }],
  ['linux', { key: 'D', ctrlKey: true, shiftKey: true }],
] as const)('the technical details key on %s', (platform, init) => {
  it('toggles the details and never reaches the document dispatcher', async () => {
    const undoPlatform = withPlatform(platform)
    const dispatcher = vi.fn()
    document.addEventListener('keydown', dispatcher)
    try {
      const target = mountPane({ friendly: { ...base, category: 'transient', retryHint: true }, isFocused: true })
      await tick()
      const details = target.querySelector<HTMLDetailsElement>('details.technical-details')
      expect(details?.open).toBe(false)

      document.body.dispatchEvent(new KeyboardEvent('keydown', { ...init, bubbles: true }))
      await tick()

      expect(details?.open).toBe(true)
      expect(dispatcher).not.toHaveBeenCalled()
      target.remove()
    } finally {
      document.removeEventListener('keydown', dispatcher)
      undoPlatform()
    }
  })
})
