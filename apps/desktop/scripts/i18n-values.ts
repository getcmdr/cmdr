#!/usr/bin/env node
/**
 * `pnpm i18n:values`: what these keys say right now in these locales, branch by
 * branch, beside the English. Read-only. A per-OS batch's brief repeats each
 * key's notes and diffs (an 89-key brief ran to 300 KB), so translators kept
 * writing a throwaway `dump.ts` or `node -e` to see just the current values; this
 * is that dump, shared.
 *
 *   pnpm i18n:values --lang <tag>[,<tag>…]|all  (--keys a.b,c.*  |  --keys-file <file>  |  --stale)
 *
 * `--keys` takes exact keys or `*` globs, like `i18n:brief`; `--stale` picks
 * every key one of the locales translated from other English. An overlay
 * (`es-419`, `en-GB`) is welcome: its base's text prints beside it, and a key it
 * doesn't fork reads "(inherits <base>)". Each line marks `stale` (the stored
 * hash names other English) and `= English` (a branch still pending
 * translation). Exit 0, or 2 on a usage error.
 */

import { parseArgs } from 'node:util'
import {
  BASE_LOCALE,
  layerCatalogs,
  listLocales,
  loadCatalog,
  resolveLocaleSource,
  sourceHash,
} from './i18n-catalog-lib.ts'
import type { Catalog } from './i18n-catalog-lib.ts'
import { selectKeys } from './i18n-brief-select.ts'
import { EXIT_CLEAN, EXIT_ERROR } from './i18n-locale-check-lib.ts'
import { branchReader, platformBranches } from './i18n-platform-branches.ts'
import { keyPatternsFrom } from './i18n-repo-lib.ts'
import { pickPlatformBranch } from '../src/lib/intl/platform-select.ts'

/** One locale as the report reads it. */
export interface ValuesLocale {
  tag: string
  catalog: Catalog
  /** what the locale's values were translated from: `en`, or for an overlay `en` with its base layered on */
  source: Catalog
  /** an overlay's base tag (`es` for `es-419`) */
  base?: string
}

/** A catalog value, or `undefined` where the catalog lacks the key (its index type says it never does). */
function valueOf(messages: Record<string, string>, key: string): string | undefined {
  return key in messages ? messages[key] : undefined
}

/** Whether `locale` translated `key` from other English than its source holds now. */
function isStale(locale: ValuesLocale, key: string): boolean {
  const sourceValue = valueOf(locale.source.messages, key)
  if (!(key in locale.catalog.messages) || sourceValue === undefined) return false
  const stored = key in locale.catalog.metadata ? locale.catalog.metadata[key].sourceHash : undefined
  return stored !== sourceHash(sourceValue)
}

/** The report: per key, per branch of the English, the English and each locale's text. */
export function valuesReport(en: Catalog, locales: readonly ValuesLocale[], keys: readonly string[]): string {
  const width = Math.max(BASE_LOCALE.length, ...locales.flatMap(({ tag, base }) => [tag.length, base?.length ?? 0]))
  const pad = (tag: string) => tag.padEnd(width)
  const out: string[] = []
  for (const key of keys) {
    const english = valueOf(en.messages, key)
    if (english === undefined) continue
    const stale = locales.filter((locale) => isStale(locale, key)).map(({ tag }) => tag)
    out.push(`## ${key}${stale.length > 0 ? `  (stale: ${stale.join(', ')})` : ''}`)
    for (const { name, text } of platformBranches(english)) {
      const indent = name === null ? '' : '  '
      if (name !== null) out.push(`- ${name}`)
      out.push(`${indent}${pad(BASE_LOCALE)}  ${text}`)
      const read = (value: string) => (name === null ? value : pickPlatformBranch(value, branchReader(name)))
      for (const locale of locales) {
        const value = valueOf(locale.catalog.messages, key)
        if (locale.base !== undefined) {
          const baseValue = valueOf(locale.source.messages, key)
          if (baseValue !== undefined && baseValue !== english) {
            out.push(`${indent}${pad(locale.base)}  ${read(baseValue)}`)
          }
        }
        if (value === undefined) {
          out.push(`${indent}${pad(locale.tag)}  ${locale.base ? `(inherits ${locale.base})` : '(missing)'}`)
          continue
        }
        const shown = read(value)
        out.push(`${indent}${pad(locale.tag)}  ${shown}${shown === text ? '  (= English)' : ''}`)
      }
    }
    out.push('')
  }
  return out.join('\n')
}

/** The parsed flags. */
export interface ValuesArgs {
  langs: string[] | 'all'
  keys?: string
  keysFile?: string
  stale: boolean
  messagesRoot?: string
}

/** Parses `--lang` plus one key selector. */
export function parseValuesArgs(argv: string[]): ValuesArgs {
  const { values } = parseArgs({
    args: argv,
    options: {
      lang: { type: 'string' },
      keys: { type: 'string' },
      'keys-file': { type: 'string' },
      stale: { type: 'boolean', default: false },
      'messages-root': { type: 'string' },
    },
  })
  const lang = values.lang?.trim()
  if (!lang) throw new Error('Pass --lang <tag>[,<tag>…] or --lang all')
  if (!values.keys && !values['keys-file'] && !values.stale) {
    throw new Error('Pick keys: --keys a.b,c.*, --keys-file <file>, and/or --stale')
  }
  return {
    langs: lang === 'all' ? 'all' : lang.split(',').map((tag) => tag.trim()),
    keys: values.keys,
    keysFile: values['keys-file'],
    stale: values.stale,
    messagesRoot: values['messages-root'],
  }
}

/** Loads each locale with what it was translated from. */
export function loadValuesLocales(langs: string[] | 'all', messagesRoot?: string): ValuesLocale[] {
  const available = listLocales(messagesRoot).filter((tag) => tag !== BASE_LOCALE)
  const tags = langs === 'all' ? available.filter((tag) => !resolveLocaleSource(tag, available).isOverlay) : langs
  const unknown = tags.filter((tag) => !available.includes(tag))
  if (unknown.length > 0) throw new Error(`No catalog: ${unknown.join(', ')} (have: ${available.join(', ')})`)
  const en = loadCatalog(BASE_LOCALE, messagesRoot)
  return tags.map((tag) => {
    const { overrides, isOverlay } = resolveLocaleSource(tag, [BASE_LOCALE, ...available])
    const catalog = loadCatalog(tag, messagesRoot)
    if (!isOverlay) return { tag, catalog, source: en }
    return { tag, catalog, source: layerCatalogs(en, loadCatalog(overrides, messagesRoot)), base: overrides }
  })
}

/** Runs the CLI; returns the exit code. */
export function runValues(argv: string[], write: (text: string) => void = console.log): number {
  const args = parseValuesArgs(argv)
  const en = loadCatalog(BASE_LOCALE, args.messagesRoot)
  const locales = loadValuesLocales(args.langs, args.messagesRoot)
  const patterns = keyPatternsFrom(args)
  const picked = patterns ? new Set(selectKeys({ en: en.messages, patterns })) : undefined
  const keys = Object.keys(en.messages).filter(
    (key) => picked?.has(key) || (args.stale && locales.some((locale) => isStale(locale, key))),
  )
  write(keys.length === 0 ? 'No key matches.' : valuesReport(en, locales, keys))
  return EXIT_CLEAN
}

// Run as a CLI (not when imported by tests).
if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    process.exit(runValues(process.argv.slice(2)))
  } catch (err) {
    console.error(err instanceof Error ? err.message : String(err))
    process.exit(EXIT_ERROR)
  }
}
