/**
 * Tests for `publishScreenshots`: the copy that moves a green capture's PNGs into
 * the machine-wide store, so they outlive the worktree that took them.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { publishScreenshots } from './screenshot-store.ts'

let root: string

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'cmdr-screenshot-store-'))
})

afterEach(() => {
  rmSync(root, { recursive: true, force: true })
})

describe('publishScreenshots', () => {
  it('copies top-level PNGs and replaces older same-named ones', () => {
    const from = join(root, 'worktree')
    const to = join(root, 'store')
    mkdirSync(from)
    mkdirSync(to)
    writeFileSync(join(from, 'a.png'), 'new a')
    writeFileSync(join(from, 'b.png'), 'b')
    writeFileSync(join(to, 'a.png'), 'old a')
    writeFileSync(join(to, 'kept.png'), 'kept')

    expect(publishScreenshots(from, to)).toBe(2)
    expect(readFileSync(join(to, 'a.png'), 'utf8')).toBe('new a')
    expect(readFileSync(join(to, 'b.png'), 'utf8')).toBe('b')
    // An image this run didn't take stays: the store is additive.
    expect(readFileSync(join(to, 'kept.png'), 'utf8')).toBe('kept')
  })

  it('leaves the JSON artifacts and the overflow subfolder alone', () => {
    const from = join(root, 'worktree')
    const to = join(root, 'store')
    mkdirSync(join(from, 'overflow'), { recursive: true })
    writeFileSync(join(from, 'capture-report.json'), '{}')
    writeFileSync(join(from, 'overflow', 'x.png'), 'x')

    expect(publishScreenshots(from, to)).toBe(0)
    expect(existsSync(join(to, 'capture-report.json'))).toBe(false)
    expect(existsSync(join(to, 'overflow'))).toBe(false)
  })

  it('is a no-op when the run already wrote into the store', () => {
    const dir = join(root, 'main-clone')
    mkdirSync(dir)
    writeFileSync(join(dir, 'a.png'), 'a')
    expect(publishScreenshots(dir, dir)).toBe(0)
  })

  it('creates the store when it is missing and skips a missing source', () => {
    const from = join(root, 'worktree')
    mkdirSync(from)
    writeFileSync(join(from, 'a.png'), 'a')
    expect(publishScreenshots(from, join(root, 'fresh', 'store'))).toBe(1)
    expect(publishScreenshots(join(root, 'nope'), join(root, 'store'))).toBe(0)
  })
})
