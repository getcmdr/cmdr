/**
 * Pins the menu look tokens (`app-menu.css`): every token the house look defines has a Windows
 * value in light, dark (where it's a color), and contrast themes; the contrast themes speak only
 * system colors; and the Windows text tokens clear WCAG AA on the flyout, both on acrylic's solid
 * fallback and on acrylic over the worst backdrop. Sources: `docs/notes/windows-port/menu-look-tokens.md`.
 */
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { contrastRatio, parseHex, toHex } from '#lib/utils/srgb-mix.js'
import { findCssBeyondSafari15 } from '../scripts/vite-webkit-css-floor.ts'

// Read from disk: Vitest hands a `.css?raw` import back empty (it skips CSS by default).
const menuCss = readFileSync(path.join(import.meta.dirname, 'app-menu.css'), 'utf8')

interface Block {
  /** The enclosing at-rule preludes, outermost first, like `@media (prefers-color-scheme: dark)`. */
  atRules: string[]
  selector: string
  decls: Map<string, string>
}

/** A small brace walker: enough for a file of custom-property blocks. */
function parseBlocks(css: string): Block[] {
  const source = css.replace(/\/\*[\s\S]*?\*\//g, '')
  const blocks: Block[] = []
  const stack: string[] = []
  let cursor = 0
  let pending = ''
  while (cursor < source.length) {
    const ch = source[cursor]
    if (ch === '{') {
      const prelude = pending.trim().replace(/\s+/g, ' ')
      pending = ''
      if (prelude.startsWith('@')) {
        stack.push(prelude)
      } else {
        const end = source.indexOf('}', cursor)
        const decls = new Map<string, string>()
        for (const part of source.slice(cursor + 1, end).split(';')) {
          const colon = part.indexOf(':')
          if (colon > 0) decls.set(part.slice(0, colon).trim(), part.slice(colon + 1).trim())
        }
        blocks.push({ atRules: [...stack], selector: prelude, decls })
        cursor = end
      }
    } else if (ch === '}') {
      stack.pop()
      pending = ''
    } else {
      pending += ch
    }
    cursor++
  }
  return blocks
}

const blocks = parseBlocks(menuCss)

function block(selector: string, atRules: string[] = []): Map<string, string> {
  const found = blocks.find((b) => b.selector === selector && b.atRules.join('|') === atRules.join('|'))
  if (!found) throw new Error(`No block ${atRules.join(' ')} ${selector}`)
  return found.decls
}

const WINDOWS = ":root[data-look='windows']"
const DARK = '@media (prefers-color-scheme: dark)'
/** Contrast themes, behind the guard the Safari 15.4 floor accepts (`scripts/vite-webkit-css-floor.ts`). */
const FORCED_GUARD = '@supports (forced-color-adjust: none)'
const FORCED = '@media (forced-colors: active)'
const REDUCED = `${WINDOWS}.reduce-transparency`

const house = block(':root')
const light = block(WINDOWS)
const dark = block(WINDOWS, [DARK])
const forced = block(`${WINDOWS}, ${REDUCED}`, [FORCED_GUARD, FORCED])

/** A token whose value changes with the theme: a color, a shadow, or the backdrop. */
function isThemed(name: string): boolean {
  return /^--(color|shadow|glass)-/.test(name)
}

/** A literal color in the light block (hex or rgba), as opposed to `transparent` or a `var()`. */
function isLiteralColor(value: string): boolean {
  return /^#|^rgba\(/.test(value)
}

describe('menu look tokens', () => {
  it('gives every house token a Windows value', () => {
    expect([...house.keys()].filter((name) => !light.has(name))).toEqual([])
    expect([...light.keys()].filter((name) => !house.has(name))).toEqual([])
  })

  it('gives every Windows color that changes with the theme a dark value', () => {
    const missing = [...light.entries()]
      .filter(([name, value]) => isThemed(name) && isLiteralColor(value) && !dark.has(name))
      .map(([name]) => name)
    expect(missing).toEqual([])
  })

  it('gives every themed Windows token a contrast-theme value, in system colors only', () => {
    const missing = [...light.keys()].filter((name) => isThemed(name) && !forced.has(name))
    expect(missing).toEqual([])
    const systemColor = /^(Canvas|CanvasText|ButtonFace|ButtonText|Highlight|HighlightText|GrayText|transparent|none)$/
    const foreign = [...forced.entries()].filter(
      ([name, value]) => name.startsWith('--color-') && !systemColor.test(value),
    )
    expect(foreign).toEqual([])
  })

  it('maps the contrast theme the way WinUI does', () => {
    expect(forced.get('--color-menu-surface-bg')).toBe('Canvas')
    expect(forced.get('--color-menu-surface-border')).toBe('CanvasText')
    expect(forced.get('--spacing-menu-surface-border')).toBe('2px')
    expect(forced.get('--color-menu-row-fg')).toBe('ButtonText')
    expect(forced.get('--color-menu-row-bg-hover')).toBe('Highlight')
    expect(forced.get('--color-menu-row-fg-hover')).toBe('HighlightText')
    expect(forced.get('--color-menu-row-fg-disabled')).toBe('GrayText')
    expect(forced.get('--color-menu-shortcut-fg')).toBe('CanvasText')
    expect(forced.get('--glass-menu-backdrop')).toBe('none')
    expect(forced.get('--shadow-menu-surface')).toBe('none')
  })

  it('drops the blur and goes opaque without transparency effects', () => {
    const reduced = block(REDUCED)
    expect(reduced.get('--glass-menu-backdrop')).toBe('none')
    expect(reduced.get('--color-menu-surface-bg')).toBe('#f9f9f9')
    expect(block(REDUCED, [DARK]).get('--color-menu-surface-bg')).toBe('#2c2c2c')
  })

  it('lets a contrast theme win over reduced transparency: same selector weight, later in the file', () => {
    const indexOf = (selector: string, atRules: string[]) =>
      blocks.findIndex((b) => b.selector === selector && b.atRules.join('|') === atRules.join('|'))
    expect(indexOf(`${WINDOWS}, ${REDUCED}`, [FORCED_GUARD, FORCED])).toBeGreaterThan(indexOf(REDUCED, [DARK]))
  })

  it("starts the house bar's first title 10 px in and spaces the titles 2 px apart, as the demo's macOS bar measured", () => {
    const px = (name: string) => Number.parseFloat(house.get(name) ?? 'NaN')
    expect(px('--spacing-menu-bar-padding-inline') + px('--spacing-menu-bar-item-margin-inline')).toBe(10)
    expect(2 * px('--spacing-menu-bar-item-margin-inline')).toBe(2)
  })

  it('uses only CSS that Safari 15.4 reads, or guards it (the build fails otherwise)', () => {
    expect(findCssBeyondSafari15(menuCss)).toEqual([])
  })
})

/** `top` (any alpha) painted over an opaque `bottom`. */
function over(top: string, bottom: string): string {
  const t = parseHex(top)
  const b = parseHex(bottom)
  const mix = (front: number, back: number) => front * t.a + back * (1 - t.a)
  return toHex(mix(t.r, b.r), mix(t.g, b.g), mix(t.b, b.b))
}

describe('menu text contrast on Windows', () => {
  const themes = [
    { name: 'light', tokens: light, fallback: '#f9f9f9' },
    { name: 'dark', tokens: { get: (key: string) => dark.get(key) ?? light.get(key) }, fallback: '#2c2c2c' },
  ]
  // The fill acrylic paints (luminosity layer, then tint) over the worst page beneath it.
  const backdrops = ['#000000', '#ffffff']
  const textOn = [
    ['--color-menu-row-fg', ''],
    ['--color-menu-row-fg-hover', '--color-menu-row-bg-hover'],
    ['--color-menu-row-fg-hover', '--color-menu-row-bg-pressed'],
    ['--color-menu-shortcut-fg', ''],
    ['--color-menu-shortcut-fg-hover', '--color-menu-row-bg-hover'],
    ['--color-menu-bar-item-fg', '--color-menu-bar-item-bg-hover'],
  ] as const

  for (const theme of themes) {
    const token = (key: string): string => {
      const value = theme.tokens.get(key)
      if (value === undefined) throw new Error(`${theme.name} has no ${key}`)
      return value
    }
    const surfaces = [
      theme.fallback,
      ...backdrops.map((page) =>
        over(token('--color-menu-surface-tint'), over(token('--color-menu-surface-bg'), page)),
      ),
    ]
    for (const [fg, rowBg] of textOn) {
      it(`${theme.name}: ${fg}${rowBg ? ` on ${rowBg}` : ''} clears 4.5:1`, () => {
        for (const surface of surfaces) {
          const bg = rowBg ? over(token(rowBg), surface) : surface
          expect(contrastRatio(over(token(fg), bg), bg)).toBeGreaterThanOrEqual(4.5)
        }
      })
    }
  }
})
