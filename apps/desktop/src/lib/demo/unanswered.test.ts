import { afterEach, describe, expect, it, vi } from 'vitest'
import { exposeUnanswered, noteUnanswered } from './unanswered.js'

afterEach(() => {
  vi.restoreAllMocks()
})

function seen(): Map<string, number> {
  return (window as unknown as { __cmdrDemo: { unanswered: Map<string, number> } }).__cmdrDemo.unanswered
}

describe('unanswered commands', () => {
  it('says out loud, once per command, that nothing answers a command the demo never heard of', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    exposeUnanswered()

    expect(() => noteUnanswered('mystery_command', { a: 1 })).toThrow(/mystery_command/)
    expect(() => noteUnanswered('mystery_command', { a: 2 })).toThrow(/mystery_command/)

    expect(seen().get('mystery_command')).toBe(2)
    expect(error).toHaveBeenCalledTimes(1)
  })

  it('stays quiet for a command the demo leaves out on purpose, and keeps it off the unanswered list', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    vi.spyOn(console, 'debug').mockImplementation(() => undefined)
    exposeUnanswered()

    // `coverage.ts`'s `notInDemo`: a value command and a void one.
    expect(() => noteUnanswered('get_recent_paths', {})).toThrow(/get_recent_paths/)
    expect(noteUnanswered('show_notification', {})).toBeNull()

    expect(seen().has('get_recent_paths')).toBe(false)
    expect(seen().has('show_notification')).toBe(false)
    expect(error).not.toHaveBeenCalled()
  })

  it('resolves a void command, as its caller expects nothing back', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
    // `Promise<void>` in the bindings, and a `Result<(), E>` one, whose success carries `null`.
    expect(noteUnanswered('cancel_listing', {})).toBeNull()
    expect(noteUnanswered('show_notification', {})).toBeNull()
  })

  it('rejects a command that owes an answer, the way Tauri rejects one it lacks', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
    vi.spyOn(console, 'debug').mockImplementation(() => undefined)
    // A `null` here once reached the search dialog as a real answer and crashed it.
    expect(() => noteUnanswered('get_index_disk_usage', {})).toThrow(/get_index_disk_usage/)
    expect(() => noteUnanswered('not_a_command_at_all', {})).toThrow(/not_a_command_at_all/)
  })
})
