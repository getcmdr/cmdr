/**
 * The Servers list (`network` at `smb://`) rendered by the plain Full view, from its kind's
 * words alone (`pane/list-kind-words.ts`): Name, Details, Last used, no Size; each server
 * drawn as its glyph (an SMB machine, any other server); its status as a dot on the icon,
 * whose words the row's accessible name says. ❌ No view knows it's showing servers.
 *
 * Widths come from a fake measurer (7 px per character).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { dirEntry, mountFullList } from './test-full-list'
import { _setMeasureForTests } from './measure-column-widths'
import type { ServerMeta } from '#lib/ipc/bindings.js'

vi.mock('#lib/tauri-commands/index.js', async () => (await import('./test-file-list-mocks')).tauriCommandsMock())
vi.mock('#lib/icon-cache.js', async () => (await import('./test-file-list-mocks')).iconCacheMock())
vi.mock('#lib/indexing/index-state.svelte.js', async () => (await import('./test-file-list-mocks')).indexStateMock())
vi.mock('#lib/settings/settings-store.js', async () => (await import('./test-file-list-mocks')).settingsStoreMock())
vi.mock('#lib/settings/reactive-settings.svelte.js', async () => ({
  ...(await import('./test-file-list-mocks')).reactiveSettingsMock(),
  getNetworkEnabled: () => true,
}))

import { listWordsFor } from '../pane/list-kind-words'

beforeEach(() => {
  _setMeasureForTests((text) => text.length * 7)
})
afterEach(() => {
  _setMeasureForTests(null)
})

function server(name: string, path: string, iconId: string, meta: Partial<ServerMeta>, modifiedAt?: number) {
  return dirEntry({
    name,
    path,
    iconId,
    modifiedAt,
    recursiveSize: undefined,
    serverMeta: {
      rowId: `row-${name}`,
      protocol: 'smb',
      address: '192.168.1.111',
      status: 'saved',
      account: null,
      saved: true,
      place: null,
      pinned: false,
      ...meta,
    },
  })
}

function headerSortLabels(target: HTMLElement): string[] {
  return [...target.querySelectorAll('.header-row .sortable-header .label')].map((l) => l.textContent)
}

describe('FullList on the Servers list', () => {
  it('shows Name, Details, and Last used, with no Size, each server as its glyph and status dot', async () => {
    const list = await mountFullList({
      entries: [
        server(
          'Naspolya',
          'smb://naspolya',
          'network:server-smb',
          { status: 'connected', account: { kind: 'user', username: 'test_user_1' } },
          1_790_000_000,
        ),
        server('Testhost', 'sftp://test_user_1@testhost:22', 'network:server', {
          protocol: 'sftp',
          address: 'testhost:22',
          place: 'sftp-1',
        }),
        server('Found box', 'smb://found-box', 'network:server-smb', { status: 'found_nearby', saved: false }),
      ],
      viewport: { clientHeight: 400, clientWidth: 900, offsetWidth: 900 },
      props: { listWords: listWordsFor('network') },
    })

    expect(list.rowNames()).toEqual(['Naspolya', 'Testhost', 'Found box'])
    expect(headerSortLabels(list.target)).toEqual(['Name', 'Details', 'Last used'])
    expect(list.target.querySelectorAll('.col-size').length).toBe(0)
    const details = [...list.target.querySelectorAll('.col-ext')].map((cell) => cell.textContent.trim())
    expect(details).toEqual(['SMB · 192.168.1.111 · as test_user_1', 'SFTP · testhost:22', 'SMB · 192.168.1.111'])

    const rows = list.rows()
    // The SMB machine wears the server-icon accent; any other server is the plain glyph.
    expect(rows[0].querySelector('.glyph-icon.is-machine')).toBeTruthy()
    expect(rows[1].querySelector('.glyph-icon:not(.is-machine)')).toBeTruthy()
    expect(rows[0].querySelector('.status-badge .smb-indicator-direct')).toBeTruthy()
    expect(rows[1].querySelector('.status-badge .smb-indicator-saved')).toBeTruthy()
    // A host Cmdr only found nearby has nothing to be live or idle about: no dot.
    expect(rows[2].querySelector('.status-badge')).toBeNull()

    // The dot is decorative; its words are in the row's name, and the date phrase is the column's own.
    const name = rows[0].getAttribute('aria-label') ?? ''
    expect(name).toContain('Connected')
    expect(name).toContain('SMB · 192.168.1.111 · as test_user_1')
    expect(name).toContain('(Last used)')
    expect(rows[2].getAttribute('aria-label')).toContain('Found nearby')
  })
})
