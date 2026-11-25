/**
 * The one reveal rule both wait faces follow: invisible until 800 ms into the wait, then a
 * 200 ms fade, whenever in the wait the face mounted.
 */
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { WAIT_REVEAL_AFTER_MS, WAIT_REVEAL_RAMP_MS, waitRevealStyle } from './wait-reveal'

const here = path.dirname(fileURLToPath(import.meta.url))

describe('waitRevealStyle', () => {
  it('holds a face that mounts as the wait starts for the whole 800 ms', () => {
    expect(WAIT_REVEAL_AFTER_MS).toBe(800)
    expect(waitRevealStyle(0)).toBe(
      `--transition-wait-reveal-delay: 800ms; --transition-wait-reveal-ramp: ${String(WAIT_REVEAL_RAMP_MS)}ms`,
    )
  })

  it('counts the time a face mounted late into the wait, so every face shows at 800 ms', () => {
    expect(waitRevealStyle(100)).toContain('--transition-wait-reveal-delay: 700ms')
    expect(waitRevealStyle(WAIT_REVEAL_AFTER_MS)).toContain('--transition-wait-reveal-delay: 0ms')
    expect(waitRevealStyle(5_000)).toContain('--transition-wait-reveal-delay: 0ms')
  })
})

// Global CSS never reaches the test DOM, so the class is pinned at the source.
describe('.wait-reveal', () => {
  const css = readFileSync(path.join(here, '../../app-utilities.css'), 'utf8')

  it('fades in after the delay, and only appears under reduced motion', () => {
    expect(css).toMatch(
      /\.wait-reveal \{\s*animation: wait-reveal var\(--transition-wait-reveal-ramp\) ease-in var\(--transition-wait-reveal-delay\) both;/,
    )
    expect(css).toMatch(/prefers-reduced-motion: reduce\) \{\s*\.wait-reveal \{\s*animation-duration: 0ms;/)
  })
})
