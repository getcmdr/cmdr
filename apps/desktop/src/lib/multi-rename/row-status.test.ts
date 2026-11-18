import { describe, it, expect } from 'vitest'
import type { RowStatus } from '#lib/ipc/bindings.js'
import { rowStatusView, statusWords, warnedRowView } from './row-status'

const PROBLEMS: RowStatus[] = [
  { type: 'invalidName', reason: { type: 'empty' } },
  { type: 'duplicate' },
  { type: 'targetExists' },
  { type: 'missing' },
  { type: 'noValue', field: 'exifTaken' },
  { type: 'unreadable', field: 'tagArtist' },
]

describe('rowStatusView', () => {
  it('keeps ready, unchanged, and reading rows quiet: no glyph, words for screen readers only', () => {
    for (const status of [{ type: 'ready' }, { type: 'unchanged' }, { type: 'reading' }] satisfies RowStatus[]) {
      const view = rowStatusView(status)
      expect(view.glyph).toBeNull()
      expect(view.isProblem).toBe(false)
      expect(view.label.key).toBeTruthy()
    }
    expect(rowStatusView({ type: 'ready' }).label.key).not.toBe(rowStatusView({ type: 'unchanged' }).label.key)
  })

  it('gives each kind of problem its own glyph, a short label, and the full reason', () => {
    const views = PROBLEMS.map(rowStatusView)
    expect(views.every((v) => v.isProblem && v.glyph !== null && v.reason !== null)).toBe(true)
    expect(new Set(views.map((v) => v.glyph)).size).toBe(PROBLEMS.length)
    expect(new Set(views.map((v) => v.label.key)).size).toBe(PROBLEMS.length)
  })

  it('names the reason a name is invalid, and the character a name can’t contain', () => {
    const reasons = (['empty', 'tooLong', 'reserved'] as const).map(
      (type) => rowStatusView({ type: 'invalidName', reason: { type } }).reason?.key,
    )
    expect(new Set(reasons).size).toBe(3)

    const slash = rowStatusView({ type: 'invalidName', reason: { type: 'disallowedCharacter', character: '/' } })
    expect(slash.glyph).toBe(rowStatusView({ type: 'invalidName', reason: { type: 'empty' } }).glyph)
    expect(slash.label.params).toEqual({ character: '/' })
    expect(slash.reason?.params).toEqual({ character: '/' })
  })

  it('names the media field a row misses, and offers the fallback for a date', () => {
    const artist = rowStatusView({ type: 'noValue', field: 'tagArtist' })
    expect(artist.label.params).toEqual({ field: { key: 'multiRename.mediaField.tagArtist' } })
    expect(artist.reason).toEqual({
      key: 'multiRename.reason.noValue',
      params: { field: { key: 'multiRename.mediaField.tagArtist' } },
    })
    expect(rowStatusView({ type: 'noValue', field: 'exifTaken' }).reason?.key).toBe('multiRename.reason.noDate')
    expect(rowStatusView({ type: 'unreadable', field: 'exifCamera' }).reason?.params).toEqual({
      field: { key: 'multiRename.mediaField.exifCamera' },
    })
  })
})

describe('statusWords', () => {
  it('words a message, its field param in words of its own', () => {
    const words = (key: string, params?: Record<string, string>) => `${key}(${JSON.stringify(params ?? {})})`
    expect(
      statusWords(
        { key: 'multiRename.status.noValue', params: { field: { key: 'multiRename.mediaField.tagArtist' } } },
        words,
      ),
    ).toBe('multiRename.status.noValue({"field":"multiRename.mediaField.tagArtist({})"})')
    expect(statusWords({ key: 'multiRename.status.ready' }, words)).toBe('multiRename.status.ready({})')
  })
})

describe('warnedRowView', () => {
  it('is no problem, but shows a glyph saying where the name won’t work', () => {
    const colon = { kind: 'windowsCharacter', character: ':' } as const
    const onStick = warnedRowView(colon, 'windows')
    expect(onStick.isProblem).toBe(false)
    expect(onStick.glyph).toBe('triangle-alert')
    expect(onStick.label).toEqual({ key: 'multiRename.status.notPortable', params: { alsoLinux: 'yes' } })
    expect(onStick.reason).toEqual({
      key: 'fileOperations.validation.notPortable.windowsCharacter',
      params: { format: 'drive', character: ':' },
    })
    // A share's stand-ins map back for Linux, and a reserved name is only Windows' trouble.
    expect(warnedRowView(colon, 'smbShare').label.params).toEqual({ alsoLinux: 'no' })
    expect(warnedRowView({ kind: 'reservedName', name: 'CON' }, 'windows').label.params).toEqual({ alsoLinux: 'no' })
  })
})
