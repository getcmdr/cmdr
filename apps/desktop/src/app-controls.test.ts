/**
 * Pins the control look tokens (`app-controls.css`):
 * - the house values, as a snapshot: macOS and Linux paint exactly these, so any change to one is
 *   a deliberate change to those looks (the catalog-wide computed-style proof that adopting the
 *   tokens moved nothing is in `docs/notes/windows-port/control-look-tokens.md`);
 * - every house token has a Windows value, every Windows color that changes with the theme a dark
 *   one, and every themed token a contrast-theme value written in system colors only;
 * - Windows' text clears WCAG AA and its control edges 3:1 on the surfaces they sit on.
 * Sources: `docs/notes/windows-port/control-look-tokens.md`.
 */
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { contrastRatio, parseHex, toHex } from '#lib/utils/srgb-mix.js'
import { findCssBeyondSafari15 } from '../scripts/vite-webkit-css-floor.ts'

// Read from disk: Vitest hands a `.css?raw` import back empty (it skips CSS by default).
const read = (file: string): string =>
  readFileSync(path.join(import.meta.dirname, file), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')

/** The custom properties of the rule `selector` in `css`, nested in exactly `media` (outermost first). */
function tokens(css: string, selector: string, media: string[] = []): Map<string, string> {
  const out = new Map<string, string>()
  const visit = (body: string, context: string[]): void => {
    let depth = 0
    let start = 0
    let prelude = ''
    for (let i = 0; i < body.length; i++) {
      if (body[i] === '{') {
        if (depth === 0) prelude = body.slice(start, i).trim()
        if (depth === 0) start = i + 1
        depth++
      } else if (body[i] === '}') {
        depth--
        if (depth === 0) {
          const inner = body.slice(start, i)
          if (prelude.startsWith('@')) visit(inner, [...context, prelude])
          else if (prelude === selector && context.join('|') === media.join('|')) {
            for (const decl of inner.split(';')) {
              const [name, ...value] = decl.split(':')
              if (name.trim().startsWith('--')) out.set(name.trim(), value.join(':').trim())
            }
          }
          start = i + 1
        }
      }
    }
  }
  visit(css, [])
  if (out.size === 0) throw new Error(`No tokens in ${media.join(' ')} ${selector}`)
  return out
}

const WINDOWS = ":root[data-look='windows']"
const DARK = '@media (prefers-color-scheme: dark)'
/** Contrast themes, behind the guard the Safari 15.4 floor accepts (`scripts/vite-webkit-css-floor.ts`). */
const FORCED = ['@supports (forced-color-adjust: none)', '@media (forced-colors: active)']

const windowsCss = read('app-controls-windows.css')
const house = tokens(read('app-controls.css'), ':root')
const light = tokens(windowsCss, WINDOWS)
const dark = tokens(windowsCss, WINDOWS, [DARK])
const forced = tokens(read('app-controls-windows-contrast.css'), WINDOWS, FORCED)

/** Windows redefines these `app.css` tokens (the dialog surface, the field corner and edge). */
const APP_CSS_TOKENS = [
  '--color-bg-dialog',
  '--color-dialog-border-outer',
  '--color-dialog-border-inner',
  '--radius-dialog',
  '--spacing-dialog',
  '--shadow-dialog',
  '--radius-input',
  '--color-field-border',
]
/** The house look draws native scrollbars, so these have no house value. */
const WINDOWS_ONLY = [
  '--color-scrollbar-thumb',
  '--color-scrollbar-thumb-hover',
  '--spacing-scrollbar-size',
  '--spacing-scrollbar-thumb-inset',
]

const isThemed = (name: string): boolean => /^--(color|shadow|glass)-/.test(name)
const SYSTEM_COLORS = new Set([
  'Canvas',
  'CanvasText',
  'ButtonFace',
  'ButtonText',
  'Highlight',
  'HighlightText',
  'GrayText',
])

describe('control look tokens', () => {
  it('keeps the house values (macOS and Linux paint exactly these)', () => {
    expect(Object.fromEntries(house)).toMatchSnapshot()
  })

  it.each(['app-controls.css', 'app-controls-windows.css', 'app-controls-windows-contrast.css', 'app-tooltip.css'])(
    '%s uses only CSS that Safari 15.4 reads, or guards it (the build fails otherwise)',
    (file) => {
      expect(findCssBeyondSafari15(read(file))).toEqual([])
    },
  )

  it('gives every house token a Windows value, and nothing else', () => {
    expect([...house.keys()].filter((name) => !light.has(name) && name !== '--color-forced-adjust')).toEqual([])
    const extra = [...light.keys()].filter((name) => !house.has(name))
    expect(extra.sort()).toEqual([...APP_CSS_TOKENS, ...WINDOWS_ONLY].sort())
    const appCss = readFileSync(path.join(import.meta.dirname, 'app.css'), 'utf8')
    for (const name of APP_CSS_TOKENS) expect(appCss, name).toContain(`${name}:`)
  })

  it('gives every Windows literal color a dark value unless WinUI uses the same one in both themes', () => {
    // Same value in WinUI's Light and Default dictionaries, or derived from the accent alone.
    const themeIndependent = new Set([
      '--color-button-primary-border', // ControlStrokeColorOnAccentDefault #14FFFFFF
      '--color-button-primary-border-top',
      '--color-button-primary-border-hover',
      '--color-dialog-border-outer', // SurfaceStrokeColorDefault #66757575
      '--color-dialog-scrim', // SmokeFillColorDefault #4D000000
    ])
    const missing = [...light.entries()]
      .filter(
        ([name, value]) =>
          isThemed(name) && /#[0-9a-f]{3,8}\b/i.test(value) && !dark.has(name) && !themeIndependent.has(name),
      )
      .map(([name]) => name)
    expect(missing).toEqual([])
  })

  it('gives every themed Windows token a contrast-theme value, in system colors only', () => {
    // A token that reads `app-menu.css`'s flyout material (the combobox list, the tooltip) is
    // covered by that file's own contrast block.
    const viaMenu = (value: string): boolean => /^var\(--(color|glass|shadow|spacing|radius)-menu-/.test(value)
    const missing = [...light.entries()]
      .filter(([name, value]) => isThemed(name) && !forced.has(name) && !viaMenu(value))
      .map(([name]) => name)
    expect(missing).toEqual([])
    for (const [name, value] of forced) {
      if (!name.startsWith('--color-') && !name.startsWith('--shadow-')) continue
      const words = value.replace(
        /linear-gradient|color-mix|in srgb|inset|solid|center|no-repeat|none|transparent|\d+(\.\d+)?(px|%)?|[(),/]/g,
        ' ',
      )
      const foreign = words.split(/\s+/).filter((word) => word && !SYSTEM_COLORS.has(word))
      expect(foreign, `${name}: ${value}`).toEqual([])
    }
  })
})

/** `top` (any alpha) painted over an opaque `bottom`. */
function over(top: string, bottom: string): string {
  const t = parseHex(top)
  const b = parseHex(bottom)
  const mix = (front: number, back: number): number => front * t.a + back * (1 - t.a)
  return toHex(mix(t.r, b.r), mix(t.g, b.g), mix(t.b, b.b))
}

describe('Windows control contrast', () => {
  // The surfaces a control sits on: the page, a card, the dialog, and its command space.
  const themes = [
    { name: 'light', get: (key: string) => light.get(key), pages: ['#ffffff', '#f5f5f5', '#f3f3f3'] },
    {
      name: 'dark',
      get: (key: string) => dark.get(key) ?? light.get(key),
      pages: ['#1e1e1e', '#2a2a2a', '#2b2b2b', '#202020'],
    },
  ]
  const text: [string, string][] = [
    ['--color-button-secondary-fg', '--color-button-secondary-bg'],
    ['--color-button-secondary-fg-hover', '--color-button-secondary-bg-hover'],
    ['--color-button-secondary-fg-pressed', '--color-button-secondary-bg-pressed'],
    ['--color-button-danger-fg', '--color-button-danger-bg'],
    ['--color-field-placeholder', '--color-field-bg'],
    ['--color-field-placeholder', '--color-field-bg-hover'],
    ['--color-select-trigger-fg', '--color-select-trigger-bg'],
    ['--color-select-trigger-fg', '--color-select-trigger-bg-hover'],
    ['--color-select-item-fg-highlighted', '--color-select-item-bg-highlighted'],
    ['--color-select-item-description-highlighted', '--color-select-item-bg-highlighted'],
    ['--color-radio-label', ''],
    ['--color-tooltip-fg', ''],
  ]
  // A control's outline is its affordance: WCAG 1.4.11's 3:1 against the surface.
  const edges = [
    '--color-checkbox-border',
    '--color-radio-border',
    '--color-field-border-bottom',
    '--color-slider-track',
    '--color-switch-thumb',
    '--color-scrollbar-thumb',
  ]

  for (const theme of themes) {
    const token = (key: string): string => {
      const value = theme.get(key)
      if (value === undefined || !value.startsWith('#'))
        throw new Error(`${theme.name} ${key} is not a literal color: ${value ?? 'undefined'}`)
      return value
    }
    for (const [fg, bg] of text) {
      it(`${theme.name}: the ${fg} text${bg ? ` on ${bg}` : ''} reaches 4.5:1`, () => {
        for (const page of theme.pages) {
          const surface = bg ? over(token(bg), page) : page
          expect(contrastRatio(over(token(fg), surface), surface)).toBeGreaterThanOrEqual(4.5)
        }
      })
    }
    for (const edge of edges) {
      it(`${theme.name}: the ${edge} edge reaches 3:1`, () => {
        for (const page of theme.pages) expect(contrastRatio(over(token(edge), page), page)).toBeGreaterThanOrEqual(3)
      })
    }
  }
})
