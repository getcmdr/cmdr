/**
 * IPC contract tests for the network/SMB connection surface: `connect_to_server` and
 * `mount_network_share`.
 *
 * MTP and SMB stood out in the coverage report as nearly-zero IPC coverage. The
 * underlying smb2 / mDNS logic is tested in its own crate; what this group catches
 * is the **wire format**, especially the many-positional-arg shapes that AGENTS.md
 * specifically calls out as fragile (the `mountNetworkShare(server, share, username,
 * password, port, timeoutMs)` signature has 6 positional args; getting the order
 * wrong silently breaks at runtime).
 */

import { afterEach, describe, expect, it } from 'vitest'

import { commands } from '#lib/ipc/bindings.js'
import type { ManualConnectResult, MountResult } from '#lib/ipc/bindings.js'
import { clearIpcMocks, installIpcMock } from '#lib/ipc/test-helpers.js'

afterEach(() => {
  clearIpcMocks()
})

describe('commands.connectToServer', () => {
  it('forwards address as a single payload key', async () => {
    const ipc = installIpcMock()
    const result: ManualConnectResult = {
      host: {
        id: 'manual-1',
        name: 'storage.local',
        hostname: 'storage.local',
        ipAddress: '192.168.1.42',
        port: 445,
      },
      sharePath: null,
    }
    ipc.mock('connect_to_server', () => result)

    const out = await commands.connectToServer('smb://storage.local/share', null, null, true)

    expect(out).toEqual({ status: 'ok', data: result })
    expect(ipc.lastCall('connect_to_server')?.payload).toEqual({
      address: 'smb://storage.local/share',
      name: null,
      username: null,
      checkReachability: true,
    })
  })

  it('surfaces a typed refusal on unreachable hosts', async () => {
    const ipc = installIpcMock()
    ipc.mock('connect_to_server', () => {
      throw { type: 'unreachable', message: 'host unreachable' }
    })

    const out = await commands.connectToServer('smb://nonexistent.invalid', null, null, true)

    expect(out.status).toBe('error')
    if (out.status === 'error') expect(out.error).toEqual({ type: 'unreachable', message: 'host unreachable' })
  })
})

describe('commands.mountNetworkShare', () => {
  it('sends the 6 positional args in declared order (server, share, username, password, port, timeoutMs)', async () => {
    const ipc = installIpcMock()
    const result: MountResult = { mountPath: '/Volumes/Public', alreadyMounted: false }
    ipc.mock('mount_network_share', () => result)

    const server = 'storage.local'
    const share = 'Public'
    const username: string | null = 'test_user_10'
    const password: string | null = 'hunter2'
    const port: number | null = 445
    const timeoutMs: number | null = 20000

    const hostName: string | null = 'Storage'
    const attemptId: string | null = 'server-connect-1'

    await commands.mountNetworkShare(server, share, username, password, port, timeoutMs, hostName, attemptId)

    expect(ipc.lastCall('mount_network_share')?.payload).toEqual({
      server,
      share,
      username,
      password,
      port,
      timeoutMs,
      hostName,
      attemptId,
    })
  })

  it('surfaces typed MountError variants (auth_failed) on the error branch', async () => {
    const ipc = installIpcMock()
    ipc.mock('mount_network_share', () => {
      throw { type: 'auth_failed', server: 's' }
    })

    const out = await commands.mountNetworkShare('s', 'sh', 'u', 'p', null, null, null, null)

    expect(out.status).toBe('error')
    if (out.status === 'error') {
      expect(out.error).toEqual({ type: 'auth_failed', server: 's' })
    }
  })
})
