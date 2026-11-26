/**
 * Icon ids the frontend draws as one of its own glyphs instead of asking the OS for an icon:
 * a Servers row's server (`network:server-smb`, `network:server`, set by
 * `src-tauri/src/network/servers_listing.rs`). An SMB server is a machine, the filled
 * monitor macOS draws for one, in the server-icon accent; any other server is a service on
 * one. `FileIcon.svelte` draws them and `prefetchIcons` never asks the OS for them.
 *
 * Its own module, apart from `icon-cache.ts`, for the reason `stacked-icons.ts` is: view
 * tests mock the cache wholesale, and this pure lookup has to keep answering under them.
 */

import type { IconName } from '#lib/ui/icons/icon-map.js'

/** A reserved id's glyph, and whether it's a machine (tinted like a server's icon). */
export interface GlyphIcon {
  name: IconName
  machine: boolean
}

const GLYPH_ICONS: ReadonlyMap<string, GlyphIcon> = new Map([
  ['network:server-smb', { name: 'monitor-filled', machine: true }],
  ['network:server', { name: 'server', machine: false }],
])

/** The glyph `iconId` is drawn as, or `undefined` for an id the OS (or another rule) answers. */
export function glyphIcon(iconId: string): GlyphIcon | undefined {
  return GLYPH_ICONS.get(iconId)
}
