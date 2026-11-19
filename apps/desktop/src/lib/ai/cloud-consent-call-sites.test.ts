/**
 * "Nothing reaches a cloud AI service until the user turns on Allow cloud AI" holds only if
 * nothing but that switch's click can record the consent. The backend can't tell a click from
 * any other caller of `accept_cloud_ai_consent`, so the frontend's call sites are the guard,
 * and this census pins them (`lib/ai/DETAILS.md` § Cloud AI consent).
 *
 * A source scan because the invariant is about the whole frontend, not one component: a new
 * surface that grants consent as a side effect (onboarding's Cloud pick, a "turn on" button in a
 * gate) is exactly the regression, and no mount of an existing component would catch it.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
// The module whose accept this pins; imported so the census is about real source, not a string.
import * as cloudConsent from './cloud-consent.svelte'

/** Vitest's root is `apps/desktop` (see `vitest.config.ts`). */
const SRC = resolve(process.cwd(), 'src') + '/'

function sourceFiles(dir: string, found: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) {
      sourceFiles(full, found)
    } else if (/\.(ts|svelte)$/.test(entry) && !/\.test\.ts$/.test(entry)) {
      found.push(full)
    }
  }
  return found
}

function filesMentioning(identifier: string): string[] {
  const pattern = new RegExp(`\\b${identifier}\\b`)
  return sourceFiles(SRC)
    .filter((file) => pattern.test(readFileSync(file, 'utf8')))
    .map((file) => file.slice(SRC.length))
    .sort()
}

describe('only the Allow cloud AI switch grants cloud consent', () => {
  it('acceptCloudConsent is defined in the state module and called from the switch alone', () => {
    expect(Object.keys(cloudConsent)).toContain('acceptCloudConsent')
    expect(filesMentioning('acceptCloudConsent')).toEqual([
      'lib/ai/AiCloudConsentToggle.svelte',
      'lib/ai/cloud-consent.svelte.ts',
    ])
  })

  it('the raw IPC wrapper is reached only through that state module', () => {
    expect(filesMentioning('acceptCloudAiConsent')).toEqual([
      'lib/ai/cloud-consent.svelte.ts',
      // Names it only as a key of the demo's "left out" list (no AI in a page), never calls it.
      'lib/demo/coverage.ts',
      'lib/ipc/bindings.ts',
      'lib/tauri-commands/ai.ts',
      'lib/tauri-commands/index.ts',
    ])
  })
})
