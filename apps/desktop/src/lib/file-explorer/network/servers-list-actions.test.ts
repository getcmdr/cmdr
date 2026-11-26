/**
 * What F8, a right-click, and a row-menu pick do to each kind of Servers row.
 *
 * The whole risk here is that a one-place row and an SMB host take different
 * paths at every branch, and the wrong branch removes the wrong thing: a Forget
 * on a place drops a saved server plus its pins, a Forget on an SMB host drops a
 * manual-server entry, and a Disconnect on a host unmounts shares rather than
 * dropping a session.
 */

import { describe, it, expect, vi, beforeEach, beforeAll, afterAll } from 'vitest'
import { _setLocaleForTests } from '#lib/intl/locale.js'
import type { NetworkHost, VolumeInfo } from '../types'

const forgetSavedSmbHost = vi.fn((_id: string) => Promise.resolve(true))
const forgetServer = vi.fn((_volumeId: string) => Promise.resolve(true))
const forgetServerSecret = vi.fn((_volumeId: string) => Promise.resolve(true))
const disconnectNetworkHost = vi.fn(() => Promise.resolve(['/Volumes/Public']))
/** Whether a share from the host is mounted, one answer per ask (the kernel's mount table). */
const smbHostHasMounts = vi.fn((_host: unknown) => Promise.resolve(false))
const forgetSavedServer = vi.fn(() => Promise.resolve())
const setServerAutoReconnect = vi.fn((_volumeId: string, _on: boolean) => Promise.resolve())
const runVolumeRowAction = vi.fn((_payload: unknown) => Promise.resolve())
const openRow = vi.fn()
const runServerAccountAction = vi.fn((_action: string, _server: unknown) => Promise.resolve({ listed: false }))
/** What this session's listings said about a host, as the account items read it. */
let accountFacts: {
  listedAccount: { kind: 'user'; username: string } | { kind: 'guest' } | undefined
  guestListingWorked: boolean
} = {
  listedAccount: undefined,
  guestListingWorked: false,
}
const addToast = vi.fn()
const confirmDialog = vi.fn(() => Promise.resolve(true))
const confirmWithCheckbox = vi.fn((_question: unknown) => Promise.resolve({ confirmed: true, checked: true }))
const forgetSavedSmbHostPassword = vi.fn((_id: string) => Promise.resolve(true))
const listSavedServers = vi.fn(() => Promise.resolve<unknown[]>([]))
const setCredentialStatus = vi.fn()
/** What the store's in-memory credential status says about the host. */
let credentialStatus = 'has_creds'
const openEditServerSheet = vi.fn((_server: unknown) => Promise.resolve({ kind: 'saved' }))
const runServerRowAction = vi.fn((_payload: unknown) => Promise.resolve())
const savedServerOfShareList = vi.fn((_path: string) => Promise.resolve<string | null>(null))

vi.mock('#lib/tauri-commands/index.js', () => ({
  forgetSavedSmbHost: (id: string) => forgetSavedSmbHost(id),
  forgetSavedSmbHostPassword: (id: string) => forgetSavedSmbHostPassword(id),
  listSavedServers: () => listSavedServers(),
  forgetServer: (volumeId: string) => forgetServer(volumeId),
  forgetServerSecret: (volumeId: string) => forgetServerSecret(volumeId),
  disconnectNetworkHost: (...args: unknown[]) => disconnectNetworkHost(...(args as [])),
  smbHostHasMounts: (host: unknown) => smbHostHasMounts(host),
  savedServerOfShareList: (path: string) => savedServerOfShareList(path),
}))
vi.mock('./share-list-label', () => ({
  shareListServerLabel: (path: string) => (path === 'smb://printer' ? 'Printer' : 'Attic NAS'),
}))
vi.mock('./network-store.svelte', () => ({
  getCredentialStatus: () => credentialStatus,
  setCredentialStatus: (...args: unknown[]) => {
    setCredentialStatus(...(args as []))
  },
}))
vi.mock('./share-list-sign-in', () => ({
  runServerAccountAction: (action: string, server: unknown) => runServerAccountAction(action, server),
  serverAccountFacts: () => accountFacts,
}))
vi.mock('../navigation/server-row-actions', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../navigation/server-row-actions')>()),
  forgetSavedServer: (...args: unknown[]) => forgetSavedServer(...(args as [])),
  setServerAutoReconnect: (volumeId: string, on: boolean) => setServerAutoReconnect(volumeId, on),
  runServerRowAction: (payload: unknown) => runServerRowAction(payload),
}))
vi.mock('#lib/stores/volume-busy-store.svelte.js', () => ({ isVolumeBusy: () => false, isVolumeEjecting: () => false }))
vi.mock('../navigation/row-menu', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../navigation/row-menu')>()),
  runVolumeRowAction: (payload: unknown) => runVolumeRowAction(payload),
}))
vi.mock('#lib/ui/toast/index.js', () => ({
  addToast: (...args: unknown[]) => {
    addToast(...(args as []))
  },
}))
vi.mock('#lib/servers/open-sign-in.js', () => ({
  openEditServerSheet: (server: unknown) => openEditServerSheet(server),
}))
vi.mock('#lib/utils/confirm-dialog.js', () => ({
  confirmDialog: (...args: unknown[]) => confirmDialog(...(args as [])),
  confirmWithCheckbox: (question: unknown) => confirmWithCheckbox(question),
}))

import { createServerRowActions, editServerRow, editServerInView } from './servers-list-actions'
import type { ServerRow } from './server-row-of-entry'
import type { VolumeContextActionKind } from '#lib/ipc/bindings.js'
import type { ServerRowMenuEntry } from '../navigation/row-menu'

const host: NetworkHost = { id: 'h1', name: 'Attic NAS', ipAddress: '10.0.0.4', port: 445, source: 'manual' }

/** A one-place server: the servers family speaks for it. */
const placeRow: ServerRow = {
  id: 'sftp-testnas.local-22-test_user_1',
  path: 'sftp://test_user_1@testnas.local:22',
  account: null,
  name: 'Testnas',
  protocol: 'sftp',
  address: 'testnas.local:22',
  status: 'saved',
  volumeId: 'sftp-testnas.local-22-test_user_1',
  pinned: true,
  saved: {
    id: 'sftp-testnas.local-22-test_user_1',
    protocol: 'sftp',
    displayName: 'Testnas',
    nameSource: 'user',
    address: 'testnas.local:22',
    username: 'test_user_1',
    pinned: true,
    lastConnectedAt: null,
    autoReconnect: true,
    places: [
      {
        volumeId: 'sftp-testnas.local-22-test_user_1',
        name: 'Testnas',
        pinned: true,
        connected: false,
        appRoot: 'sftp://test_user_1@testnas.local:22',
        username: 'test_user_1',
        autoReconnect: true,
      },
    ],
  },
  host: null,
}

/** A saved SMB host: a manual-server entry, with no place to act on. */
const savedHostRow: ServerRow = {
  id: 'manual-10-0-0-4-445',
  path: 'smb://10.0.0.4',
  account: null,
  name: 'Attic NAS',
  protocol: 'smb',
  address: '10.0.0.4',
  status: 'found_nearby',
  volumeId: null,
  pinned: false,
  saved: {
    id: 'manual-10-0-0-4-445',
    protocol: 'smb',
    displayName: 'Attic NAS',
    nameSource: 'user',
    address: '10.0.0.4',
    username: null,
    pinned: false,
    lastConnectedAt: null,
    autoReconnect: null,
    places: [],
  },
  host,
}

/** A host only mDNS knows about: nothing of the user's to remove. */
const nearbyOnlyRow: ServerRow = {
  ...savedHostRow,
  id: 'h2',
  saved: null,
  host: { ...host, id: 'h2', source: 'discovered' },
}

const liveVolume: VolumeInfo = {
  id: 'sftp-testnas.local-22-test_user_1',
  name: 'Testnas',
  path: 'sftp://test_user_1@testnas.local:22',
  category: 'network',
  fsType: 'sftp',
  isEjectable: false,
  connectionState: 'direct',
}

const refreshSaved = vi.fn(() => Promise.resolve())

function actions(volumes: VolumeInfo[] = []) {
  return createServerRowActions({
    getRows: () => [placeRow, savedHostRow, nearbyOnlyRow],
    getVolumes: () => volumes,
    refreshSaved,
    openRow,
  })
}

beforeAll(() => {
  _setLocaleForTests('en-US')
})
afterAll(() => {
  _setLocaleForTests(null)
})
beforeEach(() => {
  vi.clearAllMocks()
  confirmDialog.mockResolvedValue(true)
  confirmWithCheckbox.mockResolvedValue({ confirmed: true, checked: true })
  credentialStatus = 'has_creds'
  accountFacts = { listedAccount: undefined, guestListingWorked: false }
  smbHostHasMounts.mockResolvedValue(false)
})

/** What a row menu learned on open: nothing mounted, unless a test says so. */
const nothingMounted = { hasMounts: false }

/** A menu entry's name, so an assertion reads like the menu. */
function keyOf(entry: ServerRowMenuEntry): string {
  if (entry.type === 'action') return entry.action
  if (entry.type === 'fix') return entry.fix
  if (entry.type === 'toggle') return entry.toggle
  if (entry.type === 'account') return `account:${entry.account}`
  return `share:${entry.share}`
}

describe('forget', () => {
  it('sends a one-place server to the servers family, so the Servers list asks what the switcher asks', async () => {
    await actions().forget(placeRow)
    expect(forgetSavedServer).toHaveBeenCalledWith('sftp-testnas.local-22-test_user_1', 'Testnas')
    expect(forgetSavedSmbHost).not.toHaveBeenCalled()
  })

  it('removes a saved SMB host from the manual store instead, after asking', async () => {
    await actions().forget(savedHostRow)
    expect(confirmWithCheckbox).toHaveBeenCalledOnce()
    expect(forgetSavedSmbHost).toHaveBeenCalledWith('manual-10-0-0-4-445')
    expect(forgetSavedServer).not.toHaveBeenCalled()
  })

  /**
   * ❗ One verb: the menu says "Forget server", so the alert's title, its button, and
   * the toast say Forget too. The title slot used to carry the button's word
   * ("Remove") and the button read "OK" (QA round 2).
   */
  it('asks with a Forget title and a Forget button, and says Forgot after', async () => {
    await actions().forget(savedHostRow)
    expect(confirmWithCheckbox).toHaveBeenCalledWith({
      message: 'Forget Attic NAS? Cmdr stops listing it and the shares saved under it. Nothing gets unmounted.',
      title: 'Forget server',
      confirmLabel: 'Forget',
      checkboxLabel: 'Also forget the saved password',
      checked: true,
    })
    expect(addToast).toHaveBeenCalledWith('Forgot Attic NAS', { level: 'success' })
  })

  /**
   * ❗ The box is checked by default, and a checked box takes the host's stored
   * password FIRST: the backend finds its names on the rows the Forget then removes.
   */
  it('forgets the host’s saved password first when the box stays checked', async () => {
    const order: string[] = []
    forgetSavedSmbHostPassword.mockImplementationOnce(() => {
      order.push('password')
      return Promise.resolve(true)
    })
    forgetSavedSmbHost.mockImplementationOnce(() => {
      order.push('host')
      return Promise.resolve(true)
    })
    await actions().forget(savedHostRow)
    expect(order).toEqual(['password', 'host'])
    expect(forgetSavedSmbHostPassword).toHaveBeenCalledWith('manual-10-0-0-4-445')
    expect(setCredentialStatus).toHaveBeenCalledWith('Attic NAS', 'no_creds')
  })

  /**
   * ❗ The box shows only where a password may be stored: an account the host is used
   * with, or one this session already read. A guest-only host offered to forget a
   * password it never had (final QA). Known without a Keychain read.
   */
  it('asks plainly, with no password box, for a host with no account and nothing read', async () => {
    credentialStatus = 'unknown'
    await actions().forget(savedHostRow)
    expect(confirmWithCheckbox).not.toHaveBeenCalled()
    expect(confirmDialog).toHaveBeenCalledOnce()
    expect(forgetSavedSmbHostPassword).not.toHaveBeenCalled()
    expect(forgetSavedSmbHost).toHaveBeenCalledOnce()
  })

  it('offers the box for a host used with an account, even with nothing read', async () => {
    credentialStatus = 'unknown'
    const withAccount = {
      ...savedHostRow,
      saved: savedHostRow.saved && { ...savedHostRow.saved, username: 'test_user_1' },
    }
    await actions().forget(withAccount)
    expect(confirmWithCheckbox).toHaveBeenCalledOnce()
  })

  it('keeps the host’s saved password when the box was unchecked', async () => {
    confirmWithCheckbox.mockResolvedValueOnce({ confirmed: true, checked: false })
    await actions().forget(savedHostRow)
    expect(forgetSavedSmbHostPassword).not.toHaveBeenCalled()
    expect(forgetSavedSmbHost).toHaveBeenCalledOnce()
  })

  it('still forgets the host when its password won’t go, and says which half didn’t', async () => {
    forgetSavedSmbHostPassword.mockRejectedValueOnce(new Error('keychain locked'))
    await actions().forget(savedHostRow)
    expect(forgetSavedSmbHost).toHaveBeenCalledOnce()
    expect(addToast).toHaveBeenCalledWith(expect.stringContaining('saved password'), { level: 'error' })
  })

  /**
   * ❗ Cmdr lists every server a mounted share comes from, so a host forgotten with a
   * share still mounted would come right back: the question offers to unmount first,
   * and only an unmount that left nothing behind lets the Forget go on.
   */
  it('offers to unmount a host’s shares first while one is mounted, then forgets it', async () => {
    smbHostHasMounts.mockResolvedValueOnce(true).mockResolvedValueOnce(false)
    const order: string[] = []
    disconnectNetworkHost.mockImplementationOnce(() => {
      order.push('unmount')
      return Promise.resolve(['/Volumes/Public'])
    })
    forgetSavedSmbHost.mockImplementationOnce(() => {
      order.push('forget')
      return Promise.resolve(true)
    })
    await actions().forget(savedHostRow)
    expect(confirmWithCheckbox).toHaveBeenCalledWith(
      expect.objectContaining({
        message:
          'Some of Attic NAS’s shares are still mounted, so Cmdr would list it again right away. Unmount them and forget Attic NAS?',
        confirmLabel: 'Unmount and forget',
      }),
    )
    expect(order).toEqual(['unmount', 'forget'])
  })

  it('keeps a host whose share won’t unmount, and says why', async () => {
    smbHostHasMounts.mockResolvedValue(true)
    await actions().forget(savedHostRow)
    expect(disconnectNetworkHost).toHaveBeenCalledOnce()
    expect(forgetSavedSmbHost).not.toHaveBeenCalled()
    expect(forgetSavedSmbHostPassword).not.toHaveBeenCalled()
    expect(addToast).toHaveBeenCalledWith(
      'Some of Attic NAS’s shares are still mounted, so Cmdr kept it. Close what’s using them and try again.',
      { level: 'error' },
    )
  })

  it('unmounts nothing for a host nothing is mounted from, or when the person says no', async () => {
    await actions().forget(savedHostRow)
    expect(disconnectNetworkHost).not.toHaveBeenCalled()
    smbHostHasMounts.mockResolvedValue(true)
    confirmWithCheckbox.mockResolvedValue({ confirmed: false, checked: true })
    await actions().forget(savedHostRow)
    expect(disconnectNetworkHost).not.toHaveBeenCalled()
  })

  it('re-reads the saved list after removing a host, which nothing broadcasts', async () => {
    await actions().forget(savedHostRow)
    expect(refreshSaved).toHaveBeenCalledOnce()
  })

  it('keeps a host the user said no to', async () => {
    confirmWithCheckbox.mockResolvedValue({ confirmed: false, checked: false })
    await actions().forget(savedHostRow)
    expect(forgetSavedSmbHost).not.toHaveBeenCalled()
  })

  it('says a discovered host is not the user’s to remove, rather than doing nothing', async () => {
    await actions().forget(nearbyOnlyRow)
    expect(forgetSavedSmbHost).not.toHaveBeenCalled()
    expect(forgetSavedServer).not.toHaveBeenCalled()
    expect(addToast).toHaveBeenCalledWith('Can’t remove discovered hosts', { level: 'warn' })
  })
})

describe('rowMenu', () => {
  /** The menu's entries as their action names, so an assertion reads like the menu. */
  function actionsOf(row: ServerRow, volumes: VolumeInfo[] = [], opened = nothingMounted): string[] {
    const menu = actions(volumes).rowMenu(row, opened)
    if (!menu) return []
    return [...menu.actions, ...menu.fixes, ...menu.settings].map(keyOf)
  }

  it('gives a one-place row the servers list, the same one the switcher row’s submenu shows', () => {
    // Live and saved: every server action. Pinned, so the pin reads Unpin. Saved, so
    // "Reconnect automatically" rides below, from the saved entry.
    expect(actionsOf(placeRow, [{ ...liveVolume, pinned: true }])).toEqual([
      'open',
      'edit',
      'disconnect',
      'unpin',
      'forget-secret',
      'forget-server',
      'auto-reconnect',
    ])
    const toggle = actions([liveVolume]).rowMenu(placeRow, nothingMounted)?.settings[0]
    expect(toggle).toMatchObject({ toggle: 'auto-reconnect', checked: true })
  })

  it('stands in for a place the volume list has no row for, rather than skipping the menu', () => {
    // A saved server that is neither pinned nor connected isn't in the listing: no session, so no Disconnect.
    expect(actionsOf(placeRow, [])).toEqual([
      'open',
      'edit',
      'unpin',
      'forget-secret',
      'forget-server',
      'auto-reconnect',
    ])
  })

  it('gives a saved SMB host Open, Edit, its account items, and Forget server, with no pin', () => {
    expect(actionsOf(savedHostRow)).toEqual([
      'open',
      'edit',
      'account:sign-in-as',
      'account:forget-secret',
      'forget-server',
    ])
  })

  it('offers an SMB host’s Disconnect only while a share from it is mounted', () => {
    expect(actionsOf(savedHostRow, [], { hasMounts: true })).toContain('disconnect')
    expect(actionsOf(savedHostRow)).not.toContain('disconnect')
  })

  it('offers Browse as guest only while signed in as a user on a server a guest listing worked on', () => {
    accountFacts = { listedAccount: { kind: 'user', username: 'test_user_1' }, guestListingWorked: true }
    expect(actionsOf(savedHostRow)).toContain('account:browse-as-guest')
    accountFacts = { listedAccount: { kind: 'guest' }, guestListingWorked: true }
    expect(actionsOf(savedHostRow)).not.toContain('account:browse-as-guest')
  })

  it('gives a host only mDNS knows about no Edit and no Forget server: nothing of the user’s is saved', () => {
    expect(actionsOf(nearbyOnlyRow)).toEqual(['open', 'account:sign-in-as', 'account:forget-secret'])
  })

  /** ❗ Opening the menu reads no password: Forget saved password is offered whatever is stored. */
  it('offers Forget saved password with nothing known to be stored, at no Keychain read', () => {
    credentialStatus = 'unknown'
    expect(actionsOf(savedHostRow)).toContain('account:forget-secret')
  })
})

describe('runRowEntry', () => {
  const entry = (action: VolumeContextActionKind) =>
    ({ type: 'action', action, label: action, icon: 'pencil' }) as const

  it('opens the place through the Servers list’s own Enter path', async () => {
    await actions([liveVolume]).runRowEntry(placeRow, entry('open'))
    expect(openRow).toHaveBeenCalledWith(placeRow)
    expect(runVolumeRowAction).not.toHaveBeenCalled()
  })

  it('hands every other action to the one runner the switcher uses', async () => {
    await actions([liveVolume]).runRowEntry(placeRow, entry('disconnect'))
    expect(runVolumeRowAction).toHaveBeenCalledWith({ volume: liveVolume, action: 'disconnect' })
  })

  it('flips "Reconnect automatically" from the saved entry’s value, through the servers family', async () => {
    const toggle = {
      type: 'toggle',
      toggle: 'auto-reconnect',
      label: 'Reconnect automatically',
      checked: true,
    } as const
    await actions([liveVolume]).runRowEntry(placeRow, toggle)
    expect(setServerAutoReconnect).toHaveBeenCalledWith('sftp-testnas.local-22-test_user_1', false)
  })
})

describe('an SMB host row’s actions', () => {
  const entry = (action: VolumeContextActionKind) =>
    ({ type: 'action', action, label: action, icon: 'pencil' }) as const
  const account = (kind: 'sign-in-as' | 'browse-as-guest' | 'forget-secret') =>
    ({ type: 'account', account: kind, label: kind, icon: 'user' }) as const

  it('routes Forget server through the same branch F8 takes', async () => {
    await actions().runRowEntry(savedHostRow, entry('forget-server'))
    expect(forgetSavedSmbHost).toHaveBeenCalledWith('manual-10-0-0-4-445')
  })

  it('opens the edit sheet on the saved host the menu was raised for', async () => {
    await actions().runRowEntry(savedHostRow, entry('edit'))
    expect(openEditServerSheet).toHaveBeenCalledWith(savedHostRow.saved)
  })

  it('unmounts an SMB host’s shares, which is what its Disconnect means', async () => {
    await actions().runRowEntry(savedHostRow, entry('disconnect'))
    // ❗ The whole host: its mounts are found by where it dials, every name on ITS port.
    expect(disconnectNetworkHost).toHaveBeenCalledWith(host)
    expect(addToast).toHaveBeenCalledWith('Disconnected from Attic NAS', { level: 'success' })
    expect(runVolumeRowAction).not.toHaveBeenCalled()
  })

  it('treats nothing-to-unmount as a normal answer, not a fault', async () => {
    disconnectNetworkHost.mockResolvedValueOnce([])
    await actions().runRowEntry(savedHostRow, entry('disconnect'))
    expect(addToast).toHaveBeenCalledWith('No mounted shares from Attic NAS')
  })

  /** ❌ Never the thrown text: anything thrown is the IPC transport, which a person must not be handed. */
  it('words a Disconnect that broke down as a sentence about the server', async () => {
    disconnectNetworkHost.mockRejectedValueOnce(new Error('ipc channel closed'))
    await actions().runRowEntry(savedHostRow, entry('disconnect'))
    expect(addToast).toHaveBeenCalledExactlyOnceWith('Cmdr couldn’t disconnect from Attic NAS.', { level: 'error' })
  })

  it('runs an account item through the one runner every door shares, for this row’s host and name', async () => {
    await actions().runRowEntry(savedHostRow, account('forget-secret'))
    expect(runServerAccountAction).toHaveBeenCalledExactlyOnceWith('forget-secret', { host, label: 'Attic NAS' })
    expect(openRow).not.toHaveBeenCalled()
  })

  /** A Sign in as… from the row is the person pointing at the server, so #324 holds: it opens, as Enter does. */
  it('opens the server’s share list once an account item listed it', async () => {
    runServerAccountAction.mockResolvedValueOnce({ listed: true })
    await actions().runRowEntry(savedHostRow, account('sign-in-as'))
    expect(openRow).toHaveBeenCalledExactlyOnceWith(savedHostRow)
  })

  /**
   * ❗ **Two rows can stand on one discovered host**: a saved host and a second
   * saved row for the same machine (QA 2026-09-25: "Edit server…" on the SECOND
   * row saved its name into the FIRST). A pick acts on the row it was picked on,
   * whichever row comes first in the list.
   */
  describe('when two rows share one host', () => {
    const twinRow: ServerRow = {
      ...savedHostRow,
      id: 'manual-localhost-11482-445',
      name: 'localhost:11482',
      saved: { ...(savedHostRow.saved as NonNullable<ServerRow['saved']>), id: 'manual-localhost-11482-445' },
    }
    const withTwin = () =>
      createServerRowActions({
        getRows: () => [savedHostRow, twinRow],
        getVolumes: () => [],
        refreshSaved,
        openRow,
      })

    it('edits the row the menu was raised on', async () => {
      await withTwin().runRowEntry(twinRow, entry('edit'))
      expect(openEditServerSheet).toHaveBeenCalledWith(twinRow.saved)
    })

    it('names the host in its toasts the way its row does', async () => {
      await withTwin().runRowEntry(twinRow, entry('disconnect'))
      expect(addToast).toHaveBeenCalledWith('Disconnected from localhost:11482', { level: 'success' })
    })

    it('forgets the row the menu was raised on, and never its twin', async () => {
      await withTwin().runRowEntry(twinRow, entry('forget-server'))
      expect(forgetSavedSmbHost).toHaveBeenCalledExactlyOnceWith(twinRow.id)
    })
  })
})

/**
 * Edit and Rename (F4, F2, ⇧F6 by default, whatever they're bound to) on a Servers row are
 * "Edit server…". ❗ A row with nothing to edit says why, ❌ never does nothing.
 */
describe('editServerRow', () => {
  it('opens Edit server on a saved SMB host', async () => {
    await editServerRow(savedHostRow)
    expect(openEditServerSheet).toHaveBeenCalledExactlyOnceWith(savedHostRow.saved)
    expect(addToast).not.toHaveBeenCalled()
  })

  it('opens Edit server on a one-place server through the menu’s own Edit', async () => {
    await editServerRow(placeRow)
    expect(runServerRowAction).toHaveBeenCalledExactlyOnceWith({
      action: 'edit',
      volumeId: 'sftp-testnas.local-22-test_user_1',
      volumeName: 'Testnas',
    })
  })

  /** ❗ With an `id`, so F4 held down or pressed again replaces the hint instead of stacking copies. */
  it('says why a host only mDNS knows has nothing to edit', async () => {
    await editServerRow(nearbyOnlyRow)
    expect(openEditServerSheet).not.toHaveBeenCalled()
    expect(addToast).toHaveBeenCalledExactlyOnceWith(expect.stringContaining('Attic NAS'), {
      level: 'info',
      id: 'servers-edit-hint',
    })
  })
})

/**
 * What's in view on the Servers volume: a Servers row, or the share list that is up.
 * ❗ Every case answers something: a sheet, or a toast saying why not.
 */
describe('editServerInView', () => {
  it('edits the Servers row under the cursor', async () => {
    await editServerInView({ row: savedHostRow, shareListPath: null })
    expect(openEditServerSheet).toHaveBeenCalledExactlyOnceWith(savedHostRow.saved)
  })

  it('edits the saved server whose row opens the share list that is up', async () => {
    if (!savedHostRow.saved) throw new Error('fixture')
    savedServerOfShareList.mockResolvedValueOnce(savedHostRow.saved.id)
    listSavedServers.mockResolvedValueOnce([savedHostRow.saved])
    await editServerInView({ row: null, shareListPath: 'smb://10.0.0.4' })
    expect(savedServerOfShareList).toHaveBeenCalledExactlyOnceWith('smb://10.0.0.4')
    expect(openEditServerSheet).toHaveBeenCalledExactlyOnceWith(savedHostRow.saved)
  })

  it('says a share list nobody saved a server for has nothing to edit', async () => {
    await editServerInView({ row: null, shareListPath: 'smb://printer' })
    expect(openEditServerSheet).not.toHaveBeenCalled()
    expect(addToast).toHaveBeenCalledExactlyOnceWith(expect.stringContaining('Printer'), {
      level: 'info',
      id: 'servers-edit-hint',
    })
  })

  it('asks for a server when the cursor is on Add server…', async () => {
    await editServerInView({ row: null, shareListPath: null })
    expect(addToast).toHaveBeenCalledExactlyOnceWith('Select a server to edit it.', {
      level: 'info',
      id: 'servers-edit-hint',
    })
  })
})

describe('an S3 account', () => {
  const s3Place = (bucket: string) => ({
    volumeId: `s3-host-443-akia-${bucket}`,
    name: bucket,
    pinned: true,
    connected: false,
    appRoot: `s3://AKIA@host:443/${bucket}`,
    username: 'AKIA',
    autoReconnect: true,
  })
  const account: ServerRow = {
    ...placeRow,
    id: 's3-host-443-akia-root',
    path: 's3://AKIA@host:443',
    name: 'AKIA@host',
    protocol: 's3',
    volumeId: null,
    pinned: false,
    saved: {
      id: 's3-host-443-akia-root',
      protocol: 's3',
      displayName: 'AKIA@host',
      nameSource: 'fallback',
      address: 'host',
      username: 'AKIA',
      pinned: false,
      lastConnectedAt: null,
      autoReconnect: true,
      places: [s3Place('photos'), s3Place('scans')],
    },
  }

  it('forgets the whole account: the shared secret FIRST, then every place under it', async () => {
    await actions().forget(account)
    expect(forgetServerSecret).toHaveBeenCalledExactlyOnceWith('s3-host-443-akia-photos')
    expect(forgetServer.mock.calls.map(([id]) => id)).toEqual(['s3-host-443-akia-photos', 's3-host-443-akia-scans'])
    expect(forgetServerSecret.mock.invocationCallOrder[0]).toBeLessThan(forgetServer.mock.invocationCallOrder[0])
  })

  it('keeps the secret when the box is cleared, and forgets nothing when the question is declined', async () => {
    confirmWithCheckbox.mockResolvedValueOnce({ confirmed: true, checked: false })
    await actions().forget(account)
    expect(forgetServerSecret).not.toHaveBeenCalled()
    expect(forgetServer).toHaveBeenCalledTimes(2)

    vi.clearAllMocks()
    confirmWithCheckbox.mockResolvedValueOnce({ confirmed: false, checked: true })
    await actions().forget(account)
    expect(forgetServer).not.toHaveBeenCalled()
  })

  it('edits the account row as the account', async () => {
    await editServerRow(account)
    expect(openEditServerSheet).toHaveBeenCalledExactlyOnceWith(account.saved)
    expect(addToast).not.toHaveBeenCalled()
  })
})
