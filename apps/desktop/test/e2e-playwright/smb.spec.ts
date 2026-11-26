/**
 * E2E tests for SMB network integration.
 *
 * Tests virtual SMB host discovery, share listing, mounting, file browsing,
 * and cross-storage copy through the full Cmdr stack: UI → Tauri IPC →
 * network module → Docker SMB containers.
 *
 * Requires:
 * - App built with `--features playwright-e2e,smb-e2e`
 * - Docker SMB containers running: `./apps/desktop/test/smb-servers/start.sh all`
 * - Guest share pre-mounted (handled by smb-fixtures.ts setup)
 */

import { waitBudget } from './wait-budget.js'
import fs from 'fs'
import path from 'path'
import { test, expect } from './fixtures.js'
import { restoreFixtureTree } from '../e2e-shared/fixture-manifest.js'
import { recreateFixtures } from '../e2e-shared/fixtures.js'
import {
  setupSmb,
  teardownSmb,
  guestMountPoint,
  guestServerIp,
  unmountGuestShareFrom,
  requireGuestMount,
  requireGuestSuite,
  SMB_E2E_SUITE_DIR,
  SMB_GUEST_SHARE,
  SMB_GUEST_HOST,
  SMB_GUEST_PORT,
  SMB_AUTH_HOST,
  SMB_AUTH_PORT,
  SMB_AUTH_SHARE,
  SMB_AUTH_USERNAME,
  SMB_AUTH_PASSWORD,
  SMB_50SHARES_HOST,
  SMB_50SHARES_PORT,
  SMB_UNICODE_HOST,
  SMB_UNICODE_PORT,
  smbWriteFile,
} from '../e2e-shared/smb-fixtures.js'
import {
  initMcpClient,
  mcpCall,
  mcpCallRaw,
  mcpReadResource,
  mcpSelectVolume,
  mcpNavToPath,
  mcpAwaitItem,
  mcpAwaitPath,
  mcpConfirmedCopy,
} from '../e2e-shared/mcp-client.js'
import {
  dismissOverlay,
  dispatchMenuCommand,
  ensureAppReady,
  focusPane,
  pressKey,
  escapeOverlayUntilGone,
  expectAndDismissToast,
  getFixtureRoot,
  pollUntil,
  isStateClean,
} from './helpers.js'
import { defaultKey } from './helpers/registry-keys.js'
import type { CredentialSource, ShareListResult } from '../../src/lib/ipc/bindings.js'

import os from 'os'

// SMB operations involve network + Docker overhead.
test.setTimeout(waitBudget(120_000))

// Linux SMB tests run inside Docker (gvfs-based mounting). mDNS discovery and
// GVFS mount are environmentally flaky on Docker overlay filesystems; see
// e2e-linux/CLAUDE.md for the GVFS / UDisks2VolumeMonitor warning the OS spews
// when concurrent mounts collide. A single retry hides this without masking
// real regressions. SMB tests are skipped on macOS, so the retry only fires on
// Linux.
test.describe.configure({ retries: 1 })

/** Name of the root/local volume (differs by platform). */
const LOCAL_VOLUME_NAME = os.platform() === 'linux' ? 'Root' : 'Macintosh HD'

test.beforeAll(() => {
  setupSmb()
})

test.afterAll(() => {
  teardownSmb()
})

test.beforeEach(async ({ tauriPage }) => {
  // ── MCP health diagnostic ──────────────────────────────────────────────
  // When running the full test suite, the MCP server sometimes dies before
  // SMB tests start. Log the MCP port to help diagnose when/why this happens.
  try {
    const port = await tauriPage.evaluate<number>(`window.__TAURI_INTERNALS__.invoke('get_mcp_port')`)
    console.log(`[SMB diag] MCP port: ${String(port)}`)
  } catch (err: unknown) {
    console.error(`[SMB diag] MCP port check failed (app may be dead):`, err instanceof Error ? err.message : err)
  }

  // Fixture recreation is opt-in per test: only the two cross-storage copy
  // tests below touch local files, so recreating fixtures for every SMB test
  // would waste work on the 14 that never read from `left/` or `right/`.
  // Those two tests call `recreateFixtures()` themselves; the `ensureAppReady()`
  // they run right after navigates, polls the fresh fixtures present, flushes
  // the file watcher, and re-confirms stability, so no extra settle is needed.

  // Navigate to the main route first: volume-select event listeners
  // only exist on the file explorer page, not on /settings.
  await tauriPage.evaluate(`(function() {
        var a = document.createElement('a');
        a.href = '/';
        document.body.appendChild(a);
        a.click();
        a.remove();
    })()`)
  await tauriPage.waitForSelector('.dual-pane-explorer', 15000)

  await initMcpClient(tauriPage)

  // Force both panes back to a local volume. Previous tests may have left a pane
  // on Network or MTP. ensureAppReady's mcp-nav-to-path events get rejected by
  // navigateToPath when the pane is on a non-local volume, so we switch volumes first.
  //
  // Short-circuit: skip the volume-select + Escape sequence when both panes are
  // already on the local volume and no modal overlay is lingering.
  if (!(await isStateClean(tauriPage, LOCAL_VOLUME_NAME))) {
    await tauriPage.evaluate(`(function() {
          var invoke = window.__TAURI_INTERNALS__.invoke;
          invoke('plugin:event|emit', { event: 'mcp-volume-select', payload: { pane: 'left', name: '${LOCAL_VOLUME_NAME}' } });
          invoke('plugin:event|emit', { event: 'mcp-volume-select', payload: { pane: 'right', name: '${LOCAL_VOLUME_NAME}' } });
      })()`)
    // Wait for both panes to show the local volume in cmdr://state.
    await expect
      .poll(
        async () => {
          const state = await mcpReadResource('cmdr://state')
          const volumeLines = (state.match(/\n {2}volume: ([^\n]+)/g) ?? []).map((line) =>
            line.replace(/^\n {2}volume: /, ''),
          )
          return volumeLines.length >= 2 && volumeLines[0] === LOCAL_VOLUME_NAME && volumeLines[1] === LOCAL_VOLUME_NAME
        },
        { timeout: waitBudget(5000) },
      )
      .toBeTruthy()

    // Previously: double-Escape + best-effort modal-overlay poll to clean up
    // dialogs leaked from prior tests. The global afterEach safety net in
    // fixtures.ts now catches and auto-cleans any leaks at the point of leak,
    // so this defensive cleanup is no longer needed.
  }
})

// Putting the shared `left/` + `right/` tree back is this spec's job: the
// post-test leak guard fails whoever leaves it dirty, and the restore is
// surgical, so it only rewrites what actually drifted.
test.afterEach(() => {
  restoreFixtureTree(getFixtureRoot())
})

// ── Helper ───────────────────────────────────────────────────────────────────

/**
 * The servers hub's entry for a host in `cmdr://state` (`<name>  protocol=smb  status=…`), or `undefined`.
 *
 * ❗ Read off the state, ❌ not the DOM: the hosts Cmdr only found sit in a group that starts collapsed once a server is
 * saved (which the mounts in this file do), and a collapsed group renders none of them. The state lists them either
 * way, and `move_cursor` onto one opens the group.
 */
async function hostEntryInHub(hostName: string): Promise<string | undefined> {
  const state = await mcpReadResource('cmdr://state')
  return state.split('\n').find((line) => line.includes(`${hostName}  protocol=`))
}

/** Checks whether the servers hub lists a host. */
async function hostExistsInPane(hostName: string): Promise<boolean> {
  return (await hostEntryInHub(hostName)) !== undefined
}

/**
 * One pane as `cmdr://state` publishes it: its volume id, the cursor's row, and each row's text (what follows
 * `i:<n> <kind> `, with the `[cur]` / `[sel]` markers taken off).
 *
 * ❗ The share-list pins read THIS, ❌ never the share list's DOM. A share list is an ordinary file listing on the
 * `network` volume: its `path` is the server's key (`smb://<key>`, no trailing slash) and its `volume` the hub's own
 * name, ❌ never `Network > <host>`.
 */
interface PaneRows {
  volumeId: string
  volume: string
  path: string
  cursorIndex: number
  rows: string[]
}

async function paneRows(side: 'left' | 'right'): Promise<PaneRows> {
  const state = await mcpReadResource('cmdr://state')
  const start = state.indexOf(`\n${side}:\n`)
  if (start === -1) return { volumeId: '', volume: '', path: '', cursorIndex: -1, rows: [] }
  const body = state.slice(start + side.length + 2)
  // The pane's block ends at the next top-level key.
  const end = /\n\S/.exec(body)
  const section = end ? body.slice(0, end.index) : body
  const rows = [...section.matchAll(/^ {4}- "i:\d+ [dfl] (.*)"$/gm)].map((m) => m[1].replace(/( \[(cur|sel)\])+$/, ''))
  return {
    volumeId: /^ {2}volumeId: (.+)$/m.exec(section)?.[1] ?? '',
    volume: /^ {2}volume: (.+)$/m.exec(section)?.[1] ?? '',
    path: /^ {2}path: (.+)$/m.exec(section)?.[1] ?? '',
    cursorIndex: Number(/^ {2}cursor:\n {4}index: (\d+)$/m.exec(section)?.[1] ?? '-1'),
    rows,
  }
}

/**
 * The text of the row the cursor is on, found by its `i:<n>` index. ❗ Not `rows[cursorIndex]`: the mirror lists a
 * window of rows around the visible range, so past the first screen the array position isn't the row's index.
 */
async function cursorRowText(side: 'left' | 'right'): Promise<string | null> {
  const { cursorIndex } = await paneRows(side)
  const state = await mcpReadResource('cmdr://state')
  const start = state.indexOf(`\n${side}:\n`)
  if (start === -1 || cursorIndex < 0) return null
  const body = state.slice(start + side.length + 2)
  const end = /\n\S/.exec(body)
  const section = end ? body.slice(0, end.index) : body
  const row = new RegExp(`^ {4}- "i:${String(cursorIndex)} [dfl] (.*)"$`, 'm').exec(section)
  return row ? row[1].replace(/( \[(cur|sel)\])+$/, '') : null
}

/**
 * A share list row's share name. The row carries the share's description as a `comment="…"` token when it has one, so
 * an agent reads both; the fixtures' shares have none, so this accepts either form.
 */
function shareNameOf(row: string): string {
  return row.replace(/ {2}comment=".*"$/, '')
}

/**
 * The pane's share names when it shows a server's share list (the `network` volume with `..` as row 0, the Servers
 * list's rows never start with one), else `null`.
 */
async function sharesInPane(side: 'left' | 'right'): Promise<string[] | null> {
  const pane = await paneRows(side)
  if (pane.volumeId !== 'network' || pane.rows[0] !== '..') return null
  return pane.rows.slice(1).map(shareNameOf)
}

/** Checks whether a share name appears in the pane's share list. */
async function shareExistsInPane(side: 'left' | 'right', shareName: string): Promise<boolean> {
  return (await sharesInPane(side))?.includes(shareName) ?? false
}

/** The Servers list's row the cursor is on, or `null` when the pane isn't on the Servers list. */
async function hubRowUnderCursor(side: 'left' | 'right'): Promise<string | null> {
  const pane = await paneRows(side)
  if (pane.volumeId !== 'network' || pane.rows[0] === '..') return null
  return pane.rows[pane.cursorIndex] ?? null
}

/**
 * Where the guest share is mounted, once the mount table shows it. A worker's `setupSmb` mounts it a moment before
 * the first test runs, so a bare `requireGuestMount()` there can read the table too early.
 */
async function awaitGuestMount(): Promise<string> {
  await expect.poll(() => guestMountPoint(), { timeout: waitBudget(15000) }).not.toBeNull()
  return requireGuestMount()
}

/** Opens a host from the Servers list in `side` and waits for its share list to hold `shareName`. */
async function openHost(side: 'left' | 'right', hostName: string, shareName: string): Promise<void> {
  await mcpCall('move_cursor', { pane: side, filename: hostName })
  await mcpCall('open_under_cursor', {})
  await expect.poll(async () => shareExistsInPane(side, shareName), { timeout: waitBudget(30000) }).toBeTruthy()
}

/**
 * Opens a host from the hub, waits for its share list, and comes back to the hub.
 *
 * A host Cmdr only found has its shares listed when someone opens it, and at no other time (#324), so this is what
 * puts a `shares=` count on its hub entry.
 */
async function openHostAndComeBack(hostName: string): Promise<void> {
  await mcpCall('move_cursor', { pane: 'left', filename: hostName })
  await mcpCall('open_under_cursor', {})
  await expect
    .poll(async () => (await sharesInPane('left'))?.length ?? 0, { timeout: waitBudget(30000) })
    .toBeGreaterThan(0)
  await mcpSelectVolume('left', 'Servers')
}

/** What a listing signs in with. `username: null` is guest, as it is in the sign-in sheet's answer. */
interface ListingSignIn {
  username: string | null
  password: string | null
  credentialSource: CredentialSource
}

/** Guest, the way "Browse as guest" and the sheet's guest choice ask for it. */
const AS_GUEST: ListingSignIn = { username: null, password: null, credentialSource: 'typed' }

/**
 * Lists a host's shares with `list_shares_with_credentials`, the command the sign-in sheet's attempt runs, skipping
 * the UI for the real Samba round-trip.
 *
 * ❗ The argument keys mirror `commands.listSharesWithCredentials` in `src/lib/ipc/bindings.ts`. tsc can't see inside
 * the evaluated string, so a parameter the Rust command gains or renames reaches this spec only as a red run; keeping
 * every call behind this one function makes that a one-place fix. The value types are the generated ones.
 *
 * A unique `hostId` per call keeps a cached listing from answering.
 */
async function listSharesOverIpc(
  tauriPage: Parameters<typeof pollUntil>[0],
  host: { idPrefix: string; hostname: string; port: number },
  signIn: ListingSignIn,
): Promise<ShareListResult> {
  return tauriPage.evaluate<ShareListResult>(`
    window.__TAURI_INTERNALS__.invoke('list_shares_with_credentials', {
      hostId: ${JSON.stringify(host.idPrefix)} + Date.now(),
      hostname: ${JSON.stringify(host.hostname)},
      ipAddress: null,
      port: ${String(host.port)},
      username: ${JSON.stringify(signIn.username)},
      password: ${JSON.stringify(signIn.password)},
      credentialSource: ${JSON.stringify(signIn.credentialSource)},
      timeoutMs: 30000,
      cacheTtlMs: 5000,
    })
  `)
}

// ── Tests ────────────────────────────────────────────────────────────────────

// macOS: SMB mounting requires OS-level permission dialogs (mkdir /Volumes/*)
// that can't be approved in headless E2E. Linux uses GVFS mounts which work
// without elevated permissions, so all SMB functionality is tested there.
// eslint-disable-next-line @typescript-eslint/unbound-method -- conditional skip
const describeSmb = process.platform === 'darwin' ? test.describe.skip : test.describe

describeSmb('SMB host discovery', () => {
  test('virtual hosts appear in Network view', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // Switch left pane to Network
    await mcpSelectVolume('left', 'Servers')

    // Wait for virtual hosts to appear (injected by smb-e2e feature).
    // 30s: defensive bound. Hosts typically appear within 1-3 s; longer budget covers
    // mDNS discovery latency variance on Linux Docker.
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(30000) }).toBeTruthy()

    const hasGuest = await hostExistsInPane('SMB Test (Guest)')
    const hasAuth = await hostExistsInPane('SMB Test (Auth)')
    expect(hasGuest).toBe(true)
    expect(hasAuth).toBe(true)
  })

  test('guest host shows share count once it has been opened', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(30000) }).toBeTruthy()

    await openHostAndComeBack('SMB Test (Guest)')
    await expect
      .poll(async () => (await hostEntryInHub('SMB Test (Guest)'))?.includes('shares=1'), {
        timeout: waitBudget(30000),
      })
      .toBeTruthy()
  })
})

describeSmb('Adding an SMB host through the sign-in sheet', () => {
  test('typing a host opens its shares, and asks for no password on the way', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await mcpSelectVolume('left', 'Servers')
    await expect
      .poll(async () => tauriPage.isVisible('.servers-hub .add-row'), { timeout: waitBudget(30000) })
      .toBeTruthy()

    // A DOUBLE click: the hub's rows follow the file list, where one click moves
    // the cursor and two open the thing.
    await tauriPage.evaluate(`(function () {
      document.querySelector('.servers-hub .add-row').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    })()`)
    const sheet = '[data-dialog-id="server-sign-in"]'
    await expect.poll(async () => tauriPage.isVisible(sheet), { timeout: waitBudget(5000) }).toBeTruthy()

    await tauriPage.evaluate(`(function () {
      var input = document.querySelector('${sheet} #server-address');
      input.value = ${JSON.stringify(`${SMB_GUEST_HOST}:${String(SMB_GUEST_PORT)}`)};
      input.dispatchEvent(new Event('input', { bubbles: true }));
    })()`)

    // ❗ SMB is the default protocol, whose connect is a share MOUNT rather than a
    // session: nothing is asked until a listing or a mount refuses, so there is
    // no password field to put in front of anyone.
    expect(await tauriPage.isVisible(`${sheet} #server-secret`)).toBe(false)

    await tauriPage.click(`${sheet} .modal-footer button:last-of-type`)

    // The sheet hands off and the pane lands in the host's places list.
    await expect.poll(async () => !(await tauriPage.isVisible(sheet)), { timeout: waitBudget(30000) }).toBeTruthy()
    await expect
      .poll(async () => (await mcpReadResource('cmdr://state')).includes(SMB_GUEST_SHARE), {
        timeout: waitBudget(30000),
      })
      .toBeTruthy()
  })
})

describeSmb('SMB share browsing', () => {
  test('opening guest host shows its share list, with `..` first and the cursor on it', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()

    await openHost('left', 'SMB Test (Guest)', SMB_GUEST_SHARE)

    // The share list lives on the `network` volume, `..` is row 0, and the list opens with the cursor on it, the way a
    // file list opens on its parent row.
    const pane = await paneRows('left')
    expect(pane.volumeId).toBe('network')
    expect(pane.rows[0]).toBe('..')
    expect(pane.cursorIndex).toBe(0)
    // The standard mirror's spelling: the server's stable key, and the hub's own volume name.
    expect(pane.path).toBe('smb://smb test (guest)')
    expect(pane.volume).not.toContain(' > ')
  })

  test('nav_to_path onto a server opens its share list, and a share’s path still refuses', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()

    await mcpNavToPath('left', 'smb://smb test (guest)')
    await expect
      .poll(async () => shareExistsInPane('left', SMB_GUEST_SHARE), { timeout: waitBudget(30000) })
      .toBeTruthy()
  })

  test('a share list refuses F5 and MCP copy with words, and opens no transfer', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()
    await openHost('left', 'SMB Test (Guest)', SMB_GUEST_SHARE)
    await mcpCall('move_cursor', { pane: 'left', filename: SMB_GUEST_SHARE })

    const answer = await mcpCallRaw('copy', { pane: 'left' })
    expect(answer.error, 'a share list is no copy source').toBeDefined()

    await dispatchMenuCommand(tauriPage, 'file.copy')
    await expect
      .poll(async () => tauriPage.evaluate<string>('document.body.textContent || ""'), { timeout: waitBudget(5000) })
      .toContain('nothing here to copy')
    expect(await tauriPage.isVisible('[data-dialog-id="transfer"]')).toBe(false)
    await dismissOverlay(tauriPage)
  })

  test('a share row’s menu opens by right-click and by the keyboard, with Open and Copy path', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()
    await openHost('left', 'SMB Test (Guest)', SMB_GUEST_SHARE)
    await mcpCall('move_cursor', { pane: 'left', filename: SMB_GUEST_SHARE })
    const menuRows = async () =>
      tauriPage.evaluate<string[]>(
        `[...document.querySelectorAll('[data-menu] [data-menu-row]')].map(function (el) { return el.textContent.trim() })`,
      )

    // The keyboard's context-menu key, anchored at the cursor row.
    await pressKey(tauriPage, defaultKey('file.contextMenu'))
    await expect.poll(menuRows, { timeout: waitBudget(5000) }).toEqual(expect.arrayContaining(['Open', 'Copy path']))
    await escapeOverlayUntilGone(tauriPage, '[data-menu]')

    // A right-click on the share's row: the same menu, ❌ never the native file menu.
    await tauriPage.evaluate(`(function () {
      var row = document.querySelector('[data-filename=${JSON.stringify(SMB_GUEST_SHARE)}]');
      var box = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: box.left + 10, clientY: box.top + 5 }));
    })()`)
    await expect.poll(menuRows, { timeout: waitBudget(5000) }).toEqual(expect.arrayContaining(['Open', 'Copy path']))
    await escapeOverlayUntilGone(tauriPage, '[data-menu]')
  })

  test('`..` on a share list goes back to the Servers list with the cursor on that server', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // ❗ A server that isn't the list's first row: a Servers list opening on its first row would pass on the guest server.
    const server = 'SMB Test (50 Shares)'
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane(server), { timeout: waitBudget(15000) }).toBeTruthy()
    await mcpCall('move_cursor', { pane: 'left', filename: server })
    await mcpCall('open_under_cursor', {})
    await expect
      .poll(async () => (await sharesInPane('left'))?.length ?? 0, { timeout: waitBudget(30000) })
      .toBeGreaterThan(0)

    // `..` by its index: a file list's `move_cursor` finds rows by name in the backend listing, which has no `..`.
    await mcpCall('move_cursor', { pane: 'left', index: 0 })
    await mcpCall('open_under_cursor', {})

    await expect
      .poll(async () => (await hubRowUnderCursor('left'))?.startsWith(`${server}  protocol=smb`), {
        timeout: waitBudget(15000),
      })
      .toBeTruthy()
    expect((await paneRows('left')).cursorIndex, 'the server is not the first row').toBeGreaterThan(0)
  })

  test('re-selecting Servers from a share list returns the pane to the Servers list', async ({ tauriPage }) => {
    // Pre-fix the pane kept its open host on a volume re-select, so `select_volume
    // Network` from inside a host was a silent no-op and the tool timed out waiting
    // for the volume name to fall back from "Network > SMB Test (Guest)". With a
    // failed mount on screen that left an agent with no way out at all.
    // ❗ Also #324's "Servers lands on the hub": picking Servers never reopens (and so never re-lists) the last server.
    await ensureAppReady(tauriPage)

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()
    await openHost('left', 'SMB Test (Guest)', SMB_GUEST_SHARE)

    // The tool itself polls for the volume name, so a no-op surfaces as a timeout here.
    await mcpSelectVolume('left', 'Servers')

    await expect.poll(async () => hubRowUnderCursor('left'), { timeout: waitBudget(15000) }).not.toBeNull()
    const pane = await paneRows('left')
    expect(pane.volumeId).toBe('network')
    expect(pane.rows.some((row) => row.startsWith('SMB Test (Guest)  protocol=smb'))).toBe(true)
    expect(await sharesInPane('left')).toBeNull()
  })

  test('`..` in a share’s top folder opens its server’s share list on that share (#1238)', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    // The explainer shows once per install; put it back so this run sees it every time, retries included.
    await mcpCall('set_setting', { id: 'behavior.shareParentRowHintSeen', value: false })

    const mount = await awaitGuestMount()
    // Into the share the way a person gets there: open the server, then the share (already mounted, so it lands).
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()
    await openHost('left', 'SMB Test (Guest)', SMB_GUEST_SHARE)
    await mcpCall('move_cursor', { pane: 'left', filename: SMB_GUEST_SHARE })
    await mcpCall('open_under_cursor', {})
    await mcpAwaitPath('left', mount, 30)
    // The share's top folder leads out through `..`.
    await expect
      .poll(
        async () => {
          const pane = await paneRows('left')
          return pane.volumeId !== 'network' && pane.rows[0] === '..'
        },
        { timeout: waitBudget(15000) },
      )
      .toBeTruthy()

    await mcpCall('move_cursor', { pane: 'left', index: 0 })
    await mcpCall('open_under_cursor', {})

    await expect
      .poll(
        async () => {
          const pane = await paneRows('left')
          return pane.rows[0] === '..' ? shareNameOf(pane.rows[pane.cursorIndex] ?? '') : null
        },
        { timeout: waitBudget(30000) },
      )
      .toBe(SMB_GUEST_SHARE)
    expect((await paneRows('left')).volumeId).toBe('network')
    // The hint names where `..` went, once the cursor is on the share.
    await expectAndDismissToast(tauriPage, 'took you to', { timeout: waitBudget(10000) })
  })

  test('a server added by IP: `..` goes up to its share list on the share, and again to its row', async ({
    tauriPage,
  }) => {
    // QA 2026-11-26: a NAS reached by a Tailscale IP went up from a share to "Path not found: smb://<ip>/". The
    // mounted-only kind (macOS mounted it, Cmdr saved nothing) is pinned in Rust
    // (`every_kind_of_server_dot_dot_leads_to_is_one_its_share_list_resolves`): GVFS mounts Cmdr didn't make aren't
    // volumes here. This is the kind a person adds.
    await ensureAppReady(tauriPage)
    await mcpCall('set_setting', { id: 'behavior.shareParentRowHintSeen', value: true })
    const address = guestServerIp()
    test.skip(address === null, 'needs the guest server’s IP (Linux Docker)')
    if (address === null) return
    const name = 'SMB Test by IP'
    const added = await tauriPage.evaluate<{ host: { id: string } }>(
      `window.__TAURI_INTERNALS__.invoke('connect_to_server', ${JSON.stringify({
        address: `smb://${address}`,
        name,
        username: null,
        checkReachability: true,
      })})`,
    )
    try {
      await mcpSelectVolume('left', 'Servers')
      await expect.poll(async () => hostExistsInPane(name), { timeout: waitBudget(15000) }).toBeTruthy()
      await openHost('left', name, SMB_GUEST_SHARE)
      expect((await paneRows('left')).path).toBe(`smb://${address}`)
      await mcpCall('move_cursor', { pane: 'left', filename: SMB_GUEST_SHARE })
      await mcpCall('open_under_cursor', {})
      await expect
        .poll(
          async () => {
            const pane = await paneRows('left')
            return pane.volumeId !== 'network' && pane.rows[0] === '..'
          },
          { timeout: waitBudget(30000) },
        )
        .toBeTruthy()

      // Up out of the share: its server's share list, the cursor on the share.
      await mcpCall('move_cursor', { pane: 'left', index: 0 })
      await mcpCall('open_under_cursor', {})
      await expect
        .poll(
          async () => {
            const pane = await paneRows('left')
            return pane.volumeId === 'network' && pane.rows[0] === '..'
              ? shareNameOf(pane.rows[pane.cursorIndex] ?? '')
              : null
          },
          { timeout: waitBudget(30000) },
        )
        .toBe(SMB_GUEST_SHARE)
      expect((await paneRows('left')).path).toBe(`smb://${address}`)

      // Up out of the share list: the Servers list, the cursor on the server.
      await mcpCall('move_cursor', { pane: 'left', index: 0 })
      await mcpCall('open_under_cursor', {})
      await expect
        .poll(async () => (await hubRowUnderCursor('left'))?.startsWith(`${name}  protocol=smb`), {
          timeout: waitBudget(15000),
        })
        .toBeTruthy()
    } finally {
      await mcpSelectVolume('left', LOCAL_VOLUME_NAME)
      unmountGuestShareFrom(address)
      await tauriPage.evaluate(
        `window.__TAURI_INTERNALS__.invoke('forget_server', { id: ${JSON.stringify(added.host.id)} })`,
      )
    }
  })

  test('Copy path between panes with the cursor on a share mounts it in the other pane, once', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    const mount = await awaitGuestMount()

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()
    await openHost('left', 'SMB Test (Guest)', SMB_GUEST_SHARE)
    // `move_cursor` also focuses the left pane, which the cursor only refines the copy from.
    await mcpCall('move_cursor', { pane: 'left', filename: SMB_GUEST_SHARE })

    await dispatchMenuCommand(tauriPage, 'pane.copyPathLeftToRight')
    // The right pane opens the server's share list and mounts the share the cursor was on.
    await mcpAwaitPath('right', mount, 30)

    // The queued mount was spent: opening the same server again in the right pane stays on its share list.
    await mcpSelectVolume('right', 'Servers')
    await expect.poll(async () => hubRowUnderCursor('right'), { timeout: waitBudget(15000) }).not.toBeNull()
    await openHost('right', 'SMB Test (Guest)', SMB_GUEST_SHARE)
    // A round trip that needs the share list, after its listing landed: a second mount would have left it by now.
    await mcpCall('move_cursor', { pane: 'right', filename: SMB_GUEST_SHARE })
    expect(await shareExistsInPane('right', SMB_GUEST_SHARE)).toBe(true)
  })

  test('nav_to_path says so instead of reporting success from the host list', async ({ tauriPage }) => {
    // `resolve_location` maps every `smb://` path onto the virtual Network volume,
    // whose only navigable path is the `smb://` host-list sentinel. Pre-fix a longer
    // one committed the switch and acked OK while the pane sat on the host list.
    await ensureAppReady(tauriPage)

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()

    const answer = await mcpCallRaw('nav_to_path', {
      pane: 'left',
      path: `smb://SMB Test (Guest)/${SMB_GUEST_SHARE}`,
    })
    expect(answer.error, 'an smb:// path must be refused, not acked').toBeDefined()
    // Still the host list, and no share list opened behind the refusal.
    expect(await hostExistsInPane('SMB Test (Guest)')).toBe(true)
  })
})

describeSmb('SMB mounting and file browsing', () => {
  test('mounting guest share navigates to mounted path', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // The guest share is pre-mounted by smb-fixtures.ts setupSmb().
    // When the app tries to mount it, it gets EEXIST (already mounted) which is treated as success.

    // Switch to Network → open guest host → select share
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Guest)'), { timeout: waitBudget(15000) }).toBeTruthy()

    await mcpCall('move_cursor', { pane: 'left', filename: 'SMB Test (Guest)' })
    await mcpCall('open_under_cursor', {})
    await expect
      .poll(async () => shareExistsInPane('left', SMB_GUEST_SHARE), { timeout: waitBudget(30000) })
      .toBeTruthy()

    // Open the share (triggers mount)
    await mcpCall('move_cursor', { pane: 'left', filename: SMB_GUEST_SHARE })
    await mcpCall('open_under_cursor', {})

    // After mounting, the pane should navigate to the mounted volume path.
    // ❗ Wherever it landed (`/Volumes/public-1` when something else holds `/Volumes/public`), found by source.
    await expect.poll(() => guestMountPoint(), { timeout: waitBudget(30000) }).not.toBeNull()
    const mount = guestMountPoint() ?? ''
    await mcpAwaitPath('left', mount, 30)

    const state = await mcpReadResource('cmdr://state')
    expect(state).toContain(mount)
  })

  test('browse files on mounted guest share', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // Navigate directly to the pre-mounted share
    // (The share is mounted by setupSmb → preMountGuestShare)
    // ❗ Fails, naming the fixture and what's mounted, rather than skipping: a skip hid a mounting regression.
    const mount = requireGuestMount()

    await mcpNavToPath('left', mount)

    // The Docker guest container creates files in /share.
    // Verify we can read the directory (even if empty, the navigation should succeed).
    const state = await mcpReadResource('cmdr://state')
    expect(state).toContain(mount)
  })
})

describeSmb('SMB cross-storage copy', () => {
  test('copies file from local to mounted SMB share', async ({ tauriPage }) => {
    recreateFixtures(getFixtureRoot())
    await ensureAppReady(tauriPage)
    const fixtureRoot = getFixtureRoot()

    // ❗ The fixture's own share, found by source, with the suite folder visible through the mount. Fails, naming
    // the fixture and what's mounted, rather than skipping: a skip here hid both copy specs on every run.
    const suite = await requireGuestSuite()

    // Left pane: local fixtures (already set by ensureAppReady)
    // Right pane: suite-specific subdir on the mounted SMB share
    // (write isolation from the Rust integration tests — see
    // SMB_E2E_SUITE_DIR in smb-fixtures.ts).
    await mcpNavToPath('right', suite)

    // Copy file-a.txt from left to right
    await mcpCall('select', { pane: 'left', names: ['file-a.txt'] })
    await mcpConfirmedCopy('left')

    // Wait for the file to appear on the SMB share
    await mcpAwaitItem('right', 'file-a.txt', 30)

    // Verify on disk (the mount maps to Docker container volume)
    const copied = path.join(suite, 'file-a.txt')
    await expect.poll(() => fs.existsSync(copied), { timeout: waitBudget(10000) }).toBeTruthy()

    // Verify source still exists (copy, not move)
    expect(fs.existsSync(path.join(fixtureRoot, 'left', 'file-a.txt'))).toBe(true)
    // The copy's completion toast: seen, then closed, so it doesn't leak into the next spec.
    await expectAndDismissToast(tauriPage, 'Copied 1 file')
  })

  test('copies file from mounted SMB share to local', async ({ tauriPage }) => {
    recreateFixtures(getFixtureRoot())
    await ensureAppReady(tauriPage)
    const fixtureRoot = getFixtureRoot()

    // ❗ The fixture's own share, found by source, with the suite folder visible through the mount. Fails, naming
    // the fixture and what's mounted, rather than skipping: a skip here hid both copy specs on every run.
    const suite = await requireGuestSuite()

    // Write test file directly to the SMB server via smbclient (bypasses GVFS
    // caching; files written through the GVFS mount aren't immediately visible).
    // Target the suite-specific subdir so concurrent Rust integration tests
    // can never see this file.
    smbWriteFile(
      SMB_GUEST_HOST,
      SMB_GUEST_PORT,
      SMB_GUEST_SHARE,
      `${SMB_E2E_SUITE_DIR}/smb-test-file.txt`,
      'File from SMB share for E2E test.\n',
    )

    // Left pane: SMB share, Right pane: local fixtures right/
    await mcpNavToPath('left', suite)
    await mcpAwaitItem('left', 'smb-test-file.txt', 15)

    // Copy from SMB to local
    await mcpCall('select', { pane: 'left', names: ['smb-test-file.txt'] })
    await mcpConfirmedCopy('left')

    // Wait for file to appear in right pane
    await mcpAwaitItem('right', 'smb-test-file.txt', 30)

    // Verify on disk
    const localCopy = path.join(fixtureRoot, 'right', 'smb-test-file.txt')
    expect(fs.existsSync(localCopy)).toBe(true)
    expect(fs.readFileSync(localCopy, 'utf-8')).toContain('File from SMB share')
    await expectAndDismissToast(tauriPage, 'Copied 1 file')
  })
})

// ── Authentication tests ─────────────────────────────────────────────────────
//
// The auth Docker container (smb-auth, `guest ok = no`) turns a guest's share
// listing away, so opening the auth host asks for a sign-in. Nothing lists it
// before that: a host Cmdr only found is listed when someone opens it (#324).
//
// These tests verify that ask, and the share listing via IPC with credentials,
// which is the command the sign-in sheet's own attempt runs. The sheet itself is
// covered by the unit tests, which can drive its `attempt` directly; here the
// fixture's value is the real Samba round-trip.

describeSmb('SMB authentication', () => {
  test('opening the auth host asks for a sign-in, and cancelling stays on its Sign in until ⌘[ goes back', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    const sheet = '[data-dialog-id="server-sign-in"]'

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Auth)'), { timeout: waitBudget(30000) }).toBeTruthy()
    // Found, and not listed: no share count on its entry until someone opens it.
    expect(await hostEntryInHub('SMB Test (Auth)')).not.toContain('shares=')

    await mcpCall('move_cursor', { pane: 'left', filename: 'SMB Test (Auth)' })
    await mcpCall('open_under_cursor', {})
    await expect.poll(async () => tauriPage.isVisible(sheet), { timeout: waitBudget(30000) }).toBeTruthy()

    // "Not now" leaves the pane on the in-pane sign-in state, the honest "you're not signed in" place, with
    // keyboard focus on its Sign in. (A synthetic Enter doesn't press a button, so the test presses it by click.)
    await escapeOverlayUntilGone(tauriPage, sheet)
    const focusedSignIn = `(function () {
      var el = document.activeElement;
      return !!el && el.tagName === 'BUTTON' && !!el.closest('.error-pane') && el.textContent.trim() === 'Sign in';
    })()`
    await expect
      .poll(async () => tauriPage.evaluate<boolean>(focusedSignIn), { timeout: waitBudget(15000) })
      .toBeTruthy()

    // Sign in asks again; a second "Not now" lands on Sign in again.
    await tauriPage.evaluate(`document.activeElement.click()`)
    await expect.poll(async () => tauriPage.isVisible(sheet), { timeout: waitBudget(15000) }).toBeTruthy()
    await escapeOverlayUntilGone(tauriPage, sheet)
    await expect
      .poll(async () => tauriPage.evaluate<boolean>(focusedSignIn), { timeout: waitBudget(15000) })
      .toBeTruthy()

    // ⌘[ goes back to the hub, with the cursor on the server. Read off the DOM first: while the error screen is up,
    // `cmdr://state` still carries the hub's rows, so the mirror alone can't tell the two apart.
    await pressKey(tauriPage, defaultKey('nav.back'))
    await expect
      .poll(
        async () =>
          tauriPage.evaluate<boolean>(
            `!document.querySelector('.error-pane') && !!document.querySelector('.servers-hub')`,
          ),
        { timeout: waitBudget(15000) },
      )
      .toBeTruthy()
    await expect
      .poll(async () => (await hubRowUnderCursor('left'))?.startsWith('SMB Test (Auth)  protocol=smb'), {
        timeout: waitBudget(15000),
      })
      .toBeTruthy()
  })

  test('signing in on the sheet lists the server’s shares, and the Servers list names the account', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    const sheet = '[data-dialog-id="server-sign-in"]'

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Auth)'), { timeout: waitBudget(30000) }).toBeTruthy()
    await mcpCall('move_cursor', { pane: 'left', filename: 'SMB Test (Auth)' })
    await mcpCall('open_under_cursor', {})
    await expect.poll(async () => tauriPage.isVisible(sheet), { timeout: waitBudget(30000) }).toBeTruthy()

    // Typed, not remembered: nothing reaches the Keychain.
    await tauriPage.evaluate(`(function () {
      function type(selector, value) {
        var input = document.querySelector(selector);
        input.value = value;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }
      type('${sheet} #sign-in-username', ${JSON.stringify(SMB_AUTH_USERNAME)});
      type('${sheet} #sign-in-secret', ${JSON.stringify(SMB_AUTH_PASSWORD)});
    })()`)
    await tauriPage.click(`${sheet} .modal-footer button:last-of-type`)

    await expect.poll(async () => !(await tauriPage.isVisible(sheet)), { timeout: waitBudget(30000) }).toBeTruthy()
    await expect
      .poll(async () => shareExistsInPane('left', SMB_AUTH_SHARE), { timeout: waitBudget(30000) })
      .toBeTruthy()

    // Back on the Servers list, the server's row says who it's listed as.
    await mcpCall('move_cursor', { pane: 'left', index: 0 })
    await mcpCall('open_under_cursor', {})
    await expect
      .poll(async () => hubRowUnderCursor('left'), { timeout: waitBudget(15000) })
      .toMatch(new RegExp(`^SMB Test \\(Auth\\) {2}protocol=smb .*account=${SMB_AUTH_USERNAME}( |$)`))
  })

  /**
   * The hub row's menu is one of the account's three doors (with the path bar's account and the palette), all running
   * the one sign-in module. A Sign in as… that lists opens the server's share list, as activating the row does.
   */
  test('Sign in as… from a server row’s menu signs in and opens its share list', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    const sheet = '[data-dialog-id="server-sign-in"]'

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Auth)'), { timeout: waitBudget(30000) }).toBeTruthy()
    // The cursor on it opens its group, so its row is on screen to right-click.
    await mcpCall('move_cursor', { pane: 'left', filename: 'SMB Test (Auth)' })
    await expect
      .poll(
        async () =>
          tauriPage.evaluate<boolean>(`(function () {
            var row = [...document.querySelectorAll('[data-hub-row]')].find(function (el) {
              return el.textContent.includes('SMB Test (Auth)');
            });
            if (!row) return false;
            var box = row.getBoundingClientRect();
            row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: box.left + 20, clientY: box.top + 5 }));
            return true;
          })()`),
        { timeout: waitBudget(15000) },
      )
      .toBeTruthy()

    // The item by its value (`row:<rowId>:account:sign-in-as`), ❌ not its words.
    const signInRow = `document.querySelector('[data-menu] [data-menu-row$=":account:sign-in-as"]')`
    await expect
      .poll(async () => tauriPage.evaluate<boolean>(`${signInRow} !== null`), { timeout: waitBudget(15000) })
      .toBeTruthy()
    await tauriPage.evaluate(`${signInRow}.click()`)
    await expect.poll(async () => tauriPage.isVisible(sheet), { timeout: waitBudget(15000) }).toBeTruthy()

    // Typed, not remembered: nothing reaches the Keychain.
    await tauriPage.evaluate(`(function () {
      function type(selector, value) {
        var input = document.querySelector(selector);
        input.value = value;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }
      type('${sheet} #sign-in-username', ${JSON.stringify(SMB_AUTH_USERNAME)});
      type('${sheet} #sign-in-secret', ${JSON.stringify(SMB_AUTH_PASSWORD)});
    })()`)
    await tauriPage.click(`${sheet} .modal-footer button:last-of-type`)

    await expect.poll(async () => !(await tauriPage.isVisible(sheet)), { timeout: waitBudget(30000) }).toBeTruthy()
    await expect
      .poll(async () => shareExistsInPane('left', SMB_AUTH_SHARE), { timeout: waitBudget(30000) })
      .toBeTruthy()
  })

  test('listing shares with valid credentials returns private share', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // What the sign-in sheet sends when someone types the account in.
    const result = await listSharesOverIpc(
      tauriPage,
      { idPrefix: 'smb-e2e-auth-valid-', hostname: SMB_AUTH_HOST, port: SMB_AUTH_PORT },
      { username: SMB_AUTH_USERNAME, password: SMB_AUTH_PASSWORD, credentialSource: 'typed' },
    )

    const shareNames = result.shares.map((s) => s.name)
    expect(shareNames).toContain(SMB_AUTH_SHARE)
  })
})

// ── Diverse server tests ────────────────────────────────────────────────────
//
// These tests exercise Cmdr's UI against smb2's consumer containers with
// non-trivial data: many shares, unicode names, etc. They test discovery
// and share listing (no mounting needed).

describeSmb('SMB 50-share server', () => {
  test('50-share server lists all shares', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // List shares via IPC (bypasses UI, tests the backend share listing path)
    const result = await listSharesOverIpc(
      tauriPage,
      { idPrefix: 'smb-e2e-50shares-', hostname: SMB_50SHARES_HOST, port: SMB_50SHARES_PORT },
      AS_GUEST,
    )

    // smb2's consumer 50-shares container creates 50 shares
    expect(result.shares.length).toBeGreaterThanOrEqual(50)
  })

  test('50-share host shows correct share count in Network view', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (50 Shares)'), { timeout: waitBudget(30000) }).toBeTruthy()

    await openHostAndComeBack('SMB Test (50 Shares)')
    await expect
      .poll(async () => (await hostEntryInHub('SMB Test (50 Shares)'))?.includes('shares=50'), {
        timeout: waitBudget(30000),
      })
      .toBeTruthy()
  })

  test('typing a share’s letters on its share list moves the cursor to that share', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    // Typing filters by default; this pins Jump, through the path the Settings toggle takes.
    await mcpCall('set_setting', { id: 'fileExplorer.typeToJump.mode', value: 'jump' })
    try {
      await mcpSelectVolume('left', 'Servers')
      await expect
        .poll(async () => hostExistsInPane('SMB Test (50 Shares)'), { timeout: waitBudget(30000) })
        .toBeTruthy()
      await mcpCall('move_cursor', { pane: 'left', filename: 'SMB Test (50 Shares)' })
      await mcpCall('open_under_cursor', {})
      await expect
        .poll(async () => (await sharesInPane('left'))?.length ?? 0, { timeout: waitBudget(30000) })
        .toBeGreaterThanOrEqual(50)

      // The fixture's names are smb2's, so pick the target from the list: the last share whose typed letters
      // (letters and digits; anything else isn't a jump key) match no other share's name, so only it can win.
      const shares = (await sharesInPane('left')) ?? []
      const typed = (name: string) =>
        name
          .toLowerCase()
          .match(/[\p{L}\p{N}]/gu)
          ?.join('') ?? ''
      const isSubsequence = (needle: string, hay: string) => {
        let at = 0
        for (const c of hay) if (c === needle[at]) at++
        return at === needle.length
      }
      const target = [...shares]
        .reverse()
        .find(
          (name) =>
            typed(name) !== '' &&
            shares.every((other) => other === name || !isSubsequence(typed(name), other.toLowerCase())),
        )
      expect(target, `a share only its own letters match, among ${shares.join(', ')}`).toBeDefined()
      const targetName = target ?? ''
      expect(shares.indexOf(targetName), 'a share the cursor has to travel to').toBeGreaterThan(0)

      await focusPane(tauriPage, 0)
      for (const char of typed(targetName)) await pressKey(tauriPage, char)

      await expect
        .poll(async () => shareNameOf((await cursorRowText('left')) ?? ''), { timeout: waitBudget(5000) })
        .toBe(targetName)
      // Still the share list: the letters jumped, they didn't open or filter anything.
      const pane = await paneRows('left')
      expect(pane.volumeId).toBe('network')
      expect(pane.path).toBe('smb://smb test (50 shares)')
      // Escape clears the buffer and leaves the share list where it is.
      await pressKey(tauriPage, 'Escape')
    } finally {
      await mcpCall('set_setting', { id: 'fileExplorer.typeToJump.mode', value: 'filter' })
    }
  })
})

describeSmb('SMB unicode server', () => {
  test('unicode server lists shares with non-ASCII names', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // List shares via IPC
    const result = await listSharesOverIpc(
      tauriPage,
      { idPrefix: 'smb-e2e-unicode-', hostname: SMB_UNICODE_HOST, port: SMB_UNICODE_PORT },
      AS_GUEST,
    )

    // smb2's unicode container has shares with CJK, emoji, and accented names
    expect(result.shares.length).toBeGreaterThan(0)

    // At least one share name should contain non-ASCII characters
    const hasNonAscii = result.shares.some((s) => s.name.split('').some((c) => c.charCodeAt(0) > 127))
    expect(hasNonAscii).toBe(true)
  })

  test('unicode shares render correctly in share browser', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)

    // Switch to Network, open unicode host
    await mcpSelectVolume('left', 'Servers')
    await expect.poll(async () => hostExistsInPane('SMB Test (Unicode)'), { timeout: waitBudget(15000) }).toBeTruthy()

    await mcpCall('move_cursor', { pane: 'left', filename: 'SMB Test (Unicode)' })
    await mcpCall('open_under_cursor', {})

    // Wait for the share list to load, should show at least one share
    await expect
      .poll(async () => (await sharesInPane('left'))?.length ?? 0, { timeout: waitBudget(30000) })
      .toBeGreaterThan(0)

    // The names come through whole (not empty or garbled), non-ASCII ones included
    const shareNames = (await sharesInPane('left')) ?? []
    expect(shareNames.every((n) => n.length > 0)).toBe(true)
    expect(shareNames.some((n) => n.split('').some((c) => c.charCodeAt(0) > 127))).toBe(true)
  })
})
