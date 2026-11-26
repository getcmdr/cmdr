/**
 * Tier 3 a11y tests for `ShareListAccountChip.svelte`: the share list's account in the
 * path bar, a menu button ("as testuser") whose name says whose account it is.
 */

import { describe, it, vi } from 'vitest'
import { mount, tick } from 'svelte'
import ShareListAccountChip from './ShareListAccountChip.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

vi.mock('./network-store.svelte', () => ({
  getNetworkHosts: () => [
    { id: 'testnas-id', name: 'Testnas', hostname: 'testnas.local', port: 445, source: 'discovered' },
  ],
}))
vi.mock('./share-list-sign-in', () => ({
  runServerAccountAction: vi.fn(() => Promise.resolve({ listed: false })),
  serverAccountFacts: () => ({ listedAccount: { kind: 'user', username: 'testuser' }, guestListingWorked: false }),
}))

describe('ShareListAccountChip a11y', () => {
  it('has no a11y violations', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(ShareListAccountChip, {
      target,
      props: { path: 'smb://testnas', onListed: vi.fn(), onOpenChange: vi.fn(), onOpenShare: vi.fn() },
    })
    await tick()
    await expectNoA11yViolations(target)
  })
})
