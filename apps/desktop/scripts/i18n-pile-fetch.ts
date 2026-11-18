#!/usr/bin/env node
/**
 * `pnpm i18n:pile-fetch`: puts the GNOME and KDE desktop catalogs that Cmdr's
 * `linuxGnome` / `linuxKde` branches cite into the reference pile, beside the
 * file managers it already has. Three translator waves fetched these by hand
 * (`curl` from l10n.gnome.org and websvn.kde.org) because the pile carried only
 * Nautilus and Dolphin, so a Linux-desktop branch had nothing to cite for
 * "Passwords and Keys", "KDE Wallet", KStandardAction's "Configure %1…", or the
 * KCM names. `pnpm i18n:pile` reads both new folders as Linux (Tier 3) evidence.
 *
 *   <pile>/<folder>/gnome-desktop/<module>.po   seahorse, gnome-keyring, gnome-control-center
 *   <pile>/<folder>/kde-desktop/<catalog>.po    kconfigwidgets6, kxmlgui6, kwalletd6, kwallet._desktop_,
 *                                               kwalletmanager, kwalletmanager._desktop_, systemsettings,
 *                                               kcm_keyboard, kcm_keys, plasma-desktop._json_
 *
 *   pnpm i18n:pile-fetch [--lang <tag>[,<tag>…]|all]   (default: every full translation Cmdr ships)
 *
 * Re-runnable; overwrites. The pile is gitignored and lives only in the main
 * clone (`pileRoot()`), so this writes there from any worktree. GNOME and KDE
 * catalogs are GPL/LGPL: evidence for picking a term, never text to paste
 * (`docs/i18n/reference-pile/README.md`). Exit 0 when every catalog arrived, 1
 * when any is missing (the rest still land), 2 on a usage error.
 */

import { existsSync, mkdirSync, renameSync, symlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { pileRoot } from './i18n-repo-lib.ts'

/** Cmdr's full translations: the tags whose pile folders get the desktop catalogs. */
export const DESKTOP_PILE_TAGS: readonly string[] = Object.freeze([
  'cs',
  'de',
  'es',
  'fr',
  'hu',
  'nl',
  'pt',
  'ru',
  'sv',
  'uk',
  'vi',
  'zh',
  'zh-Hant',
])

/** GNOME modules, from l10n.gnome.org's POT export of the `main` branch. */
export const GNOME_MODULES: readonly string[] = Object.freeze(['seahorse', 'gnome-keyring', 'gnome-control-center'])

/** KDE catalogs, as `<repo>/<catalog>` under l10n-kf6's `messages/`. KCM names live in `_json_`, not a KCM's own .po. */
export const KDE_CATALOGS: readonly string[] = Object.freeze([
  'kconfigwidgets/kconfigwidgets6',
  'kxmlgui/kxmlgui6',
  'kwallet/kwalletd6',
  'kwallet/kwallet._desktop_',
  'kwalletmanager/kwalletmanager',
  'kwalletmanager/kwalletmanager._desktop_',
  'systemsettings/systemsettings',
  'plasma-desktop/kcm_keyboard',
  'plasma-desktop/kcm_keys',
  'plasma-desktop/plasma-desktop._json_',
])

/** The pile subfolders this writes, which `i18n-pile-lib.ts` reads as Linux evidence. */
export const DESKTOP_PILE_SOURCES: readonly string[] = Object.freeze(['gnome-desktop', 'kde-desktop'])

/**
 * Where a tag's Linux sources live in the pile, and the upstream language code.
 * `zh` ships Simplified, whose Linux sources sit in `zh-CN/` (`zh/` symlinks to
 * them, `zh/_see-also.txt`); `zh-Hant` reads `zh-TW/`; `pt` ships Brazilian.
 */
export function desktopTarget(tag: string): { folder: string; code: string; alias?: string } {
  if (tag === 'pt') return { folder: 'pt-BR', code: 'pt_BR' }
  if (tag === 'zh') return { folder: 'zh-CN', code: 'zh_CN', alias: 'zh' }
  if (tag === 'zh-Hant') return { folder: 'zh-TW', code: 'zh_TW' }
  return { folder: tag, code: tag.replace('-', '_') }
}

/** One catalog to fetch: where from, and where it lands relative to the pile root. */
export interface DesktopCatalog {
  url: string
  path: string
}

/** Every catalog for one tag. websvn's `?view=co` form serves the file; its `*checkout*` form 404s. */
export function desktopCatalogs(tag: string): DesktopCatalog[] {
  const { folder, code } = desktopTarget(tag)
  return [
    ...GNOME_MODULES.map((module) => ({
      url: `https://l10n.gnome.org/POT/${module}.main/${module}.main.${code}.po`,
      path: join(folder, 'gnome-desktop', `${module}.po`),
    })),
    ...KDE_CATALOGS.map((catalog) => ({
      url: `https://websvn.kde.org/trunk/l10n-kf6/${code}/messages/${catalog}.po?view=co`,
      path: join(folder, 'kde-desktop', `${catalog.split('/')[1]}.po`),
    })),
  ]
}

/** Whether a fetched body is a gettext catalog, so an error page never lands in the pile. */
export function looksLikePo(body: string): boolean {
  return /^msgid "/m.test(body) && /^msgstr/m.test(body)
}

/** `--lang`: a comma list of shipped tags, or `all` (the default). */
export function parseFetchArgs(argv: string[]): { tags: string[] } {
  const { values } = parseArgs({ args: argv, options: { lang: { type: 'string' } } })
  const flag = values.lang?.trim()
  const tags = flag === undefined || flag === 'all' ? [...DESKTOP_PILE_TAGS] : flag.split(',').map((tag) => tag.trim())
  const unknown = tags.filter((tag) => !DESKTOP_PILE_TAGS.includes(tag))
  if (unknown.length > 0)
    throw new Error(`Not a full translation: ${unknown.join(', ')} (have: ${DESKTOP_PILE_TAGS.join(', ')})`)
  return { tags }
}

/** Fetches every catalog for `tags` into the pile at `root`. Returns the URLs that didn't arrive. */
async function fetchAll(root: string, tags: readonly string[]): Promise<string[]> {
  const missing: string[] = []
  for (const tag of tags) {
    const { folder, alias } = desktopTarget(tag)
    for (const { url, path } of desktopCatalogs(tag)) {
      const out = join(root, path)
      mkdirSync(join(out, '..'), { recursive: true })
      const response = await fetch(url).catch(() => undefined)
      const body = response?.ok ? await response.text() : ''
      if (!looksLikePo(body)) {
        missing.push(url)
        continue
      }
      writeFileSync(`${out}.part`, body)
      renameSync(`${out}.part`, out)
    }
    if (alias !== undefined) {
      for (const source of DESKTOP_PILE_SOURCES) {
        const link = join(root, alias, source)
        if (!existsSync(link)) symlinkSync(join('..', folder, source), link)
      }
    }
    console.log(`${tag}: ${folder}/{${DESKTOP_PILE_SOURCES.join(',')}}`)
  }
  return missing
}

// Run as a CLI (not when imported by tests).
if (import.meta.url === `file://${process.argv[1]}`) {
  let tags: string[]
  try {
    tags = parseFetchArgs(process.argv.slice(2)).tags
  } catch (err) {
    console.error(err instanceof Error ? err.message : String(err))
    process.exit(2)
  }
  const root = pileRoot()
  if (!existsSync(root)) {
    console.error(`No reference pile at ${root}: build it first (docs/i18n/reference-pile/README.md).`)
    process.exit(2)
  }
  const missing = await fetchAll(root, tags)
  if (missing.length > 0) {
    console.error(`Didn't arrive (${String(missing.length)}):\n${missing.map((url) => `  ${url}`).join('\n')}`)
    process.exit(1)
  }
  console.log('Every desktop catalog arrived.')
}
