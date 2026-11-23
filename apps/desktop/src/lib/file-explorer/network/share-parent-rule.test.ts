import { describe, expect, it } from 'vitest'
import { effectiveShow, overrideAfter } from './share-parent-rule'

describe('effectiveShow', () => {
  it('follows the default without an override', () => {
    expect(effectiveShow(null, true)).toBe(true)
    expect(effectiveShow(null, false)).toBe(false)
  })

  it('takes the share’s own choice over the default', () => {
    expect(effectiveShow(false, true)).toBe(false)
    expect(effectiveShow(true, false)).toBe(true)
  })
})

describe('overrideAfter', () => {
  it('stores nothing for a choice that matches the default', () => {
    expect(overrideAfter(true, true)).toBeNull()
    expect(overrideAfter(false, false)).toBeNull()
  })

  it('stores a choice that differs from the default', () => {
    expect(overrideAfter(false, true)).toBe(false)
    expect(overrideAfter(true, false)).toBe(true)
  })
})
