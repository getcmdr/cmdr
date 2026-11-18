/**
 * Tier-3 tests for `LicenseSection.svelte`: a Personal install sees what its free license
 * covers and when it needs a commercial one, right above the two buttons; a licensed install
 * sees its own license facts and nothing that reads like an upsell.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, tick } from 'svelte'
import type { LicenseInfo, LicenseStatus } from '#lib/tauri-commands/index.js'

const { license } = vi.hoisted(() => ({
  license: {
    info: null as LicenseInfo | null,
    status: null as LicenseStatus | null,
  },
}))
vi.mock('#lib/tauri-commands/index.js', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  getLicenseInfo: vi.fn(() => Promise.resolve(license.info)),
  getLicenseStatus: vi.fn(() => Promise.resolve(license.status)),
  openExternalUrl: vi.fn(() => Promise.resolve()),
  emitExecuteCommand: vi.fn(() => Promise.resolve()),
}))

import LicenseSection from './LicenseSection.svelte'

async function mountSection(): Promise<HTMLElement> {
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(LicenseSection, { target })
  for (let i = 0; i < 5; i++) await Promise.resolve()
  await tick()
  return target
}

function buttonLabels(target: HTMLElement): string[] {
  return Array.from(target.querySelectorAll('.actions button')).map((b) => b.textContent.trim())
}

describe('LicenseSection', () => {
  beforeEach(() => {
    license.info = null
    license.status = { type: 'personal', showCommercialReminder: false }
  })

  it('lists what Personal includes and when work needs a commercial license, above the two buttons', async () => {
    const target = await mountSection()
    const items = Array.from(target.querySelectorAll('.included-list li')).map((li) => li.textContent.trim())
    expect(items).toEqual([
      'Every file manager feature',
      'All your own Macs, for your personal use',
      'Automatic updates',
      'Cmdr AI, free during the beta, on your own key or on-device',
    ])
    expect(target.querySelector('.commercial-line')?.textContent).toContain('needs a commercial license')
    expect(buttonLabels(target)).toEqual(['Enter license key', 'Get a license'])
    target.remove()
  })

  it('sizes its buttons like the other Settings buttons', async () => {
    const target = await mountSection()
    const buttons = Array.from(target.querySelectorAll('.actions button'))
    expect(buttons.length).toBe(2)
    for (const button of buttons) expect(button.classList.contains('btn-mini')).toBe(true)
    target.remove()
  })

  it('shows a licensed install its license, without the Personal list or the upsell line', async () => {
    license.info = {
      email: 'test_user_1@example.com',
      transactionId: 'txn_12345678',
      issuedAt: '2026-01-01T00:00:00Z',
      organizationName: null,
      licenseType: 'commercial_perpetual',
      shortCode: 'CMDR-AAAA-BBBB-CCCC',
      expiresAt: null,
    }
    license.status = {
      type: 'commercial',
      licenseType: 'commercial_perpetual',
      organizationName: null,
      expiresAt: null,
    }
    const target = await mountSection()
    expect(target.textContent).toContain('Commercial perpetual')
    expect(target.textContent).toContain('CMDR-AAAA-BBBB-CCCC')
    expect(target.querySelector('.included')).toBeNull()
    expect(target.querySelector('.commercial-line')).toBeNull()
    expect(buttonLabels(target)).toEqual(['Manage license key'])
    target.remove()
  })
})
