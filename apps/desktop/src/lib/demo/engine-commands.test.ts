import { describe, expect, it } from 'vitest'
import { appAnswers, rawAnswers, silentCommands } from './answers.js'
import { LISTING_ENGINE_COMMANDS, MENU_ENGINE_COMMANDS } from './engine-command-names.js'
import { pluginHandler } from './plugins.js'
import { volumeAnswers } from './volumes.js'

/**
 * The backend tries its TS tables first, so a TS answer for an engine command would silently
 * shadow the real engine: the write engine, its journal, its undo planner, ⌘F, ⌃M, or the menu
 * state a popup's picks read (a TS `update_menu_context` once kept the menu engine from hearing it).
 */
const engineCommands = [...LISTING_ENGINE_COMMANDS, ...MENU_ENGINE_COMMANDS]

const camelCase = (snake: string) => snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase())

describe('the engine-owned commands', () => {
  it('have no TS answer to shadow the engine', () => {
    const tables = [appAnswers, volumeAnswers] as Record<string, unknown>[]
    for (const command of engineCommands) {
      const name = camelCase(command)
      expect(
        tables.some((table) => name in table),
        name,
      ).toBe(false)
      expect(rawAnswers[command], command).toBeUndefined()
      expect((silentCommands as readonly string[]).includes(name), name).toBe(false)
      expect(pluginHandler(command), command).toBeUndefined()
    }
  })
})
