/**
 * Tier 3 a11y tests for the network surfaces that aren't listings: the OS-mount fallback toast
 * and the Local Network toast. The Servers list and a server's share list are file listings,
 * audited with the views (`../views/full-list.a11y.test.ts`); a Servers row's menu is
 * `ServersListRowMenu.a11y.test.ts`'s.
 *
 * One file per component would cost twice as much: `svelte-tests`
 * charges per test FILE, not per test (`docs/testing.md` § "What a test actually
 * costs"). Each block below keeps its component's own doc comment, props, and
 * assertions. Adding a server and signing in to one both live in the sign-in
 * sheet now; its own blocks are `#lib/servers/servers.a11y.test.ts`.
 */

import { describe, it, vi, afterEach } from 'vitest'
import { mount, tick } from 'svelte'
import SmbOsMountFallbackToastContent from './SmbOsMountFallbackToastContent.svelte'
import LocalNetworkBlockedToastContent from './LocalNetworkBlockedToastContent.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

vi.mock('./direct-connect', () => ({
  connectDirectly: vi.fn(() => Promise.resolve('connected')),
}))

// These components share one jsdom document, the dialog portals into
// `document.body`, and axe resolves ARIA id references document-wide. Clearing
// between tests keeps each audit looking at its own container only.
afterEach(() => {
  document.body.innerHTML = ''
})

describe('SmbOsMountFallbackToastContent a11y', () => {
  it('default state has no a11y violations', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(SmbOsMountFallbackToastContent, {
      target,
      props: { toastId: 'smb-os-mount:smb-archive', volumeId: 'smb-archive', share: 'archive', retryable: true },
    })
    await tick()
    await expectNoA11yViolations(target)
  })

  it('the this-Mac-blocked state, with its two buttons, has no a11y violations', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(SmbOsMountFallbackToastContent, {
      target,
      props: {
        toastId: 'smb-os-mount:smb-archive',
        volumeId: 'smb-archive',
        share: 'archive',
        retryable: true,
        blockedServer: 'Testnas',
      },
    })
    await tick()
    await expectNoA11yViolations(target)
  })
})

describe('LocalNetworkBlockedToastContent a11y', () => {
  it('default state has no a11y violations', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(LocalNetworkBlockedToastContent, { target, props: { server: 'Testnas', toastId: 'local-network-blocked' } })
    await tick()
    await expectNoA11yViolations(target)
  })
})
