/**
 * What a person and an agent can do on the Servers list, pinned by MEANING so the pins outlive how the list is drawn
 * (getcmdr/cmdr#1517 made it a standard listing).
 *
 * ❗ Every check here reads which server, which pane location, and which saved entry, ❌ never the list's encoding: a
 * row is found by its NAME (what precedes its first two-space gap in `cmdr://state`), the Servers list by its location
 * (`network` at `smb://`), and a share list by its path (`smb://<server>`). The `protocol=` / `status=` / `account=`
 * tokens are an encoding, so nothing here reads them. Keys go through `defaultKey(commandId)` and the pane's focus, ❌
 * never a view selector.
 *
 * The SFTP and WebDAV flows run on both lanes (their fixture servers are leased by both). The SMB flows need the Docker
 * SMB stack's discovered hosts and its addresses, so they run on Linux only, like `smb.spec.ts`; their titles carry
 * "SMB" so `e2e-linux.sh --grep SMB` picks them up.
 */

import { waitBudget } from './wait-budget.js'
import os from 'os'
import type { TauriPage, BrowserPageAdapter } from '@srsholmes/tauri-playwright'
import { test, expect } from './fixtures.js'
import { ensureAppReady, escapeOverlayUntilGone, expectAndDismissToast, focusPane, pressKey } from './helpers.js'
import { defaultKey } from './helpers/registry-keys.js'
import { initMcpClient, mcpCall, mcpReadResource, mcpSelectVolume } from '../e2e-shared/mcp-client.js'
import { serverFixture, uniqueScratchDir, type ServerProtocol } from '../e2e-shared/server-fixtures.js'
import {
  addServerThroughSheet,
  cancelSheet,
  forgetServer,
  paneLocation,
  requireSavedServer,
  savedServerFor,
  SERVER_PASSWORD,
  SHEET,
} from './server-ops-helpers.js'
import {
  authServerIp,
  guestServerIp,
  SMB_AUTH_PASSWORD,
  SMB_AUTH_SHARE,
  SMB_AUTH_USERNAME,
  SMB_UNICODE_HOST,
} from '../e2e-shared/smb-fixtures.js'

type PageLike = TauriPage | BrowserPageAdapter
type Side = 'left' | 'right'

/** "Macintosh HD" on macOS, "Root" on Linux: where both panes start and end. */
const LOCAL_VOLUME_NAME = os.platform() === 'linux' ? 'Root' : 'Macintosh HD'

/** The Servers list's volume name, the Rust `SERVERS_VOLUME_NAME`. */
const SERVERS = 'Servers'

// ── Reading a pane by meaning ────────────────────────────────────────────────

interface PaneView {
  volumeId: string
  path: string
  cursorIndex: number
  /** Each row's text by its screen index (`i:<n>`), the `[cur]` / `[sel]` markers taken off. */
  rows: Map<number, string>
}

async function paneView(side: Side): Promise<PaneView> {
  const state = await mcpReadResource('cmdr://state')
  const start = state.indexOf(`\n${side}:\n`)
  if (start === -1) return { volumeId: '', path: '', cursorIndex: -1, rows: new Map() }
  const body = state.slice(start + side.length + 2)
  const end = /\n\S/.exec(body)
  const section = end ? body.slice(0, end.index) : body
  const rows = new Map<number, string>()
  for (const m of section.matchAll(/^ {4}- "i:(\d+) [dfl] (.*)"$/gm)) {
    rows.set(Number(m[1]), m[2].replace(/( \[(cur|sel)\])+$/, ''))
  }
  return {
    volumeId: /^ {2}volumeId: (.+)$/m.exec(section)?.[1] ?? '',
    path: (/^ {2}path: (.+)$/m.exec(section)?.[1] ?? '').replace(/^"|"$/g, ''),
    cursorIndex: Number(/^ {2}cursor:\n {4}index: (\d+)$/m.exec(section)?.[1] ?? '-1'),
    rows,
  }
}

/** A row's name: what precedes its first two-space gap, where an agent-facing row's facts start. */
function nameOf(row: string): string {
  return row.split('  ')[0]
}

function isServersList(view: PaneView): boolean {
  return view.volumeId === 'network' && view.path === 'smb://'
}

/** Whether the pane is on a server's share list (`network` at `smb://<server>`). */
function isShareList(view: PaneView): boolean {
  return view.volumeId === 'network' && view.path.startsWith('smb://') && view.path.length > 'smb://'.length
}

/** The names the pane's Servers list shows, or `null` when the pane isn't on it. */
async function serverNames(side: Side): Promise<string[] | null> {
  const view = await paneView(side)
  return isServersList(view) ? [...view.rows.values()].map(nameOf) : null
}

/** The name of the Servers list's row under the cursor, or `null` when the pane isn't on the Servers list. */
async function serverUnderCursor(side: Side): Promise<string | null> {
  const view = await paneView(side)
  if (!isServersList(view)) return null
  const row = view.rows.get(view.cursorIndex)
  return row === undefined ? null : nameOf(row)
}

async function openServersList(side: Side): Promise<void> {
  await mcpSelectVolume(side, SERVERS)
  await expect.poll(async () => isServersList(await paneView(side)), { timeout: waitBudget(15000) }).toBeTruthy()
}

async function awaitServerListed(side: Side, name: string): Promise<void> {
  await expect
    .poll(async () => (await serverNames(side))?.includes(name) ?? false, { timeout: waitBudget(30000) })
    .toBeTruthy()
}

/**
 * Puts the cursor on a Servers row by name, in a focused pane, so a key pressed next reaches it.
 *
 * ❗ The focus click first: it lands inside the pane and may move the cursor, so the name pick comes after it.
 */
async function cursorOnServer(tauriPage: PageLike, side: Side, name: string): Promise<void> {
  await focusPane(tauriPage, side === 'left' ? 0 : 1)
  await mcpCall('move_cursor', { pane: side, filename: name })
  await expect.poll(async () => serverUnderCursor(side), { timeout: waitBudget(5000) }).toBe(name)
}

/** Opens the server under the cursor and waits for the pane to be on a share list holding at least one share. */
async function openSmbServer(side: Side, name: string): Promise<PaneView> {
  await mcpCall('move_cursor', { pane: side, filename: name })
  await mcpCall('open_under_cursor', {})
  await expect
    .poll(
      async () => {
        const view = await paneView(side)
        return isShareList(view) && [...view.rows.values()].some((row) => row !== '..')
      },
      { timeout: waitBudget(30000) },
    )
    .toBeTruthy()
  return paneView(side)
}

/** `..` on a share list, by its index: `move_cursor` finds rows by name in the backend listing, which has no `..`. */
async function goUpFromShareList(side: Side): Promise<void> {
  await mcpCall('move_cursor', { pane: side, index: 0 })
  await mcpCall('open_under_cursor', {})
}

// ── SMB servers added by IP ──────────────────────────────────────────────────

/** Saves the SMB server at `address` under `name`, the way "Connect to server…" does, answering its host. */
async function addSmbServerByIp(
  tauriPage: PageLike,
  address: string,
  name: string,
  { checkReachability = false }: { checkReachability?: boolean } = {},
): Promise<{ host: { id: string } }> {
  return tauriPage.evaluate<{ host: { id: string } }>(
    `window.__TAURI_INTERNALS__.invoke('connect_to_server', ${JSON.stringify({
      address: `smb://${address}`,
      name,
      username: null,
      checkReachability,
    })})`,
  )
}

/**
 * Forgets the saved SMB host `id` as the row's Forget does (`forget_saved_smb_host`): its manual entry, the account
 * it's used with, its sign-in history, and the session credential a sign-in on it filed. ❌ Not `forget_server`, which
 * forgets a saved PLACE and answers `false` for a host, leaving it saved for every later spec.
 */
async function forgetSmbHost(tauriPage: PageLike, id: string): Promise<void> {
  const forgotten = await tauriPage.evaluate<boolean>(
    `window.__TAURI_INTERNALS__.invoke('forget_saved_smb_host', { id: ${JSON.stringify(id)} })`,
  )
  expect(forgotten, `the saved SMB host ${id} is forgotten`).toBe(true)
}

// ── Forget's confirmation ────────────────────────────────────────────────────

/**
 * Answers Forget's native confirmation "Forget, keep the password" without showing it: the native alert can't be
 * clicked from a spec, so the IPC seam (`set_test_ipc_answer`) answers in its place, and the frontend's real Forget
 * runs on that answer.
 */
async function answerForgetConfirmation(tauriPage: PageLike): Promise<void> {
  await tauriPage.evaluate(
    `window.__TAURI_INTERNALS__.invoke('set_test_ipc_answer', ${JSON.stringify({
      command: 'confirm_with_checkbox',
      answer: { kind: 'confirmed', checked: false },
      isError: false,
    })})`,
  )
}

async function clearForgetConfirmation(tauriPage: PageLike): Promise<void> {
  await tauriPage.evaluate(
    `window.__TAURI_INTERNALS__.invoke('set_test_ipc_answer', ${JSON.stringify({
      command: 'confirm_with_checkbox',
      answer: null,
      isError: false,
    })})`,
  )
}

// ── Both lanes ───────────────────────────────────────────────────────────────

test.describe('Servers list survives: picking it', () => {
  test.beforeEach(async ({ tauriPage }) => {
    await initMcpClient(tauriPage)
    await ensureAppReady(tauriPage)
  })

  test.afterEach(async () => {
    await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
  })

  test('select_volume Servers puts the pane on the network volume at smb://', async () => {
    // The tool itself waits for the pane's pushed volume name, so a Servers list that never mounts times out here.
    await mcpSelectVolume('left', SERVERS)
    await expect
      .poll(async () => paneView('left'), { timeout: waitBudget(15000) })
      .toMatchObject({
        volumeId: 'network',
        path: 'smb://',
      })
  })
})

/**
 * One saved one-place server, through its whole life on the Servers list: added through Connect to server… (⌘K), listed
 * by its name, opened onto its place, edited from F2, and forgotten with F8.
 */
function defineOnePlaceServerFlows(protocol: ServerProtocol): void {
  const fixture = serverFixture(protocol)

  test.describe(`Servers list survives: a saved ${fixture.label} server`, () => {
    test.describe.configure({ mode: 'serial', timeout: waitBudget(90_000) })

    // Unique per worker, since the fixture server is shared with other lanes and worktrees.
    const dir = uniqueScratchDir(protocol)

    test.beforeAll(() => {
      fixture.mkdir(dir)
      fixture.writeFile(`${dir}/landmark.txt`, 'here\n')
    })

    test.afterAll(() => {
      fixture.remove(dir)
    })

    test.beforeEach(async ({ tauriPage }) => {
      await initMcpClient(tauriPage)
      await ensureAppReady(tauriPage)
    })

    test.afterEach(async ({ tauriPage }) => {
      await clearForgetConfirmation(tauriPage)
      await cancelSheet(tauriPage).catch(() => undefined)
      await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
    })

    /** The server's saved name and its place, adding it first when nothing (or a server elsewhere) is saved. */
    async function ensureSaved(tauriPage: PageLike): Promise<{ name: string; placeVolumeId: string }> {
      const saved = await savedServerFor(tauriPage, fixture)
      if (!saved || !saved.places.some((place) => place.appRoot.includes(dir))) {
        await forgetServer(tauriPage, fixture)
        expect(await addServerThroughSheet(tauriPage, fixture, dir, SERVER_PASSWORD)).toBe('closed')
      }
      const server = await requireSavedServer(tauriPage, fixture)
      const named = await tauriPage.evaluate<{ id: string; displayName: string }[]>(
        `window.__TAURI_INTERNALS__.invoke('list_saved_servers')`,
      )
      const name = named.find((s) => s.id === server.id)?.displayName ?? ''
      expect(name, 'the saved server has a name to find it by').not.toBe('')
      return { name, placeVolumeId: server.places[0].volumeId }
    }

    test('Connect to server… (⌘K) adds it, and the Servers list shows it by name', async ({ tauriPage }) => {
      await forgetServer(tauriPage, fixture)
      const { name } = await ensureSaved(tauriPage)

      await openServersList('left')
      await awaitServerListed('left', name)
    })

    test('Enter on its row lands the pane on its place', async ({ tauriPage }) => {
      const { name, placeVolumeId } = await ensureSaved(tauriPage)
      await openServersList('left')
      await awaitServerListed('left', name)

      await mcpCall('move_cursor', { pane: 'left', filename: name })
      await mcpCall('open_under_cursor', {})

      await expect
        .poll(async () => (await paneLocation('left')).volumeId, { timeout: waitBudget(30000) })
        .toBe(placeVolumeId)
    })

    test('F2 on its row opens the edit sheet on that server', async ({ tauriPage }) => {
      const { name } = await ensureSaved(tauriPage)
      await openServersList('left')
      await awaitServerListed('left', name)
      await cursorOnServer(tauriPage, 'left', name)

      await pressKey(tauriPage, defaultKey('file.rename'))

      await expect.poll(async () => tauriPage.isVisible(SHEET), { timeout: waitBudget(5000) }).toBeTruthy()
      // The sheet is on THIS server: its address field names the fixture's host.
      await expect
        .poll(
          async () => tauriPage.evaluate<string>(`document.querySelector('${SHEET} #server-address')?.value ?? ''`),
          { timeout: waitBudget(5000) },
        )
        .toContain(fixture.host)
      await cancelSheet(tauriPage)
    })

    test('F8 on its row forgets it, and the Servers list drops it', async ({ tauriPage }) => {
      const { name } = await ensureSaved(tauriPage)
      await openServersList('left')
      await awaitServerListed('left', name)
      await cursorOnServer(tauriPage, 'left', name)

      await answerForgetConfirmation(tauriPage)
      await pressKey(tauriPage, defaultKey('file.delete'))

      await expect.poll(async () => savedServerFor(tauriPage, fixture), { timeout: waitBudget(10000) }).toBeNull()
      await expect
        .poll(async () => (await serverNames('left'))?.includes(name) ?? true, { timeout: waitBudget(10000) })
        .toBe(false)
    })
  })
}

defineOnePlaceServerFlows('sftp')
defineOnePlaceServerFlows('webdav')

// ── Linux: the SMB flows ─────────────────────────────────────────────────────

// macOS can't mount or reach the Docker SMB stack headless; see `smb.spec.ts`.
// eslint-disable-next-line @typescript-eslint/unbound-method -- conditional skip
const describeSmb = process.platform === 'darwin' ? test.describe.skip : test.describe

describeSmb('Servers list survives: SMB servers', () => {
  test.describe.configure({ timeout: waitBudget(120_000), retries: 1 })

  test.beforeEach(async ({ tauriPage }) => {
    await initMcpClient(tauriPage)
    await ensureAppReady(tauriPage)
  })

  test.afterEach(async ({ tauriPage }) => {
    await clearForgetConfirmation(tauriPage)
    await escapeOverlayUntilGone(tauriPage, SHEET).catch(() => undefined)
    await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
  })

  test('SMB: Enter opens a server’s share list, and `..` lands back on that server’s row', async () => {
    // ❗ Not the list's first row: a Servers list that opened on its first row would pass on the first server.
    const server = 'SMB Test (50 Shares)'
    await openServersList('left')
    await awaitServerListed('left', server)

    const shareList = await openSmbServer('left', server)
    expect(shareList.path).toBe('smb://smb test (50 shares)')

    await goUpFromShareList('left')
    await expect.poll(async () => serverUnderCursor('left'), { timeout: waitBudget(15000) }).toBe(server)
    expect((await paneView('left')).cursorIndex, 'the server is not the first row').toBeGreaterThan(0)
  })

  test('SMB: a server added by IP opens its share list by that IP, and `..` lands back on its row', async ({
    tauriPage,
  }) => {
    const address = guestServerIp()
    test.skip(address === null, 'needs the guest server’s IP (Linux Docker)')
    if (address === null) return
    const name = 'SMB Test by IP (flows)'
    const added = await addSmbServerByIp(tauriPage, address, name)
    try {
      await openServersList('left')
      await awaitServerListed('left', name)

      const shareList = await openSmbServer('left', name)
      expect(shareList.path).toBe(`smb://${address}`)

      await goUpFromShareList('left')
      await expect.poll(async () => serverUnderCursor('left'), { timeout: waitBudget(15000) }).toBe(name)
    } finally {
      await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
      await forgetSmbHost(tauriPage, added.host.id)
    }
  })

  test('SMB: F8 on a server Cmdr only found refuses, and the server stays listed', async ({ tauriPage }) => {
    // Found by discovery and never saved: nothing in this run signs in to it or adds it.
    const server = 'SMB Test (Unicode)'
    await openServersList('left')
    await awaitServerListed('left', server)
    await cursorOnServer(tauriPage, 'left', server)

    // Answered "yes" in case the refusal regressed into asking: then the server WOULD go, and the test says so.
    await answerForgetConfirmation(tauriPage)
    await pressKey(tauriPage, defaultKey('file.delete'))

    await expectAndDismissToast(tauriPage, 'discovered', { timeout: waitBudget(5000) })
    expect(await serverNames('left')).toContain(server)
  })

  test('SMB: the row menu’s Sign in as… signs in and opens the server’s share list', async ({ tauriPage }) => {
    // ❗ The auth server added by IP under a name of its own, ❌ never the discovered `SMB Test (Auth)`: a sign-in
    // outlives the test (the session credential, the account the server is used with, the listing it bought), and
    // `smb.spec.ts`'s first-sign-in flows need that host signed in to by nobody. This host's Forget takes it all.
    const address = authServerIp()
    test.skip(address === null, 'needs the auth server’s IP (Linux Docker)')
    if (address === null) return
    const server = 'SMB Test auth by IP (flows)'
    const added = await addSmbServerByIp(tauriPage, address, server)
    try {
      await signInFromRowMenu(tauriPage, server)
    } finally {
      await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
      await forgetSmbHost(tauriPage, added.host.id)
    }
  })

  async function signInFromRowMenu(tauriPage: PageLike, server: string): Promise<void> {
    await openServersList('left')
    await awaitServerListed('left', server)
    await cursorOnServer(tauriPage, 'left', server)

    // The keyboard's context-menu key on the cursor row: the door that stays whatever draws the row.
    await pressKey(tauriPage, defaultKey('file.contextMenu'))
    // The item by its value (`row:<rowId>:account:sign-in-as`), ❌ not its words.
    const signInRow = `document.querySelector('[data-menu] [data-menu-row$=":account:sign-in-as"]')`
    await expect
      .poll(async () => tauriPage.evaluate<boolean>(`${signInRow} !== null`), { timeout: waitBudget(15000) })
      .toBeTruthy()
    await tauriPage.evaluate(`${signInRow}.click()`)
    await expect.poll(async () => tauriPage.isVisible(SHEET), { timeout: waitBudget(15000) }).toBeTruthy()

    // Typed, not remembered: nothing reaches the Keychain.
    await tauriPage.evaluate(`(function () {
      function type(selector, value) {
        var input = document.querySelector(selector);
        input.value = value;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }
      type('${SHEET} #sign-in-username', ${JSON.stringify(SMB_AUTH_USERNAME)});
      type('${SHEET} #sign-in-secret', ${JSON.stringify(SMB_AUTH_PASSWORD)});
    })()`)
    await tauriPage.click(`${SHEET} .modal-footer button:last-of-type`)

    await expect.poll(async () => !(await tauriPage.isVisible(SHEET)), { timeout: waitBudget(30000) }).toBeTruthy()
    await expect
      .poll(
        async () => {
          const view = await paneView('left')
          return isShareList(view) && [...view.rows.values()].map(nameOf).includes(SMB_AUTH_SHARE)
        },
        { timeout: waitBudget(30000) },
      )
      .toBeTruthy()
  }

  /**
   * #324, literally: opening the Servers list asks NO server for its shares, read off the backend's share-listing
   * tally (`network/share_listing_tally.rs`). The rows come from memory, the stores, and the mount table; a saved
   * server's shares are read when a pane opens its share list, and a server Cmdr only found the same.
   */
  test('SMB: opening the Servers list asks no server for its shares, saved or found', async ({ tauriPage }) => {
    const address = guestServerIp()
    test.skip(address === null, 'needs the guest server’s IP (Linux Docker)')
    if (address === null) return
    const asks = (host: string) =>
      tauriPage.evaluate<{ asked: number; dialed: number }>(
        `window.__TAURI_INTERNALS__.invoke('share_listing_asks', { address: ${JSON.stringify(host)} })`,
      )
    const name = 'SMB Test saved (asks)'
    // ❗ Reached: a server saved without contact (`awaitingFirstConnect`) is nobody's to list until it connects (#777).
    const added = await addSmbServerByIp(tauriPage, address, name, { checkReachability: true })
    try {
      await tauriPage.evaluate(`window.__TAURI_INTERNALS__.invoke('reset_share_listing_asks')`)
      await openServersList('left')
      await awaitServerListed('left', name)
      await awaitServerListed('left', 'SMB Test (Unicode)')

      // No prefetch: the saved server is never asked while the list is on screen, nor a found one, by address or name.
      expect((await asks(address)).asked).toBe(0)
      expect((await asks(SMB_UNICODE_HOST)).asked).toBe(0)
      expect((await asks('SMB Test (Unicode)')).asked).toBe(0)
    } finally {
      await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
      await forgetSmbHost(tauriPage, added.host.id)
    }
  })
})
