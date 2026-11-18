/**
 * Tests for `pnpm i18n:values` (`i18n-values.ts`) over a temp catalog root with
 * `en`, a full translation (`fr`), its overlay-shaped sibling (`es` + `es-419`).
 *
 * The load-bearing promises: every English branch prints with each locale's text
 * for that OS, a branch still in English and a stale key are marked, and an
 * overlay shows its base beside it, or that it inherits.
 */
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { sourceHash } from './i18n-catalog-lib.ts'
import { parseValuesArgs, runValues } from './i18n-values.ts'

let root: string
const TITLE = '{platform, select, windows {Settings} linuxGnome {Preferences} other {Settings}}'

function writeLocale(locale: string, messages: Record<string, unknown>) {
  mkdirSync(join(root, locale), { recursive: true })
  writeFileSync(join(root, locale, 'settings.json'), JSON.stringify(messages, null, 2))
}

function run(...argv: string[]): string {
  const lines: string[] = []
  runValues([...argv, '--messages-root', root], (text) => void lines.push(text))
  return lines.join('\n')
}

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), 'cmdr-i18n-values-'))
  writeLocale('en', { 'settings.window.title': TITLE, 'settings.save': 'Save', 'settings.cancel': 'Cancel' })
  writeLocale('fr', {
    'settings.window.title': '{platform, select, windows {Réglages} linuxGnome {Preferences} other {Réglages}}',
    '@settings.window.title': { sourceHash: 'abcdef0' },
    'settings.save': 'Enregistrer',
    '@settings.save': { sourceHash: sourceHash('Save') },
    'settings.cancel': 'Annuler',
    '@settings.cancel': { sourceHash: sourceHash('Cancel') },
  })
  writeLocale('es', {
    'settings.window.title': '{platform, select, windows {Ajustes} linuxGnome {Preferencias} other {Ajustes}}',
    '@settings.window.title': { sourceHash: sourceHash(TITLE) },
    'settings.save': 'Guardar',
    '@settings.save': { sourceHash: sourceHash('Save') },
    'settings.cancel': 'Cancelar',
    '@settings.cancel': { sourceHash: sourceHash('Cancel') },
  })
  writeLocale('es-419', {
    'settings.window.title': '{platform, select, windows {Configuración} linuxGnome {Preferencias} other {Ajustes}}',
    '@settings.window.title': {
      sourceHash: sourceHash('{platform, select, windows {Ajustes} linuxGnome {Preferencias} other {Ajustes}}'),
    },
  })
})

afterAll(() => {
  rmSync(root, { recursive: true, force: true })
})

describe('pnpm i18n:values', () => {
  it('prints each English branch with every locale’s text there, marking stale keys and pending English', () => {
    const out = run('--lang', 'fr,es', '--keys', 'settings.window.title')
    expect(out).toContain('## settings.window.title  (stale: fr)')
    expect(out).toContain('- windows\n  en  Settings\n  fr  Réglages\n  es  Ajustes')
    expect(out).toContain('- linuxGnome\n  en  Preferences\n  fr  Preferences  (= English)\n  es  Preferencias')
  })

  it('prints a plain key without branch headers, and takes globs', () => {
    const out = run('--lang', 'fr', '--keys', 'settings.s*')
    expect(out).toBe('## settings.save\nen  Save\nfr  Enregistrer\n')
  })

  it('shows an overlay’s base beside it, and says where it inherits', () => {
    const out = run('--lang', 'es-419', '--keys', 'settings.window.title,settings.cancel')
    expect(out).toContain('- windows\n  en      Settings\n  es      Ajustes\n  es-419  Configuración')
    expect(out).toContain('## settings.cancel\nen      Cancel\nes      Cancelar\nes-419  (inherits es)')
  })

  it('picks every stale key with --stale', () => {
    const out = run('--lang', 'all', '--stale')
    expect(out.match(/^## /gm)).toHaveLength(1)
    expect(out).toContain('## settings.window.title  (stale: fr)')
  })

  it('needs a language and a key selector', () => {
    expect(() => parseValuesArgs(['--keys', 'a.b'])).toThrow('--lang')
    expect(() => parseValuesArgs(['--lang', 'fr'])).toThrow('Pick keys')
    expect(() => run('--lang', 'xx', '--keys', 'settings.save')).toThrow('No catalog: xx')
    expect(() => run('--lang', 'fr', '--keys', 'settings.nope')).toThrow('No English key matches')
  })
})
