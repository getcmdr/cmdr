import { describe, expect, it } from 'vitest'
import { channelTarget, createChannelDelivery, tagChannels } from './channels.js'

describe('demo channels', () => {
  it('tags every channel in a payload with the window, and nothing else', () => {
    const payload = { request: { path: '__CHANNEL__:notes.txt' }, onEvent: '__CHANNEL__:7', list: ['__CHANNEL__:8', 1] }
    expect(tagChannels(payload, 'main')).toEqual({
      request: { path: '__CHANNEL__:notes.txt' },
      onEvent: '__CHANNEL__:main:7',
      list: ['__CHANNEL__:main:8', 1],
    })
  })

  it('reads a tagged channel back to its window and id, a label with colons too', () => {
    expect(channelTarget('__CHANNEL__:main:7')).toEqual({ label: 'main', id: 7 })
    expect(channelTarget('__CHANNEL__:a:b:12')).toEqual({ label: 'a:b', id: 12 })
    expect(channelTarget('__CHANNEL__:7')).toBeNull()
    expect(channelTarget('elsewhere')).toBeNull()
  })

  it('numbers each channel’s messages from zero and ends it at the next index', () => {
    const runs: [number, unknown][] = []
    const deliver = createChannelDelivery((id, data) => runs.push([id, data]))
    deliver(1, { message: 'a' })
    deliver(2, { message: 'x' })
    deliver(1, { message: 'b' })
    deliver(1, { end: true })
    expect(runs).toEqual([
      [1, { message: 'a', index: 0 }],
      [2, { message: 'x', index: 0 }],
      [1, { message: 'b', index: 1 }],
      [1, { end: true, index: 2 }],
    ])
  })
})
