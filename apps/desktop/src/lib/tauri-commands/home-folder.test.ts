/**
 * `getHomeFolder`: one backend read shared by every caller, and a broken bridge that
 * doesn't stick. The test setup stands a fake in for this module everywhere else.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.unmock('#lib/tauri-commands/home-folder.js')
vi.mock('#lib/ipc/bindings.js', () => ({
  commands: { getHomeFolder: vi.fn() },
}))

import { commands } from '#lib/ipc/bindings.js'

const backend = vi.mocked(commands.getHomeFolder)

async function freshModule(): Promise<typeof import('./home-folder')> {
  vi.resetModules()
  return import('./home-folder')
}

beforeEach(() => {
  backend.mockReset()
})

describe('getHomeFolder', () => {
  it('answers the backend app path and asks only once', async () => {
    backend.mockResolvedValue('C:/Users/test_user_1')
    const { getHomeFolder } = await freshModule()

    expect(await getHomeFolder()).toBe('C:/Users/test_user_1')
    expect(await getHomeFolder()).toBe('C:/Users/test_user_1')
    expect(backend).toHaveBeenCalledTimes(1)
  })

  it('answers empty when the OS names no home folder', async () => {
    backend.mockResolvedValue(null)
    const { getHomeFolder } = await freshModule()

    expect(await getHomeFolder()).toBe('')
  })

  it('asks again after the bridge failed', async () => {
    backend.mockRejectedValueOnce(new Error('bridge down')).mockResolvedValueOnce('/Users/test_user_1')
    const { getHomeFolder } = await freshModule()

    await expect(getHomeFolder()).rejects.toThrow('bridge down')
    expect(await getHomeFolder()).toBe('/Users/test_user_1')
  })
})
