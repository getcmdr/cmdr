/**
 * What turning `network.enabled` off does to the Servers list.
 *
 * The switch gates mDNS discovery and SMB, which is what the macOS Local Network
 * permission is about; SFTP and WebDAV need none of it. So the list stays open and
 * keeps listing saved servers, and the only thing that changes is that the hosts
 * it would have found nearby are gone and the footer's count says the nearby
 * search is off.
 *
 * Covered here:
 * - The switcher row is called "Servers" whatever the switch says.
 * - Selecting it opens the Servers list in both states, rather than deflecting to Settings.
 * - With discovery off, the footer says so.
 *
 * Uses the `mcp-set-setting` event to write the setting from the test, which
 * triggers the same code path as the Settings UI (cache + cross-window emit +
 * `settings-applier` live-apply). This keeps the test focused on the Servers list's UX
 * without coupling it to settings-page navigation.
 *
 * Out of scope: macOS Local Network OS prompt timing (the TCC dialog isn't
 * driveable from automation). Validated manually.
 */

import { waitBudget } from './wait-budget.js'
import os from 'node:os'
import { test, expect } from './fixtures.js'
import { ensureAppReady, escapeOverlayUntilGone, isStateClean, pollUntil } from './helpers.js'
import { initMcpClient, mcpCall, mcpReadResource } from '../e2e-shared/mcp-client.js'

// Volume name for "Macintosh HD" on macOS / "Root" on Linux. We force both panes back to
// this volume in `beforeEach` so the spec runs cleanly even when a prior MTP test left a
// pane on a virtual MTP volume (mcp-nav-to-path won't cross volume boundaries).
const LOCAL_VOLUME_NAME = os.platform() === 'linux' ? 'Root' : 'Macintosh HD'

/** The Servers row's label, which is also Rust's `SERVERS_VOLUME_NAME` const. */
const SERVERS_VOLUME_NAME = 'Servers'

const PICKER_TRIGGER = '.volume-name'
const PICKER_DROPDOWN = '[data-menu]'
const ANY_VOLUME_ITEM = '[data-menu-row]'
/** The Servers list: the standard listbox, ending in its "Add server…" action row. */
const SERVERS_LIST = '.file-pane [role="listbox"] [data-trailing-row]'
/** The pane footers, whose count says whether the nearby search is on. */
const FOOTER = '.file-pane .selection-info'

async function footerText(tauriPage: Parameters<typeof pollUntil>[0]): Promise<string> {
  return tauriPage.evaluate<string>(`(document.querySelector(${JSON.stringify(FOOTER)})?.textContent ?? '')`)
}

/** Reads the visible label of the Servers row in the switcher. */
async function readServersLabel(tauriPage: Parameters<typeof pollUntil>[0]): Promise<string | null> {
  return tauriPage.evaluate<string | null>(`(function() {
    var items = document.querySelectorAll('[data-menu-row]');
    for (var i = 0; i < items.length; i++) {
      var label = items[i].querySelector('.volume-label');
      if (!label) continue;
      var text = label.textContent || '';
      if (text === ${JSON.stringify(SERVERS_VOLUME_NAME)}) return text;
    }
    return null;
  })()`)
}

/** Sets a setting through the MCP bridge (same code path the UI uses). */
async function setSettingViaBridge(settingId: string, value: unknown): Promise<void> {
  // The `set_setting` MCP tool uses `mcp_round_trip` and only returns after the
  // frontend handler has acknowledged the change. This replaces the prior
  // emit-and-sleep dance; no fixed-duration wait needed.
  await mcpCall('set_setting', { id: settingId, value })
}

async function openVolumePicker(tauriPage: Parameters<typeof pollUntil>[0]): Promise<void> {
  // If already open, no-op
  if (await tauriPage.isVisible(PICKER_DROPDOWN)) return
  await tauriPage.click(PICKER_TRIGGER)
  await tauriPage.waitForSelector(PICKER_DROPDOWN, 5000)
}

async function closeVolumePicker(tauriPage: Parameters<typeof pollUntil>[0]): Promise<void> {
  if (!(await tauriPage.isVisible(PICKER_DROPDOWN))) return
  // Dispatched at the dropdown, ❗ never at `document`: a document-level dispatch bubbles to
  // `window` and never descends into an element-bound handler, so it closes this dropdown by
  // luck of where the listener happens to sit.
  await escapeOverlayUntilGone(tauriPage, PICKER_DROPDOWN)
}

/** Puts the left pane on the Servers list and waits for it to render. */
async function openServersList(tauriPage: Parameters<typeof pollUntil>[0]): Promise<void> {
  await closeVolumePicker(tauriPage)
  await mcpCall('select_volume', { pane: 'left', name: SERVERS_VOLUME_NAME })
  await expect.poll(async () => tauriPage.isVisible(SERVERS_LIST), { timeout: waitBudget(15000) }).toBeTruthy()
}

test.describe('Local network discovery off', () => {
  test.beforeEach(async ({ tauriPage }) => {
    // Force both panes back to a local volume in case a prior MTP test left a pane on
    // a virtual MTP volume; `ensureAppReady`'s `mcp-nav-to-path` doesn't cross volume
    // boundaries, so we have to switch volumes explicitly first.
    //
    // Short-circuit: skip the volume-select + cmdr://state poll when both panes are
    // already on the local volume and no modal overlay is lingering. Common case for
    // non-first tests in the describe block.
    await initMcpClient(tauriPage)
    if (!(await isStateClean(tauriPage, LOCAL_VOLUME_NAME))) {
      await tauriPage.evaluate(`(function() {
        var invoke = window.__TAURI_INTERNALS__.invoke;
        invoke('plugin:event|emit', { event: 'mcp-volume-select', payload: { pane: 'left', name: '${LOCAL_VOLUME_NAME}' } });
        invoke('plugin:event|emit', { event: 'mcp-volume-select', payload: { pane: 'right', name: '${LOCAL_VOLUME_NAME}' } });
      })()`)
      // Wait for both panes to actually be on the local volume before asserting picker UX.
      await expect
        .poll(
          async () => {
            const state = await mcpReadResource('cmdr://state')
            const volumeLines = (state.match(/\n {2}volume: ([^\n]+)/g) ?? []).map((line) =>
              line.replace(/^\n {2}volume: /, ''),
            )
            return (
              volumeLines.length >= 2 && volumeLines[0] === LOCAL_VOLUME_NAME && volumeLines[1] === LOCAL_VOLUME_NAME
            )
          },
          { timeout: waitBudget(5000) },
        )
        .toBeTruthy()
    }

    await ensureAppReady(tauriPage)

    // Reset the toggle to its default in case a prior test left it off.
    await setSettingViaBridge('network.enabled', true)
    await closeVolumePicker(tauriPage)
  })

  test.afterEach(async ({ tauriPage }) => {
    // Close the volume picker the read-only tests leave open. They assert on the
    // dropdown's label and have no reason to dismiss it themselves, but the
    // fixtures safety-net afterEach (which runs AFTER this one — Playwright runs
    // afterEach hooks inner-to-outer) fails the test on a leaked `[data-menu]`.
    await closeVolumePicker(tauriPage)
    // Restore the default so the next spec file starts clean, and leave the pane
    // off the Servers list so the next spec doesn't inherit it.
    await setSettingViaBridge('network.enabled', true)
    await mcpCall('select_volume', { pane: 'left', name: LOCAL_VOLUME_NAME })
  })

  test('the switcher row is called "Servers" whether discovery is on or off', async ({ tauriPage }) => {
    await openVolumePicker(tauriPage)
    await tauriPage.waitForSelector(ANY_VOLUME_ITEM, 3000)
    expect(await readServersLabel(tauriPage)).toBe(SERVERS_VOLUME_NAME)

    await closeVolumePicker(tauriPage)
    await setSettingViaBridge('network.enabled', false)
    await openVolumePicker(tauriPage)
    await tauriPage.waitForSelector(ANY_VOLUME_ITEM, 3000)
    // Once this said "Network (disabled)" and the row refused to open. The
    // saved servers behind it never needed the permission the switch is about.
    expect(await readServersLabel(tauriPage)).toBe(SERVERS_VOLUME_NAME)
  })

  test('the Servers list opens with discovery off, and its footer says so', async ({ tauriPage }) => {
    await setSettingViaBridge('network.enabled', false)
    await openServersList(tauriPage)

    await expect
      .poll(async () => footerText(tauriPage), { timeout: waitBudget(5000) })
      .toContain('nearby search is off')
    // The rest of the list is untouched: the "Add server…" row is still the last one.
    expect(await tauriPage.isVisible(SERVERS_LIST)).toBe(true)
  })

  test('turning discovery back on takes the words away', async ({ tauriPage }) => {
    await setSettingViaBridge('network.enabled', false)
    await openServersList(tauriPage)
    await expect
      .poll(async () => footerText(tauriPage), { timeout: waitBudget(5000) })
      .toContain('nearby search is off')

    await setSettingViaBridge('network.enabled', true)
    await expect
      .poll(async () => footerText(tauriPage), { timeout: waitBudget(5000) })
      .not.toContain('nearby search is off')
  })
})
