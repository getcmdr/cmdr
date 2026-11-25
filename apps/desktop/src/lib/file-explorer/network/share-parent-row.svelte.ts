/**
 * `..` in an SMB share's top folder (#1238): whether a share shows it, where it
 * leads, and the one-time toast that explains it the first time it's used.
 *
 * What the backend knows about each share (`getSmbShareParent`: the server's host and
 * name, and the share's own override) is cached here by volume id, so both panes and
 * the volume switcher read one answer. Whether `..` shows is that override, else the
 * `network.showShareParentRow` default (`share-parent-rule.ts`). Every write goes
 * through Rust and then updates the cache, so the cache stays what Rust holds.
 */

import { SvelteMap } from 'svelte/reactivity'
import { getSmbShareParent, setSmbShareParentRow, type ShareParent } from '#lib/tauri-commands/index.js'
import { getShowShareParentRow } from '#lib/settings/reactive-settings.svelte.js'
import { getSetting, setSetting } from '#lib/settings/index.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { volumeKindOf } from '../pane/volume-capabilities'
import type { VolumeInfo } from '../types'
import { effectiveShow, overrideAfter } from './share-parent-rule'

const log = getAppLogger('network')

/** What Rust said about each share, by volume id: `null` when no SMB share is behind it. */
const parents = new SvelteMap<string, ShareParent | null>()

/** Whether `volume` is an SMB share, so it's worth asking Rust about. Rust has the last word. */
export function isSmbShareVolume(volume: VolumeInfo | null | undefined): volume is VolumeInfo {
  return volume != null && volumeKindOf(volume.id, volume.fsType, volume.category) === 'smb'
}

/**
 * Caches what Rust said about the share behind `volumeId`: from `getSmbShareParent`, or the
 * `shareParent` a top-folder listing carries (`listing-complete`).
 *
 * ❗ A `null` never replaces an answer. A share's volume id names that one share for good, so
 * a `null` after an answer only means it was asked too early again (a saved share whose mount
 * wasn't up yet). The other way round is the fix for #1238's missing `..`: the `null` a pane
 * got before the mount finished gives way to the next answer.
 */
export function noteShareParent(volumeId: string, parent: ShareParent | null): void {
  if (parent === null && parents.get(volumeId)) return
  parents.set(volumeId, parent)
}

/** Asks Rust about the share behind `volumeId` and caches the answer. Never throws. */
export async function loadShareParent(volumeId: string): Promise<ShareParent | null> {
  try {
    noteShareParent(volumeId, await getSmbShareParent(volumeId))
    return parents.get(volumeId) ?? null
  } catch (e) {
    log.warn('Reading where “..” leads out of {volumeId} broke down: {error}', { volumeId, error: e })
    return parents.get(volumeId) ?? null
  }
}

/** What's known about the share behind `volumeId`, or `undefined` until it's been asked. Reactive. */
export function shareParentOf(volumeId: string): ShareParent | null | undefined {
  return parents.get(volumeId)
}

/**
 * Whether the share behind `volumeId` shows `..` in its top folder, or `undefined` while
 * it isn't known to be an SMB share. Reactive (the cache and the default both).
 */
export function showsParentRow(volumeId: string): boolean | undefined {
  const parent = parents.get(volumeId)
  if (!parent) return undefined
  return effectiveShow(parent.showParentRow, getShowShareParentRow())
}

/**
 * Sets the share's own switch. Measured against the default as it is now, so a choice
 * matching it clears the override. Answers whether Rust saved it.
 */
export async function setShareParentRow(volumeId: string, show: boolean): Promise<boolean> {
  const defaultShow = getSetting('network.showShareParentRow')
  let saved: boolean
  try {
    saved = await setSmbShareParentRow(volumeId, show, defaultShow)
  } catch (e) {
    log.warn('Saving “..” for {volumeId} broke down: {error}', { volumeId, error: e })
    return false
  }
  const parent = parents.get(volumeId)
  if (saved && parent) parents.set(volumeId, { ...parent, showParentRow: overrideAfter(show, defaultShow) })
  return saved
}

/** Flips the share's switch from what it shows now. Asks Rust first when nothing is cached yet. */
export async function toggleShareParentRow(volumeId: string): Promise<void> {
  if (showsParentRow(volumeId) === undefined) await loadShareParent(volumeId)
  const shown = showsParentRow(volumeId)
  if (shown === undefined) return
  await setShareParentRow(volumeId, !shown)
}

/**
 * "Turn off for all shares": the default goes off. Every share's override stays, so a
 * share someone turned on keeps its `..`.
 */
export function turnOffShareParentRowForAll(): void {
  setSetting('network.showShareParentRow', false)
}

/** The one place `..` out of a share leads, for the pane to open. */
export interface ShareParentLanding {
  /** The share's volume, which "Turn off for this share" acts on. */
  volumeId: string
  /** The host whose share list `..` opened, so another host's list doesn't take the landing. */
  hostId: string
  /** The share's name, for the cursor to land back on in the share list. */
  shareName: string
  /** The server's name in words, for the toast. */
  serverLabel: string
}
