/**
 * Every pane's footer band is one height, top edge included: a folder's (usage track +
 * `SelectionInfo`), and the Servers list's and a share list's (plain rule + `SelectionInfo`). The
 * metrics live once, as the `--pane-footer-*` tokens in `app.css` and the `.pane-footer-edge`
 * class in `app-file-list.css`; this pins that each band reads them instead of its own numbers.
 * jsdom computes no CSS, so the pin reads the sources; the real heights were measured in the
 * running app.
 */
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import selectionInfoSource from '../selection/SelectionInfo.svelte?raw'
import filePaneSource from './FilePane.svelte?raw'

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

  it.each([['SelectionInfo', selectionInfoSource, '.selection-info']])(
    '%s takes its vertical metrics from the tokens',
    (_name, source, selector) => {
      const body = ruleBody(source, selector)
      expect(body).toMatch(/padding: var\(--pane-footer-padding-block\) /)
      expect(body).toContain('font-size: var(--pane-footer-font-size)')
      expect(body).toContain('min-height: var(--pane-footer-min-height)')
      expect(body).not.toMatch(/border(-top)?:\s*\d/)
    },
  )

  it('the top edge is one shared class, as tall as the edge token', () => {
    const body = ruleBody(fileListCss, '.pane-footer-edge')
    expect(body).toContain('height: var(--pane-footer-edge-height)')
    // The usage bar fills the edge rather than carrying a height of its own.
    expect(ruleBody(filePaneSource, '.disk-usage-bar')).toContain('height: 100%')
    expect(filePaneSource).toContain('class="pane-footer-edge"')
  })
})
