/**
 * Tier 3 a11y tests for `MonitorFilledIcon.svelte`.
 *
 * The SMB server glyph (a monitor with a filled screen). A bare inline svg with no ARIA of its
 * own: callers reach it through `Icon` (`name="monitor-filled"`), decorative beside the server's
 * name in a Servers row, or labelled where it stands alone.
 */
import { describe, it, expect } from 'vitest'
import { mount, tick } from 'svelte'
import MonitorFilledIcon from './MonitorFilledIcon.svelte'
import Icon from '../Icon.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

function host(): HTMLElement {
  const target = document.createElement('div')
  document.body.appendChild(target)
  return target
}

describe('MonitorFilledIcon a11y', () => {
  it('decorative server glyph has no a11y violations', async () => {
    const target = host()
    mount(MonitorFilledIcon, { target, props: { width: 16, height: 16, 'aria-hidden': 'true' } })
    await tick()
    await expectNoA11yViolations(target)
  })

  it('through Icon, an unnamed glyph is hidden and a labelled one is an image with that name', async () => {
    const decorative = host()
    mount(Icon, { target: decorative, props: { name: 'monitor-filled', size: 16 } })
    const labelled = host()
    mount(Icon, { target: labelled, props: { name: 'monitor-filled', size: 16, 'aria-label': 'SMB server' } })
    await tick()

    expect(decorative.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true')
    expect(labelled.querySelector('svg')?.getAttribute('role')).toBe('img')
    expect(labelled.querySelector('svg')?.getAttribute('aria-label')).toBe('SMB server')
    await expectNoA11yViolations(labelled)
  })
})
