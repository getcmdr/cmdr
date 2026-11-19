import { describe, expect, it } from 'vitest'
// A runtime import on purpose: it puts `bindings.ts` in this test's module graph, so
// `svelte-tests-related` runs it on the change that adds a command.
import { commands } from '#lib/ipc/bindings.js'
import { answeredCommands, notInDemo } from './coverage.js'

const bindingNames = Object.keys(commands)

describe("the demo's command coverage", () => {
  it('answers every command the app can call, or says why the demo leaves it out', () => {
    const declined = new Set(Object.keys(notInDemo))
    const gaps = bindingNames.filter((name) => !answeredCommands.has(name) && !declined.has(name))
    // A gap here is a command with no demo answer. Answer it (`DETAILS.md` § Answering a command) or
    // add it to `notInDemo` with the reason a browser can't.
    expect(gaps).toEqual([])
  })

  it('leaves out only commands that exist and nobody answers, each with a reason', () => {
    const known = new Set(bindingNames)
    for (const [name, reason] of Object.entries(notInDemo)) {
      expect(known.has(name), `${name} is no command`).toBe(true)
      expect(answeredCommands.has(name), `${name} is answered, so it isn't left out`).toBe(false)
      expect(reason.trim(), name).not.toBe('')
    }
  })
})
