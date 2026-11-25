/**
 * What `RemoteConnectView`'s buttons do. Its states and their a11y are pinned in
 * `connection-views.a11y.test.ts`; this pins that a button reaches the callback its
 * state hands over, since an inert button is the one thing the view refuses.
 */

import { describe, it, expect, vi, afterEach } from 'vitest'
import { mount, tick } from 'svelte'
import RemoteConnectView from './RemoteConnectView.svelte'
import { WAIT_REVEAL_AFTER_MS } from '#lib/ui/wait-reveal.js'

/** Lets a wait's quiet start run out, for a test about what the view shows after it. */
async function pastTheGrace(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, WAIT_REVEAL_AFTER_MS + 10))
  await tick()
}

describe('a changed host key', () => {
  it('opens the sheet on the new key from "Check the key", and disconnects from Disconnect', async () => {
    const checkKey = vi.fn()
    const disconnect = vi.fn()
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(RemoteConnectView, {
      target,
      props: { name: 'Testnas', state: { kind: 'host_key_changed', checkKey, disconnect } },
    })
    await tick()

    const buttons = [...target.querySelectorAll('button')]
    const check = buttons.find((button) => button.textContent.trim() === 'Check the key')
    expect(check, 'the banner offers a way to see the key').toBeDefined()
    check?.click()
    expect(checkKey).toHaveBeenCalledTimes(1)
    expect(disconnect).not.toHaveBeenCalled()

    buttons.find((button) => button.textContent.trim() === 'Disconnect')?.click()
    expect(disconnect).toHaveBeenCalledTimes(1)
    target.remove()
  })
})

describe('a refused guest session', () => {
  it('names the refusal, and opens the sheet from "Try another password…" or disconnects (#457)', async () => {
    const signIn = vi.fn()
    const disconnect = vi.fn()
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(RemoteConnectView, {
      target,
      props: { name: 'Testnas', state: { kind: 'guest_session_refused', signIn, disconnect } },
    })
    await tick()

    expect(target.textContent).toContain('Cmdr didn’t sign in to Testnas as a guest')
    const buttons = [...target.querySelectorAll('button')]
    buttons.find((button) => button.textContent.trim() === 'Try another password…')?.click()
    expect(signIn).toHaveBeenCalledTimes(1)
    expect(disconnect).not.toHaveBeenCalled()
    buttons.find((button) => button.textContent.trim() === 'Disconnect')?.click()
    expect(disconnect).toHaveBeenCalledTimes(1)
    target.remove()
  })
})

describe('the address under the name', () => {
  it('shows what the pane is trying to reach, so a person can tell a LAN address from a VPN one (ERR-VZ4CV)', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(RemoteConnectView, {
      target,
      props: { name: 'Testnas', address: '100.64.0.7:22', state: { kind: 'connecting', cancel: vi.fn() } },
    })
    await pastTheGrace()

    expect(target.textContent).toContain('100.64.0.7:22')
    target.remove()
  })
})

/**
 * A wait shows nothing for its first `WAIT_REVEAL_AFTER_MS`, the reveal a slow folder
 * shares (`#lib/ui/wait-reveal.ts`): a connect that lands inside it never flashes a spinner.
 * Every state that isn't a wait (a refusal, a sign-in) speaks at once.
 */
describe('the quiet start of a wait', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it.each([
    { label: 'a dial', state: { kind: 'connecting' as const, cancel: vi.fn() } },
    {
      label: 'a phone’s prompt',
      state: {
        kind: 'waiting_for_device' as const,
        reason: 'Allow it on the phone',
        hint: 'Tap Allow.',
        cancel: vi.fn(),
      },
    },
  ])('keeps $label’s view empty until the reveal, then offers Cancel', async ({ state }) => {
    vi.useFakeTimers()
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(RemoteConnectView, { target, props: { name: 'Testnas', state } })
    await tick()

    expect(target.querySelector('button'), 'no spinner or Cancel inside the grace').toBeNull()
    expect(target.querySelector('.remote-connect')?.textContent.trim()).toBe('')

    vi.advanceTimersByTime(WAIT_REVEAL_AFTER_MS - 1)
    await tick()
    expect(target.querySelector('button')).toBeNull()

    vi.advanceTimersByTime(1)
    await tick()
    const cancel = [...target.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Cancel')
    expect(cancel, 'Cancel once the view shows').toBeDefined()
    cancel?.click()
    expect(state.cancel).toHaveBeenCalledTimes(1)
    target.remove()
  })

  it('says a refusal at once', async () => {
    vi.useFakeTimers()
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(RemoteConnectView, {
      target,
      props: { name: 'Testnas', state: { kind: 'refused', refusal: 'Testnas said no.' } },
    })
    await tick()

    expect(target.textContent).toContain('Testnas said no.')
    target.remove()
  })
})

describe('a single dial with its own hint', () => {
  it('says the producer’s sentence under the spinner instead of the generic one (#604)', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(RemoteConnectView, {
      target,
      props: {
        name: 'testshare',
        state: { kind: 'connecting', cancel: vi.fn(), hint: 'Still waiting for macOS to connect.' },
      },
    })
    await pastTheGrace()

    expect(target.querySelector('.hint')?.textContent).toBe('Still waiting for macOS to connect.')
    expect(target.textContent).not.toContain('This can take a few seconds.')
    target.remove()
  })
})
