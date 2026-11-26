/**
 * Every pane's footer band is one height, top edge included: a folder's (usage track +
 * `SelectionInfo`), the Servers hub's (plain rule + status bar), and a share list's. The
 * metrics live once, as the `--pane-footer-*` tokens in `app.css` and the `.pane-footer-edge`
 * class in `app-file-list.css`; this pins that each band reads them instead of its own numbers.
 * jsdom computes no CSS, so the pin reads the sources; the real heights were measured in the
 * running app.
 */
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { flushSync, mount, unmount } from 'svelte'
import selectionInfoSource from '../selection/SelectionInfo.svelte?raw'
import hubStatusBarSource from '../network/ServersHubStatusBar.svelte?raw'
import filePaneSource from './FilePane.svelte?raw'
import ServersHubStatusBar from '../network/ServersHubStatusBar.svelte'

// Read from disk: the test setup turns a `.css?raw` import into an empty string.
const appCss = readFileSync(path.resolve(process.cwd(), 'src/app.css'), 'utf8')
const fileListCss = readFileSync(path.resolve(process.cwd(), 'src/app-file-list.css'), 'utf8')

/** The declarations of the first rule for `selector` in `css`. */
function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`)
  expect(start, `no rule for ${selector}`).toBeGreaterThanOrEqual(0)
  return css.slice(start, css.indexOf('}', start))
}

describe('pane footer band', () => {
  it('defines the shared metrics once, as tokens', () => {
    for (const token of [
      '--pane-footer-edge-height',
      '--pane-footer-padding-block',
      '--pane-footer-font-size',
      '--pane-footer-min-height',
    ]) {
      expect(appCss).toContain(`${token}:`)
    }
  })

  it.each([
    ['SelectionInfo', selectionInfoSource, '.selection-info'],
    ['ServersHubStatusBar', hubStatusBarSource, '.hub-status-bar'],
  ])('%s takes its vertical metrics from the tokens', (_name, source, selector) => {
    const body = ruleBody(source, selector)
    expect(body).toMatch(/padding: var\(--pane-footer-padding-block\) /)
    expect(body).toContain('font-size: var(--pane-footer-font-size)')
    expect(body).toContain('min-height: var(--pane-footer-min-height)')
    expect(body).not.toMatch(/border(-top)?:\s*\d/)
  })

  it('the top edge is one shared class, as tall as the edge token', () => {
    const body = ruleBody(fileListCss, '.pane-footer-edge')
    expect(body).toContain('height: var(--pane-footer-edge-height)')
    // The usage bar fills the edge rather than carrying a height of its own.
    expect(ruleBody(filePaneSource, '.disk-usage-bar')).toContain('height: 100%')
    expect(filePaneSource).toContain('class="pane-footer-edge"')
  })

  it("the hub's top edge is decoration: hidden, with no role", () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    const instance = mount(ServersHubStatusBar, { target, props: { serverCount: 2, onRefresh: () => {} } })
    flushSync()
    const edge = target.querySelector('.pane-footer-edge')
    expect(edge).not.toBeNull()
    expect(edge?.getAttribute('aria-hidden')).toBe('true')
    expect(edge?.hasAttribute('role')).toBe(false)
    // The edge sits above the bar, inside the one footer the volume switcher stops at.
    expect(edge?.closest('[data-pane-footer]')?.querySelector('button.hub-status-bar')).not.toBeNull()
    void unmount(instance)
    target.remove()
  })
})
