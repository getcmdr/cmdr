/**
 * Icon ids the frontend composes from another id's OS icon instead of asking the OS for one:
 * the base icon, drawn twice and stacked (`FileIcon.svelte`). A server's share
 * (`network:share`, set by `share_list_volume.rs::SHARE_ICON_ID`) is the platform folder
 * stacked, so it reads as "shares" in the folder style of whichever OS Cmdr runs on, gold
 * tint included. `prefetchIcons` asks the OS for the base.
 *
 * Its own module, apart from `icon-cache.ts`: dozens of view tests mock the cache wholesale,
 * and this pure lookup has to keep answering under every one of them.
 */
const STACKED_ICON_BASES: ReadonlyMap<string, string> = new Map([['network:share', 'dir']])

/** The id whose OS icon `iconId` is drawn from, stacked, or undefined for an ordinary id. */
export function stackedIconBase(iconId: string): string | undefined {
  return STACKED_ICON_BASES.get(iconId)
}
