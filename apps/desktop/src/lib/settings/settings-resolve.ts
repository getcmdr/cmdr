/**
 * Resolution: authored keys → rendered (getter-backed) definitions.
 *
 * `label`/`description`/`summary`/option labels are getters that resolve the catalog string
 * through `t()` at READ time. So every `getSettingDefinition(...).label` consumer gets a rendered
 * string, reactivity works in markup, and snapshot semantics hold in plain `.ts`. An option with a
 * literal `label` (brand names, numerals) passes through unchanged; option labels authored with a
 * `labelKey` resolve lazily.
 */

import type {
  EnumOption,
  EnumOptionSource,
  SettingConstraints,
  SettingConstraintsSource,
  SettingDefinition,
  SettingDefinitionSource,
} from './types'
import { tString } from '#lib/intl/messages.svelte.js'
import { shortcutAsText } from '#lib/shortcuts/reactive-shortcuts.svelte.js'

/** Resolves one authored option to a rendered `EnumOption` (getter-backed). */
function resolveOption(opt: EnumOptionSource | EnumOption): EnumOption {
  if ('label' in opt) return opt // literal label (brand names, numerals)
  const out: EnumOption = {
    value: opt.value,
    get label() {
      return tString(opt.labelKey)
    },
  }
  if (opt.icon !== undefined) out.icon = opt.icon
  if (opt.needsCapability !== undefined) out.needsCapability = opt.needsCapability
  if (opt.descriptionKey !== undefined) {
    const descKey = opt.descriptionKey
    Object.defineProperty(out, 'description', { enumerable: true, get: () => tString(descKey) })
  }
  return out
}

/** Resolves authored constraints, mapping option keys to rendered options. */
function resolveConstraints(c: SettingConstraintsSource | undefined): SettingConstraints | undefined {
  if (!c) return undefined
  const { options, ...rest } = c
  if (!options) return rest
  return { ...rest, options: options.map(resolveOption) }
}

/** Turns an authored source into a `SettingDefinition` with resolved copy. */
export function resolveDefinition(src: SettingDefinitionSource): SettingDefinition {
  // `labelKey` / `cardKey` stay on the definition (in `rest`): search indexes them in other languages.
  const { descriptionKey, descriptionWithKey, descriptionInTip, constraints, ...rest } = src
  const { labelKey, cardKey } = rest
  const summaryKey = typeof descriptionInTip === 'object' ? descriptionInTip.summaryKey : undefined
  const def = {
    ...rest,
    constraints: resolveConstraints(constraints),
    descriptionPlacement: descriptionInTip === undefined ? 'inline' : 'tip',
    get label() {
      return tString(labelKey)
    },
    get description() {
      // A key a hint names must run the command (`shortcutAsText` reads live keys).
      const combo = descriptionWithKey && shortcutAsText(descriptionWithKey.commandId, 'settingDescription')
      if (descriptionWithKey && combo !== undefined) return tString(descriptionWithKey.key, { combo })
      return descriptionKey === undefined ? '' : tString(descriptionKey)
    },
    get summary() {
      return summaryKey === undefined ? '' : tString(summaryKey)
    },
    get card() {
      return cardKey === undefined ? undefined : tString(cardKey)
    },
  } as SettingDefinition
  return def
}
