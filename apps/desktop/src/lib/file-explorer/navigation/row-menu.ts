/**
 * A row's actions: the ONE list every door to them shows. The volume switcher renders it as
 * the row's → submenu (right-click opens the same submenu), the favorites menu does the same
 * for a favorite, and the servers hub renders a one-place server's list at the pointer.
 *
 * ❗ **One list, so no two surfaces can drift** on which items a row has, their order, or
 * which ones a running transfer greys. The busy and ejecting answers are Rust's own
 * (`busy_volume_ids()` and the eject flight set, pushed into `volume-busy-store.svelte.ts`),
 * the same inputs `detachControlFor` reads for the row's inline Eject button, so the button
 * and the menu item three pixels apart can't disagree. The real guard stays in Rust
 * (`eject_volume` refuses a busy volume) and in `runDetach`; greying is the honest preview.
 *
 * `VolumeContextActionKind` stays the action vocabulary, so a pick lands in the handlers the
 * palette and the breadcrumb's native Eject already use.
 *
 * **Three named groups**, in this order (`RowMenu`): the row's ACTIONS on the volume (Open,
 * Eject, Disconnect, the pin, the forgets), then one-shot FIXES for the row's current state
 * ("Connect directly now"), then, below a rule, per-row SETTINGS (checkboxes). A fix shows
 * only while its state needs it, and reads as something to do now, so it runs straight on
 * from the actions; a setting is a standing choice, so it gets the rule. A new item goes
 * into its group's builder (`serverActions` / `detachAction`, `rowFixes`, `rowToggles`), ❌
 * never spliced in by position.
 *
 * Adding a row's checkbox (a per-row setting): give `RowToggleKind` a member, give
 * `VolumeRowFacts` the value it shows, push a `toggle` entry in `rowToggles`, and handle
 * the kind in the two `flipToggle` maps (`VolumeChooserMenu.svelte` and
 * `../network/servers-hub-actions.ts`), each a `Record<RowToggleKind, …>` that stops
 * compiling until you do. The surfaces fill the fact from their stores.
 *
 * Adding a fix: give `RowFixKind` a member, push a `fix` entry in `rowFixes` gated on the
 * state it repairs, and give `runRowFix`'s `Record<RowFixKind, …>` its runner. Every surface
 * already sends a `fix` pick to `runRowFix`.
 */

import type { VolumeContextActionKind } from '#lib/ipc/bindings.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'
import { tString } from '#lib/intl/messages.svelte.js'
import type { IconName } from '#lib/ui/icons/icon-map.js'
import type { MenuItem, MenuSection } from '#lib/ui/menu-types.js'
import { connectDirectly } from '../network/direct-connect'
import { showsDisconnect } from './connection-state'
import { detachControlFor, detachSubjectOf, type DetachSubject } from './detach-control'
import { detachAllOnDisk, runDetach } from './detach-volume'
import { isServerPlaceRow, runServerRowAction } from './server-row-actions'
import { volumeScheme } from '#lib/volume-scheme.js'
import type { VolumeInfo } from '../types'
import type { SignedInAs } from '../network/signed-in-as'

/** A per-row switch the submenu carries as a checkbox row. */
export type RowToggleKind = 'direct-connection' | 'share-parent-row' | 'auto-reconnect'

/** A one-shot repair a row offers only while its current state needs it. */
export type RowFixKind = 'connect-directly'

/** An action on the volume itself. */
export interface RowActionEntry {
  type: 'action'
  action: VolumeContextActionKind
  label: string
  icon: IconName
  disabled?: boolean
  /** Leaves the menu up after the pick (see `MenuItem.keepsMenuOpen`). */
  keepsMenuOpen?: boolean
  /** What the item does, where the label alone invites a wrong reading. */
  tooltip?: string
}

/** A one-shot fix: runs once, now. Closes the menu, since a fix may raise a sheet. */
export interface RowFixEntry {
  type: 'fix'
  fix: RowFixKind
  label: string
  icon: IconName
  /** Why the fix is on offer, since it appears and disappears with the row's state. */
  tooltip: string
}

/** A per-row setting, drawn as a checkbox. */
export interface RowToggleEntry {
  type: 'toggle'
  toggle: RowToggleKind
  label: string
  checked: boolean
  /** What the switch does, where the label alone invites a wrong reading. */
  tooltip?: string
}

/**
 * An SMB server's account action: what the hub's host row, the path bar's account menu, the
 * share-row menu, and the palette all offer, run by ONE function
 * (`../network/share-list-sign-in.ts::runServerAccountAction`). A TS kind, ❌ not members of
 * the Rust `VolumeContextActionKind`: no native menu draws these, and an SMB server has no
 * volume for that vocabulary to act on.
 */
export type ServerAccountAction = 'sign-in-as' | 'browse-as-guest' | 'forget-secret'

/** An account action on the row's SMB server. */
export interface RowAccountEntry {
  type: 'account'
  account: ServerAccountAction
  label: string
  icon: IconName
}

/** A share-list row's own verbs: Open (mounts the share) and Copy path. */
export type ShareRowAction = 'open-share' | 'copy-path'

/** A verb on a share-list row's share. */
export interface RowShareEntry {
  type: 'share'
  share: ShareRowAction
  label: string
  icon: IconName
}

/** One row of a volume row's menu: an action to run, a fix to apply, or a switch to flip. */
export type RowMenuEntry = RowActionEntry | RowFixEntry | RowToggleEntry

/**
 * One row of a menu on an SMB server's rows (the hub's host and share rows, a share-list row,
 * the path bar's account): a volume row's entries, plus the account actions and a share's verbs.
 */
export type ServerRowMenuEntry = RowMenuEntry | RowAccountEntry | RowShareEntry

/**
 * A row's menu in its three named groups (the header says why this order). `A` is what the
 * actions group holds: a volume row's actions, or (`ServerRowMenu`) an SMB server row's.
 */
export interface RowMenu<A extends ActionsGroupEntry = RowActionEntry> {
  actions: A[]
  fixes: RowFixEntry[]
  settings: RowToggleEntry[]
}

/** An SMB server row's menu, whose actions group also holds account actions and a share's verbs. */
export type ServerRowMenu = RowMenu<ActionsGroupEntry>

/** A row that offers nothing. Spread it and replace a group, ❌ never push into one: the arrays are shared. */
export const EMPTY_ROW_MENU: RowMenu = { actions: [], fixes: [], settings: [] }

/**
 * The groups as the menu draws them, a rule between each two: the actions and the fixes
 * share a block (both are things to do), and the settings sit below a rule. Empty blocks
 * drop out, so a lone block draws no rule.
 */
function ruledBlocks<A extends ActionsGroupEntry>(menu: RowMenu<A>): EntryOf<A>[][] {
  const blocks: EntryOf<A>[][] = [[...menu.actions, ...menu.fixes], menu.settings]
  return blocks.filter((block) => block.length > 0)
}

/** What a menu's actions group can hold. */
type ActionsGroupEntry = RowActionEntry | RowAccountEntry | RowShareEntry

/** Any entry of a menu whose actions group holds `A`. */
type EntryOf<A extends ActionsGroupEntry> = A | RowFixEntry | RowToggleEntry

/** What the row's own fields can't say, read by the surface from its stores. */
export interface VolumeRowFacts {
  /** A transfer reads from or writes to the volume (Rust's `busy_volume_ids()`). */
  busy: boolean
  /** The volume's eject is still running. */
  ejecting: boolean
  /** A saved server entry backs this row, so Edit and Forget server have a subject. */
  isSaved: boolean
  /** The SMB share's direct-connection switch, or `undefined` until Rust answered for it. */
  directConnection: boolean | undefined
  /**
   * A saved SFTP or WebDAV place's "Reconnect automatically" switch (`SavedServer.autoReconnect`),
   * or `undefined` where nothing saved backs the row: a one-shot connection has nothing to persist.
   */
  autoReconnect: boolean | undefined
  /**
   * An SMB share's "Show “..” in the top folder" (its override, else the default) and the
   * server `..` leads to, or `undefined` until Rust answered for the share
   * (`../network/share-parent-row.svelte.ts`).
   */
  shareParentRow: { shown: boolean; serverLabel: string } | undefined
}

/** A pick a surface hands back: the entry, and the volume it acts on. */
export interface RowMenuPick {
  kind: 'row-entry'
  volume: VolumeInfo
  entry: RowMenuEntry
}

function action(
  kind: VolumeContextActionKind,
  labelKey: MessageKey,
  icon: IconName,
  options: { disabled?: boolean; keepsMenuOpen?: boolean } = {},
): RowActionEntry {
  return { type: 'action', action: kind, label: tString(labelKey), icon, ...options }
}

/**
 * A server place's actions: Open, Edit… (when saved), Disconnect (when there's a session),
 * Pin to switcher / Unpin, Forget saved password, Forget server (when saved).
 *
 * ❗ A server says Disconnect, ❌ never Eject: "Eject" promises safe-to-unplug, and a server has
 * nothing to unplug. ❗ "Forget saved password" is offered unconditionally, ❌ never gated on "is
 * one stored?": answering that costs a Keychain read, which can raise a system prompt, so the
 * command answers instead (`forgetSavedSecret` words a `false`). Open, Edit, and the pin are
 * never greyed by a transfer: navigating, editing settings, and moving a pin break nothing.
 */
function serverActions(volume: VolumeInfo, facts: VolumeRowFacts): RowActionEntry[] {
  const { busy, isSaved } = facts
  const entries: RowActionEntry[] = [action('open', 'menu.network.open', 'arrow-right')]
  if (isSaved) entries.push(action('edit', 'menu.network.edit', 'pencil'))
  if (showsDisconnect(volume.connectionState)) {
    entries.push(
      action('disconnect', busy ? 'menu.volume.disconnectBusy' : 'menu.network.disconnect', 'unplug', {
        disabled: busy,
        keepsMenuOpen: true,
      }),
    )
  }
  entries.push(pinAction(volume))
  entries.push(
    action('forget-secret', busy ? 'menu.volume.forgetSavedPasswordBusy' : 'menu.network.forgetSavedPassword', 'key', {
      disabled: busy,
    }),
  )
  if (isSaved) {
    entries.push(
      action('forget-server', busy ? 'menu.volume.forgetServerBusy' : 'menu.network.forgetServer', 'trash-2', {
        disabled: busy,
      }),
    )
  }
  return entries
}

/** Pin to switcher, or Unpin. Never greyed: moving a pin breaks nothing. */
function pinAction(volume: VolumeInfo): RowActionEntry {
  return volume.pinned === true
    ? action('unpin', 'menu.network.unpin', 'pin-off', { keepsMenuOpen: true })
    : action('pin', 'menu.network.pinToSwitcher', 'pin', { keepsMenuOpen: true })
}

/**
 * A drive's or a phone's detach item, worded and greyed from the same `detachControlFor`
 * answer the row's inline button renders. `null` when the row has no detach at all.
 */
function detachAction(volume: VolumeInfo, facts: VolumeRowFacts & { shares: boolean }): RowActionEntry | null {
  const detach = detachControlFor(volume, { busy: facts.busy, ejecting: facts.ejecting })
  if (detach?.action !== 'eject') return null
  const { disabled, icon } = detach.button
  const label = detachItemLabel(detachSubjectOf(volume), disabled, volume.name)
  // ❗ Only here, where the row's Eject really does leave the drive attached: the bare word
  // promises safe-to-unplug everywhere else.
  const tooltip = facts.shares ? tString('fileExplorer.navigation.ejectOneVolumeTooltip') : undefined
  return { type: 'action', action: 'eject', label, icon, disabled, keepsMenuOpen: true, tooltip }
}

/**
 * The detach item's words, the same keys the native menus draw (`cmdr-menu`'s `detach_label`). A
 * phone says Disconnect (`adb` detaches nothing to unplug), a share its OS's own verb.
 */
function detachItemLabel(subject: DetachSubject, busy: boolean, name: string): string {
  if (subject === 'phone') return tString(busy ? 'menu.volume.disconnectBusy' : 'menu.network.disconnect')
  if (subject === 'share') return tString(busy ? 'menu.volume.detachShareBusy' : 'menu.volume.detachShare', { name })
  return tString(busy ? 'menu.volume.ejectBusy' : 'menu.volume.eject', { name })
}

/**
 * "Eject all", on a drive with several volumes mounted: the same control's state, since whatever
 * greys Eject also greys taking down everything around it. A write op on a SIBLING isn't greyed
 * here, because this row's facts can't see it: the backend refuses `busy` and says so.
 */
function ejectAllAction(detach: RowActionEntry): RowActionEntry {
  return {
    type: 'action',
    action: 'eject-all',
    label: tString(detach.disabled === true ? 'menu.volume.ejectAllBusy' : 'menu.volume.ejectAll'),
    icon: detach.icon,
    disabled: detach.disabled,
    keepsMenuOpen: true,
    tooltip: tString('fileExplorer.navigation.ejectAllTooltip'),
  }
}

/**
 * The fixes group: one-shot repairs, each gated on the state it repairs.
 *
 * "Connect directly now" shows ONLY while the share's switch is ON and the share is still
 * on the macOS mount (the auto upgrade couldn't dial: no saved credentials, the server
 * asleep, the pane-open cooldown). With the switch OFF, checking it already connects, and
 * a direct share has nothing to fix, so in both the checkbox stands alone.
 */
function rowFixes(volume: VolumeInfo, facts: VolumeRowFacts): RowFixEntry[] {
  const fixes: RowFixEntry[] = []
  if (facts.directConnection === true && volume.connectionState === 'os_mount') {
    fixes.push({
      type: 'fix',
      fix: 'connect-directly',
      label: tString('fileExplorer.navigation.connectDirectlyNow'),
      icon: 'zap',
      tooltip: tString('fileExplorer.navigation.connectDirectlyNowTooltip'),
    })
  }
  return fixes
}

/** The switches group: per-row settings, below a rule. */
function rowToggles(facts: VolumeRowFacts): RowToggleEntry[] {
  const toggles: RowToggleEntry[] = []
  if (facts.directConnection !== undefined) {
    toggles.push({
      type: 'toggle',
      toggle: 'direct-connection',
      label: tString('fileExplorer.navigation.useDirectConnection'),
      checked: facts.directConnection,
    })
  }
  if (facts.shareParentRow !== undefined) {
    toggles.push({
      type: 'toggle',
      toggle: 'share-parent-row',
      label: tString('fileExplorer.navigation.showShareParentRow'),
      checked: facts.shareParentRow.shown,
      tooltip: tString('fileExplorer.navigation.showShareParentRowTooltip', {
        server: facts.shareParentRow.serverLabel,
      }),
    })
  }
  if (facts.autoReconnect !== undefined) {
    // The sign-in sheet's words, label and explanation both, so the two doors to the one
    // setting can't drift apart in meaning (`#lib/servers/DETAILS.md`).
    toggles.push({
      type: 'toggle',
      toggle: 'auto-reconnect',
      label: tString('servers.sheet.autoReconnect'),
      checked: facts.autoReconnect,
      tooltip: tString('servers.sheet.autoReconnectHelp'),
    })
  }
  return toggles
}

/** A volume-switcher (or servers-hub) row's menu. Empty when the row offers nothing. */
export function volumeRowMenu(volume: VolumeInfo, facts: VolumeRowFacts): RowMenu {
  const actions: RowActionEntry[] = []
  if (isServerPlaceRow(volume)) {
    actions.push(...serverActions(volume, facts))
  } else {
    const shares = volume.sharesDiskWithOthers === true
    const detach = detachAction(volume, { ...facts, shares })
    if (detach) actions.push(detach)
    if (detach && shares) actions.push(ejectAllAction(detach))
    // A SAVED SMB share is pinnable like a server place (`docs/specs/saved-smb-shares.md`);
    // a mount nothing saved (Finder's) has no pin to move.
    if (volume.category === 'network' && volumeScheme(volume.id) === 'smb' && facts.isSaved)
      actions.push(pinAction(volume))
  }
  return { actions, fixes: rowFixes(volume, facts), settings: rowToggles(facts) }
}

/**
 * A favorite's menu: Rename, Set shortcut key, and Remove from favorites. All keep the menu up:
 * the editors happen in the row itself, and a list being tidied is several removals in a row. Never
 * greyed: a favorite is a stored `{ path, name }` pair nothing can be using.
 */
export function favoriteRowMenu(): RowMenu {
  return {
    ...EMPTY_ROW_MENU,
    actions: [
      action('rename-favorite', 'menu.volume.renameFavorite', 'pencil', { keepsMenuOpen: true }),
      action('edit-favorite-shortcut', 'menu.volume.editFavoriteShortcut', 'key', { keepsMenuOpen: true }),
      action('remove-favorite', 'menu.volume.removeFavorite', 'star-off', { keepsMenuOpen: true }),
    ],
  }
}

/** What the account items read, off what this session already knows: ❌ never a Keychain read. */
export interface ServerAccountFacts {
  /** The account the server's share list last signed in as, `undefined` when no listing said. */
  listedAccount: SignedInAs | undefined
  /** A guest listing of the server worked this session, so there's a guest to go back to. */
  guestListingWorked: boolean
}

function accountEntry(account: ServerAccountAction, labelKey: MessageKey, icon: IconName): RowAccountEntry {
  return { type: 'account', account, label: tString(labelKey), icon }
}

/**
 * An SMB server's account items, the ONE list every door to them shows (the hub's host row,
 * the path bar's account menu, the share-row menu): Sign in as…, Browse as guest (only while
 * the list is signed in as a user and a guest listing worked, so there's a guest to go back
 * to), and Forget saved password.
 *
 * ❗ Forget saved password is offered unconditionally, ❌ never gated on "is one stored?", the
 * same rule as `serverActions`: answering costs a Keychain read, so the action answers instead
 * (`share-list-sign-in.ts::forgetPassword` words a "there was none").
 */
export function serverAccountEntries(facts: ServerAccountFacts): RowAccountEntry[] {
  const entries = [accountEntry('sign-in-as', 'fileExplorer.network.share.signInAs', 'user')]
  if (facts.listedAccount?.kind === 'user' && facts.guestListingWorked) {
    entries.push(accountEntry('browse-as-guest', 'fileExplorer.network.share.useGuest', 'eye'))
  }
  entries.push(accountEntry('forget-secret', 'menu.network.forgetSavedPassword', 'key'))
  return entries
}

/** What an SMB host row's menu reads beyond its account facts. */
export interface SmbHostRowFacts extends ServerAccountFacts {
  /** A saved server entry backs the row, so Edit and Forget server have a subject. */
  isSaved: boolean
  /**
   * A share from this server is mounted (the kernel's mount table, asked when the menu opens),
   * so Disconnect does something. ❌ Never offered without: "no mounted shares" is an inert item.
   */
  hasMounts: boolean
}

/**
 * An SMB host row's menu in the servers hub: Open, Edit… (when saved), Disconnect (while a
 * share is mounted), the account items, then Forget server (when saved). ❌ No pin: an SMB
 * host has no switcher row to pin. Its actions run through the hub
 * (`../network/servers-hub-actions.ts`), keyed by the hub row, ❌ never a `volumeId`: a host has
 * none.
 */
export function smbHostRowMenu(facts: SmbHostRowFacts): ServerRowMenu {
  const actions: ActionsGroupEntry[] = [action('open', 'menu.network.open', 'arrow-right')]
  if (facts.isSaved) actions.push(action('edit', 'menu.network.edit', 'pencil'))
  if (facts.hasMounts) actions.push(action('disconnect', 'menu.network.disconnect', 'unplug'))
  actions.push(...serverAccountEntries(facts))
  if (facts.isSaved) actions.push(action('forget-server', 'menu.network.forgetServer', 'trash-2'))
  return { ...EMPTY_ROW_MENU, actions }
}

/**
 * A share-list row's menu: Open (mounts the share), Copy path, then its server's account
 * items. ❌ Never the standard file menu: Copy, Delete, Rename, and the rest are all wrong
 * for a share, which is mounted, ❌ never browsed as a folder.
 */
export function shareRowMenu(facts: ServerAccountFacts): ServerRowMenu {
  const share = (verb: ShareRowAction, labelKey: MessageKey, icon: IconName): RowShareEntry => ({
    type: 'share',
    share: verb,
    label: tString(labelKey),
    icon,
  })
  return {
    ...EMPTY_ROW_MENU,
    actions: [
      share('open-share', 'menu.network.open', 'arrow-right'),
      share('copy-path', 'menu.edit.copyPath', 'copy'),
      ...serverAccountEntries(facts),
    ],
  }
}

/** The path bar's account menu on a share list: the server's account items, nothing else. */
export function accountMenu(facts: ServerAccountFacts): ServerRowMenu {
  return { ...EMPTY_ROW_MENU, actions: serverAccountEntries(facts) }
}

/** The entry's name within its row: the action, or the fix, toggle, account, or share verb behind its prefix. */
function entryKey(entry: ServerRowMenuEntry): string {
  if (entry.type === 'action') return entry.action
  if (entry.type === 'fix') return `fix:${entry.fix}`
  if (entry.type === 'account') return `account:${entry.account}`
  if (entry.type === 'share') return `share:${entry.share}`
  return `toggle:${entry.toggle}`
}

/**
 * One entry as a house `Menu` row. Its value is unique across the whole menu
 * (`row:<rowKey>:<entry>`, since every row's submenu shares one value space), and
 * `wrap(entry)` is the payload a pick hands back. `rowKey` names the row: a volume's id,
 * or a hub row's `rowId` for a row with no volume (an SMB host).
 */
function entryItem<E extends ServerRowMenuEntry, T>(rowKey: string, entry: E, wrap: (entry: E) => T): MenuItem<T> {
  const common = { value: `row:${rowKey}:${entryKey(entry)}`, label: entry.label, data: wrap(entry) }
  if (entry.type === 'action') {
    return {
      ...common,
      icon: { lucide: entry.icon },
      disabled: entry.disabled,
      keepsMenuOpen: entry.keepsMenuOpen,
      tooltip: entry.tooltip,
    }
  }
  if (entry.type === 'toggle')
    return { ...common, check: { kind: 'toggle', checked: entry.checked }, tooltip: entry.tooltip }
  if (entry.type === 'fix') return { ...common, icon: { lucide: entry.icon }, tooltip: entry.tooltip }
  return { ...common, icon: { lucide: entry.icon } }
}

/**
 * A row's menu as a SUBMENU (the switcher's and the favorites menu's rows): one flat list,
 * with a rule above each block after the first (`ruledBlocks`). `undefined` for a row with
 * nothing to offer, so it draws no submenu arrow.
 */
export function rowMenuItems<A extends ActionsGroupEntry, T>(
  rowKey: string,
  menu: RowMenu<A>,
  wrap: (entry: EntryOf<A>) => T,
): MenuItem<T>[] | undefined {
  const items = ruledBlocks(menu).flatMap((group, groupIndex) =>
    group.map((entry, index) => ({
      ...entryItem(rowKey, entry, wrap),
      separatorBefore: groupIndex > 0 && index === 0,
    })),
  )
  return items.length > 0 ? items : undefined
}

/**
 * A row's menu as a TOP-LEVEL menu (the servers hub's right-click, the path bar's account
 * menu): a section per ruled block, which the primitive separates, since a top-level list
 * draws no rules inside a section.
 */
export function rowMenuSections<A extends ActionsGroupEntry, T>(
  rowKey: string,
  menu: RowMenu<A>,
  wrap: (entry: EntryOf<A>) => T,
): MenuSection<T>[] {
  return ruledBlocks(menu).map((group, index) => ({
    id: `group-${String(index)}`,
    items: group.map((entry) => entryItem(rowKey, entry, wrap)),
  }))
}

/** What each fix runs. A `Record`, so a new `RowFixKind` won't compile until it's handled. */
const fixRunners: Record<RowFixKind, (volume: VolumeInfo) => Promise<unknown>> = {
  // The ONE "Connect directly" flow (`../network/DETAILS.md` § "Connect directly"), the one
  // the chip's yellow dot and the fallback notice run, with its sign-in sheet and toasts.
  'connect-directly': (volume) => connectDirectly({ volumeId: volume.id, shareName: volume.name }),
}

/** Runs a row's fix, whichever surface it was picked on. */
export async function runRowFix(payload: { volume: VolumeInfo; fix: RowFixKind }): Promise<void> {
  await fixRunners[payload.fix](payload.volume)
}

/**
 * Runs a volume or server row's action. Eject and Disconnect go through `runDetach`, the
 * path the row's inline button takes, which refuses a busy volume whatever the menu showed;
 * the rest go to the servers family (`runServerRowAction`), shared with the palette.
 *
 * `onOpen` is the surface's own navigation: the switcher moves ITS pane, the hub moves its.
 */
export async function runVolumeRowAction(payload: {
  volume: VolumeInfo
  action: VolumeContextActionKind
  onOpen?: (volumeId: string) => void
}): Promise<void> {
  const { volume, action: kind, onOpen } = payload
  if (kind === 'eject') {
    await runDetach(volume, 'eject')
    return
  }
  if (kind === 'eject-all') {
    await detachAllOnDisk(volume)
    return
  }
  if (kind === 'disconnect') {
    await runDetach(volume, 'disconnect-place')
    return
  }
  await runServerRowAction({ action: kind, volumeId: volume.id, volumeName: volume.name, onOpen })
}
