/**
 * `parseActivationError` reads every code the backend's `LicenseActivationError` sends, so the
 * license dialog words each one, ❌ never the generic fallback for a code it forgot.
 */

import { describe, expect, it } from 'vitest'
import { parseActivationError } from './licensing'

describe('parseActivationError', () => {
  it('reads a busy server, as an object and as the JSON string Tauri may send', () => {
    expect(parseActivationError({ code: 'serverBusy', status: 503 })?.code).toBe('serverBusy')
    expect(parseActivationError(JSON.stringify({ code: 'serverBusy', status: 429 }))?.code).toBe('serverBusy')
  })

  it('reads a code it knows and refuses one it doesn’t', () => {
    expect(parseActivationError({ code: 'shortCodeNotFound' })?.code).toBe('shortCodeNotFound')
    expect(parseActivationError({ code: 'somethingNew' })).toBeNull()
  })
})
