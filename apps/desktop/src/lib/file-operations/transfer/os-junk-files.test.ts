import { describe, it, expect } from 'vitest'
import { defaultOsJunkFiles, osJunkFilesFor, osJunkLinkKey, osJunkToggleStartsOn } from './os-junk-files'

describe('the “skip system clutter files” setting', () => {
  it('ticks the toggle only for a skip default; “Always ask” starts it off', () => {
    expect(osJunkToggleStartsOn('skip')).toBe(true)
    expect(osJunkToggleStartsOn('include')).toBe(false)
    expect(osJunkToggleStartsOn('ask')).toBe(false)
  })

  it('sends what the toggle says', () => {
    expect(osJunkFilesFor(true)).toBe('skip')
    expect(osJunkFilesFor(false)).toBe('include')
  })

  it('includes them when nobody saw the toggle, unless the default is to skip', () => {
    expect(defaultOsJunkFiles('skip')).toBe('skip')
    expect(defaultOsJunkFiles('include')).toBe('include')
    expect(defaultOsJunkFiles('ask')).toBe('include')
  })

  it('offers to stop asking, or names the default it started from', () => {
    expect(osJunkLinkKey('ask')).toBe('fileOperations.transferDialog.osJunkFiles.dontAskAgain')
    expect(osJunkLinkKey('include')).toBe('fileOperations.transferDialog.osJunkFiles.includingByDefault')
    expect(osJunkLinkKey('skip')).toBe('fileOperations.transferDialog.osJunkFiles.skippingByDefault')
  })
})
