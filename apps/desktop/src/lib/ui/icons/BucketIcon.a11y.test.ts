/**
 * Tier 3 a11y tests for `BucketIcon.svelte`.
 *
 * The S3 bucket glyph. A bare inline svg with no ARIA of its own: callers reach it through
 * `Icon` (`name="bucket"`), decorative beside the bucket's name in its account's bucket list,
 * or labelled where it stands alone.
 */
import { describe, it, expect } from 'vitest'
import { mount, tick } from 'svelte'
import BucketIcon from './BucketIcon.svelte'
import Icon from '../Icon.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
import { glyphIcon } from '#lib/glyph-icons.js'

function host(): HTMLElement {
  const target = document.createElement('div')
  document.body.appendChild(target)
  return target
}

describe('BucketIcon a11y', () => {
  it('decorative bucket glyph has no a11y violations', async () => {
    const target = host()
    mount(BucketIcon, { target, props: { width: 16, height: 16, 'aria-hidden': 'true' } })
    await tick()
    await expectNoA11yViolations(target)
  })

  it('through Icon, an unnamed glyph is hidden and a labelled one is an image with that name', async () => {
    const decorative = host()
    mount(Icon, { target: decorative, props: { name: 'bucket', size: 16 } })
    const labelled = host()
    mount(Icon, { target: labelled, props: { name: 'bucket', size: 16, 'aria-label': 'Bucket' } })
    await tick()

    expect(decorative.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true')
    expect(labelled.querySelector('svg')?.getAttribute('role')).toBe('img')
    await expectNoA11yViolations(labelled)
  })

  it('is what a bucket row’s reserved icon id draws, ❌ never an OS icon', () => {
    expect(glyphIcon('network:bucket')).toEqual({ name: 'bucket', machine: false })
  })
})
