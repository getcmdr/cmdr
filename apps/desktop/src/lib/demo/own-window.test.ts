import { describe, expect, it, vi } from 'vitest'

const fullScreen = vi.hoisted(() => ({ on: false, set: [] as boolean[] }))
vi.mock('./browser-full-screen.js', () => ({
  isBrowserFullScreen: () => fullScreen.on,
  setBrowserFullScreen: (on: boolean) => {
    fullScreen.set.push(on)
    return Promise.resolve()
  },
}))
const asked = vi.hoisted(() => [] as unknown[])
vi.mock('./question-dialog.js', () => ({
  askQuestion: (payload: unknown) => {
    asked.push(payload)
    return Promise.resolve('Ok')
  },
}))

import { EVENT_COMMANDS, WEBVIEW_COMMANDS, windowAnswer } from './own-window.js'

const answer = (cmd: string, payload: Record<string, unknown> = {}) => windowAnswer(cmd, payload)?.()

describe("a webview's answers about its own window", () => {
  it('answers its size, scale, and position off the frame', () => {
    const scale = window.devicePixelRatio
    expect(answer('plugin:window|inner_size')).toEqual({
      width: Math.round(window.innerWidth * scale),
      height: Math.round(window.innerHeight * scale),
    })
    expect(answer('plugin:window|outer_size')).toEqual(answer('plugin:window|inner_size'))
    expect(answer('plugin:window|scale_factor')).toBe(scale)
    expect(answer('plugin:window|outer_position')).toEqual({ x: 0, y: 0 })
    expect(answer('plugin:window|inner_position')).toEqual({ x: 0, y: 0 })
  })

  it('answers its state, and takes the page-owned calls as done', () => {
    document.title = 'Cmdr'
    expect(answer('plugin:window|is_maximized')).toBe(false)
    expect(answer('plugin:window|is_visible')).toBe(true)
    expect(answer('plugin:window|is_focused')).toBe(document.hasFocus())
    expect(answer('plugin:window|title')).toBe('Cmdr')
    for (const cmd of ['plugin:window|set_title', 'plugin:window|set_min_size', 'plugin:window|set_max_size']) {
      expect(answer(cmd)).toBeNull()
    }
  })

  it("drives the browser's full screen for the window's", async () => {
    fullScreen.on = true
    expect(answer('plugin:window|is_fullscreen')).toBe(true)
    await expect(answer('plugin:window|set_fullscreen', { value: true })).resolves.toBeNull()
    await answer('plugin:window|set_fullscreen', { value: false })
    expect(fullScreen.set).toEqual([true, false])
  })

  it('asks a question over itself', async () => {
    await expect(answer('plugin:dialog|message', { message: 'Sure?' })).resolves.toBe('Ok')
    expect(asked).toEqual([{ message: 'Sure?' }])
  })

  it('owns no other command, and lists every one it does own', () => {
    expect(windowAnswer('plugin:window|close', {})).toBeUndefined()
    expect(WEBVIEW_COMMANDS).toEqual(expect.arrayContaining([...EVENT_COMMANDS, 'plugin:dialog|message']))
  })
})
