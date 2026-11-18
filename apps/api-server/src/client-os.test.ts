import { describe, expect, it } from 'vitest'
import { osOfRow, parseClientOs } from './client-os'

describe('parseClientOs', () => {
  it.each(['macos', 'windows', 'linux'])('keeps %s', (os) => {
    expect(parseClientOs(os)).toBe(os)
  })

  it.each([undefined, null, '', 'Windows', 'freebsd', 42, {}])('unsets %j rather than failing', (value) => {
    expect(parseClientOs(value)).toBeNull()
  })
})

describe('osOfRow', () => {
  it('trusts the os column when the client sent one', () => {
    expect(osOfRow({ os: 'windows', osVersion: 'macOS 26.0' })).toBe('windows')
  })

  it.each([
    ['macOS 26.0', 'macos'],
    ['macOS (unknown version)', 'macos'],
    ['15.3.1', 'macos'],
    ['Windows 10 Pro 22H2 (build 19045.6456)', 'windows'],
    ['Ubuntu 24.04 LTS', 'linux'],
    ['Linux (unknown distro)', 'linux'],
    ['Fedora Linux 41 (Workstation Edition)', 'linux'],
  ])('reads a legacy row with os_version %j as %s', (osVersion, os) => {
    expect(osOfRow({ os: null, osVersion })).toBe(os)
  })

  it('ignores an os column value it does not know', () => {
    expect(osOfRow({ os: 'freebsd', osVersion: 'macOS 26.0' })).toBe('macos')
  })
})
