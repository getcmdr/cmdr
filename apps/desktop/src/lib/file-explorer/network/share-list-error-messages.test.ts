/**
 * Every way a server's share list can stay empty has words, in the person's own
 * language, and they follow the error-copy rules.
 *
 * The record in `share-list-error-messages.ts` is exhaustive at the type level;
 * these catch a variant whose catalog key was never added (the key itself
 * renders), copy that breaks `docs/guides/error-handling.md`, and the backend's
 * diagnostic `message` leaking into what a person reads.
 */
import { describe, it, expect, beforeAll, afterAll } from 'vitest'
import type { ShareListError } from '#lib/ipc/bindings.js'
import { _setLocaleForTests } from '#lib/intl/locale.js'
import { renderShareListError } from './share-list-error-messages'
import { getListingErrorMessage, type ListingErrorReason } from '#lib/error-messages/listing-error-messages.js'
import { offersErrorReport } from '#lib/error-messages/listing-error.js'

beforeAll(() => {
  _setLocaleForTests('en-US')
})
afterAll(() => {
  _setLocaleForTests(null)
})

/** The backend's own diagnostic, which must never reach the screen. */
const DIAGNOSTIC = 'smbutil failed: Connection refused (os error 61)'

/** One value per `ShareListError` variant. A mapped type, so a new variant stops this compiling until it's here. */
const CASES: { [K in ShareListError['type']]: Extract<ShareListError, { type: K }> } = {
  host_unreachable: { type: 'host_unreachable', message: DIAGNOSTIC },
  timeout: { type: 'timeout', message: DIAGNOSTIC },
  auth_required: { type: 'auth_required', message: DIAGNOSTIC },
  signing_required: { type: 'signing_required', message: DIAGNOSTIC },
  auth_failed: { type: 'auth_failed', message: DIAGNOSTIC },
  signature_invalid: { type: 'signature_invalid', message: DIAGNOSTIC },
  negotiation_invalid: { type: 'negotiation_invalid', message: DIAGNOSTIC },
  encryption_unavailable: { type: 'encryption_unavailable', message: DIAGNOSTIC },
  encryption_required: { type: 'encryption_required', message: DIAGNOSTIC },
  protocol_error: { type: 'protocol_error', message: DIAGNOSTIC },
  resolution_failed: { type: 'resolution_failed', message: DIAGNOSTIC },
  missing_dependency: { type: 'missing_dependency', message: DIAGNOSTIC, installCommand: 'sudo apt install smbclient' },
}

const ALL: ShareListError[] = Object.values(CASES)

describe('renderShareListError', () => {
  it('gives every variant a sentence from the catalog', () => {
    for (const error of ALL) {
      const message = renderShareListError(error, 'Testnas')
      expect(message, `${error.type} must have words`).not.toBe('')
      expect(message, `${error.type} must resolve, not echo its key`).not.toMatch(/^errors\./)
      expect(message, `${error.type} must not leave a placeholder unfilled`).not.toMatch(/\{[a-zA-Z]+\}/)
    }
  })

  it('follows the error-copy writing rules', () => {
    for (const error of ALL) {
      const message = renderShareListError(error, 'Testnas')
      const lower = message.toLowerCase()
      expect(lower, `${error.type} must not say "error"`).not.toMatch(/\berrors?\b/)
      expect(lower, `${error.type} must not say "failed"`).not.toMatch(/\bfail(ed|ure)?\b/)
      for (const trivializer of ['just', 'simply', 'simple', 'easy']) {
        expect(lower, `${error.type} must not trivialize with "${trivializer}"`).not.toMatch(
          new RegExp(`\\b${trivializer}\\b`),
        )
      }
      expect(message, `${error.type} must not use an em dash`).not.toContain('—')
    }
  })

  it('gives each variant its own words', () => {
    const rendered = new Set(ALL.map((error) => renderShareListError(error, 'Testnas')))
    expect(rendered.size).toBe(ALL.length)
  })

  it('never shows the backend diagnostic, or the install command the pane shows on its own', () => {
    for (const error of ALL) {
      const message = renderShareListError(error, 'Testnas')
      expect(message, error.type).not.toContain('smbutil')
      expect(message, error.type).not.toContain('os error')
      expect(message, error.type).not.toContain('sudo')
    }
  })

  it('names the server for everything but a missing local tool', () => {
    for (const error of ALL.filter((e) => e.type !== 'missing_dependency')) {
      expect(renderShareListError(error, 'Testnas'), error.type).toContain('“Testnas”')
    }
  })
})

/** The variants the backend sends as the sign-in reason (`ShareListError::wants_sign_in`). */
const SIGN_IN_TYPES = new Set<ShareListError['type']>(['auth_required', 'signing_required', 'auth_failed'])

function asListingReason(error: ShareListError, server: string): ListingErrorReason {
  return SIGN_IN_TYPES.has(error.type)
    ? { reason: 'shareListSignIn', refusal: error, server }
    : { reason: 'shareListUnavailable', refusal: error, server }
}

describe('a share list refused as a listing', () => {
  it('gives every variant a title and the share list’s own sentence', () => {
    for (const error of ALL) {
      const message = getListingErrorMessage(asListingReason(error, 'Testnas'))
      expect(message.title, error.type).toContain('Testnas')
      expect(message.title, error.type).not.toMatch(/^(errors|fileExplorer)\./)
      expect(message.message, error.type).not.toBe('')
      expect(message.message, error.type).not.toMatch(/^(errors|fileExplorer)\./)
      expect(message.message, error.type).not.toContain('smbutil')
      if (error.type !== 'auth_required') {
        expect(message.message, `delegates to the share list's sentence: ${error.type}`).toBe(
          renderShareListError(error, 'Testnas'),
        )
      }
    }
  })

  it('words a server that wants an account as a sign-in, ❌ never as a failure', () => {
    const signIn = getListingErrorMessage(asListingReason(CASES.auth_required, 'Testnas'))
    expect(signIn.title).toBe('Sign in to Testnas')
    expect(signIn.message).toBe('This server shows its shares once you sign in.')

    const down = getListingErrorMessage(asListingReason(CASES.host_unreachable, 'Testnas'))
    expect(down.title).toBe('Couldn’t connect to Testnas')
  })

  // The explanation renders as markdown, so a name like `Test_user_1's NAS` must not
  // italicize half of itself.
  it('escapes the server name for the markdown it lands in', () => {
    const message = getListingErrorMessage(asListingReason(CASES.host_unreachable, 'Test_user_1’s NAS'))
    expect(message.message).toContain('Test&#95;user&#95;1’s NAS')
    expect(message.title, 'the title is plain text').toContain('Test_user_1’s NAS')
  })

  it('offers no error report for a sign-in, and one for the server’s other answers', () => {
    expect(offersErrorReport('shareListSignIn')).toBe(false)
    expect(offersErrorReport('shareListUnavailable')).toBe(true)
  })
})
