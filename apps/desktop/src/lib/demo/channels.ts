/**
 * Tauri `Channel`s across the demo's frames. A `Channel` argument crosses the wire as
 * `__CHANNEL__:<id>`, where the id is a callback in the frame that made it, so two frames can
 * hold the same id. The webview tags each one with its window label on the way out
 * (`__CHANNEL__:<label>:<id>`), an engine sends a message back as a `demo://channel` notice
 * carrying that tag, and the backend hands it to the webview of that label, which runs the
 * callback the way Tauri's IPC does: `{ message, index }` in order, then `{ end: true, index }`.
 * See `DETAILS.md` § Channels.
 */

const PREFIX = '__CHANNEL__:'

/** One message on a channel, as an engine's `demo://channel` notice carries it. */
export type ChannelNotice = { channel: string; message: unknown } | { channel: string; end: true }

/** What a webview delivers to one of its channels. */
export type ChannelDelivery = { message: unknown } | { end: true }

/** `payload` with every channel in it tagged with the calling window's `label`. Pure: a copy. */
export function tagChannels(payload: unknown, label: string): unknown {
  if (typeof payload === 'string') {
    return payload.startsWith(PREFIX) && /^\d+$/.test(payload.slice(PREFIX.length))
      ? `${PREFIX}${label}:${payload.slice(PREFIX.length)}`
      : payload
  }
  if (Array.isArray(payload)) return payload.map((item) => tagChannels(item, label))
  if (payload !== null && typeof payload === 'object') {
    return Object.fromEntries(Object.entries(payload).map(([key, value]) => [key, tagChannels(value, label)]))
  }
  return payload
}

/** The window and callback id a tagged channel names, or `null` for anything else. */
export function channelTarget(channel: string): { label: string; id: number } | null {
  if (!channel.startsWith(PREFIX)) return null
  const rest = channel.slice(PREFIX.length)
  const split = rest.lastIndexOf(':')
  if (split <= 0) return null
  const id = Number(rest.slice(split + 1))
  return Number.isSafeInteger(id) ? { label: rest.slice(0, split), id } : null
}

/**
 * One frame's channel deliveries: numbers each message per channel, as Tauri's IPC does (the
 * `Channel` reorders by `index` and closes on `end`), through `run`, the frame's `runCallback`.
 */
export function createChannelDelivery(
  run: (id: number, data: unknown) => void,
): (id: number, delivery: ChannelDelivery) => void {
  const nextIndex = new Map<number, number>()
  return (id, delivery) => {
    const index = nextIndex.get(id) ?? 0
    if ('end' in delivery) {
      nextIndex.delete(id)
      run(id, { end: true, index })
      return
    }
    nextIndex.set(id, index + 1)
    run(id, { message: delivery.message, index })
  }
}
