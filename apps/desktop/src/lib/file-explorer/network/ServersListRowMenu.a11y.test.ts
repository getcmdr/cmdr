/**
 * Tier 3 a11y tests for `ServersListRowMenu.svelte`: the Servers list's row menu host,
 * headless until a row's menu opens (its menu is `Menu`'s, named for the server).
 */

import { describe, it, vi } from 'vitest'
import { mount, tick } from 'svelte'
import ServersListRowMenu from './ServersListRowMenu.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

vi.mock('./network-store.svelte', () => ({ getNetworkHosts: () => [] }))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))
vi.mock('#lib/tauri-commands/index.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('#lib/tauri-commands/index.js')>()),
  listSavedServers: vi.fn(() => Promise.resolve([])),
  smbHostHasMounts: vi.fn(() => Promise.resolve(false)),
}))

describe('ServersListRowMenu a11y', () => {
  it('has no a11y violations while no row menu is open', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(ServersListRowMenu, { target, props: { onOpenChange: vi.fn(), onOpenRow: vi.fn() } })
    await tick()
    await expectNoA11yViolations(target)
  })
})
