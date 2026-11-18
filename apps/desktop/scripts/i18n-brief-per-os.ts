/**
 * The brief's per-key note for a per-OS `{platform, select, …}` value: which system reads each of
 * the key's own branches, where each system's words live in the reference pile, and that the
 * branch macOS reads keeps its translation. The rule it applies is
 * `docs/i18n/translator-instructions.md` (Per-OS values); this only maps one key onto it.
 */
import { parsePlatformSelect } from '../src/lib/intl/platform-select.ts'
import { RENDERED_PLATFORMS, platformBranches } from './i18n-platform-branches.ts'
import type { RenderedPlatform } from './i18n-platform-branches.ts'

/** A system's name in the note, and the pile folders its own words come from. */
interface OsSource {
  name: string
  pile: string
}

/** Each system's name, and the pile folders its own words come from. */
const OS_SOURCES: Readonly<Record<RenderedPlatform, OsSource>> = {
  macos: { name: 'macOS', pile: '`macOS/`' },
  windows: { name: 'Windows', pile: '`microsoft-terminology/`, `microsoft-style-guides/`' },
  linuxGnome: { name: 'Linux on GNOME', pile: '`gnome-nautilus/`' },
  linuxKde: { name: 'Linux on KDE', pile: '`kde-dolphin/`' },
  linux: {
    name: 'other Linux desktops',
    pile: '`xfce-thunar/`, plus the word `gnome-nautilus/` and `kde-dolphin/` share',
  },
}

/** Every Linux desktop reading one branch is plain "Linux", mining all three Linux catalogs. */
const WHOLE_LINUX: OsSource = { name: 'Linux', pile: '`gnome-nautilus/`, `kde-dolphin/`, `xfce-thunar/`' }

const LINUX_READERS: readonly RenderedPlatform[] = ['linuxGnome', 'linuxKde', 'linux']

/** The systems reading one branch, Linux folded into one name when every desktop reads it. */
function sourcesOf(readers: readonly RenderedPlatform[]): OsSource[] {
  const wholeLinux = LINUX_READERS.every((reader) => readers.includes(reader))
  return readers.flatMap((reader) => {
    if (!wholeLinux || !LINUX_READERS.includes(reader)) return [OS_SOURCES[reader]]
    return reader === 'linuxGnome' ? [WHOLE_LINUX] : []
  })
}

/**
 * The note, or `undefined` for a plain value. A block without a branch for some system shows
 * `other` there, so `other`'s readers depend on which branches the key has.
 * @param compact the short form for every per-OS key after a brief's first: which system reads
 *   each branch and which one macOS reads, without the pile folders and the frame rule the first
 *   key's note already gave (88 full notes were 8% of a 110k-token brief)
 */
export function perOsNote(value: string, compact = false): string | undefined {
  const parsed = parsePlatformSelect(value)
  if (parsed === null) return undefined
  const parts = platformBranches(value).map(({ name }) => {
    const readers = RENDERED_PLATFORMS.filter((reader) => {
      const own = reader === 'linux' ? undefined : parsed[reader]
      return own === undefined ? name === 'other' : name === reader
    })
    const sources = sourcesOf(readers)
    const who = sources.map((source) => source.name).join(' and ')
    const piles = sources.map((source) => source.pile).join('; ')
    const macosReads = readers.includes('macos')
    if (compact) return `\`${String(name)}\` is ${who}${macosReads ? ' (macOS reads it: keep)' : ''}`
    const keep = macosReads ? ', and macOS reads it, so a translation already there stays' : ''
    return `\`${String(name)}\` is ${who} (pile: ${piles})${keep}`
  })
  if (compact) return `Per-OS value: ${parts.join('; ')}.`
  return `Per-OS value (see the Per-OS values instruction): ${parts.join('; ')}. Keep the \`{platform, select, …}\` frame and the branch names exactly as they are.`
}
