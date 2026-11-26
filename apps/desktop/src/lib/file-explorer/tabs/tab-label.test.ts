import { describe, it, expect, vi } from 'vitest'
import type { NetworkHost } from '../types'
import { deriveTabLabel, tabLabelFor } from './tab-label'

const network = vi.hoisted(() => ({ hosts: [] as NetworkHost[] }))
vi.mock('../network/network-store.svelte', () => ({
  getNetworkHosts: () => network.hosts,
  getResolvedShareListHost: () => undefined,
}))

describe("tabLabelFor: a server's share list", () => {
  it('is called what the server is, ❌ not its path`s host key', () => {
    network.hosts = [{ id: 'h1', name: "Test_user_1's NAS", port: 445, source: 'discovered' }]
    expect(tabLabelFor({ volumeId: 'network', path: "smb://test_user_1's nas" })).toBe("Test_user_1's NAS")
  })

  it('falls back to the key for a server the app doesn’t know by name', () => {
    network.hosts = []
    expect(tabLabelFor({ volumeId: 'network', path: 'smb://testnas:4450' })).toBe('testnas')
  })

  it('leaves the Servers list and every other tab to `deriveTabLabel`', () => {
    expect(tabLabelFor({ volumeId: 'network', path: 'smb://' })).toBe('Servers')
    expect(tabLabelFor({ volumeId: 'root', path: '/Users/test_user_1/Documents' })).toBe('Documents')
  })
})

describe('deriveTabLabel', () => {
  /** ❗ The Servers view's path is the `smb://` sentinel, whose "basename" read "/" (QA round 2). */
  it('names the Servers view "Servers"', () => {
    expect(deriveTabLabel('smb://')).toBe('Servers')
  })

  it('shows "/" at an MTP storage root instead of the raw storage id', () => {
    // At the MTP storage root the last path segment is the raw storage id
    // (here 65537 = 0x10001 for Internal Storage), which used to surface as
    // the tab title "65537". A storage root carries no user-meaningful
    // basename, so the tab shows "/" — the same thing a local filesystem
    // root shows.
    expect(deriveTabLabel('mtp://0-5/65537')).toBe('/')
  })

  it('shows the folder name for an MTP subfolder', () => {
    expect(deriveTabLabel('mtp://0-5/65537/DCIM/Camera')).toBe('Camera')
    expect(deriveTabLabel('mtp://0-5/65537/DCIM')).toBe('DCIM')
  })

  it('shows "/" at an ADB device root and the folder name below it', () => {
    expect(deriveTabLabel('adb://R58M12345')).toBe('/')
    expect(deriveTabLabel('adb://R58M12345/sdcard/DCIM')).toBe('DCIM')
  })

  it('shows "/" for the local filesystem root (convention pinned)', () => {
    expect(deriveTabLabel('/')).toBe('/')
  })

  it('shows the basename for a local subfolder (convention pinned)', () => {
    expect(deriveTabLabel('/Users/test_user_14/Documents')).toBe('Documents')
  })

  it('shows the volume folder name for a mounted-volume root (convention pinned)', () => {
    // A mounted volume root like /Volumes/USB keeps its basename ("USB"),
    // unchanged from the local convention — we only special-case the MTP
    // storage root, not every volume root.
    expect(deriveTabLabel('/Volumes/USB')).toBe('USB')
  })

  /** ❗ `public` mounted at `/Volumes/public-1` (another server held `public`) named its tab "public-1" (QA round 3). */
  it('names an SMB share`s root after the share, whatever mount dir it got', () => {
    const share = { path: '/Volumes/public-1', rootLabel: 'public' }
    expect(deriveTabLabel('/Volumes/public-1', share)).toBe('public')
    // Below the root, the folder is the label as anywhere else.
    expect(deriveTabLabel('/Volumes/public-1/photos', share)).toBe('photos')
    expect(deriveTabLabel('/Volumes/USB', { path: '/Volumes/USB', rootLabel: null })).toBe('USB')
  })
})
