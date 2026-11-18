/**
 * The transfer dialog's upfront warning about names Windows won't read as typed: the scan's
 * destination-blind tally (`NamePortabilityTally`, counted by the walk) read against the
 * destination's `NameRules`. Only the classes the destination WARNS about count: one it
 * refuses (a Linux driver's `:` or trailing period) the engine refuses per item before any
 * write, asking Skip, Skip all, or Stop (`cmdr-operations`' `name_rules.rs::DestinationNames`),
 * and a POSIX destination takes every name as is. Pure.
 */

import type { NameClassTally, NamePortabilityTally, NameRules } from '#lib/ipc/bindings.js'

export interface NamePortabilitySummary {
  /** How many entries' names won't work on Windows at the destination. */
  count: number
  /** Whether Linux misreads some of them too (a stand-in on a Windows-format drive). */
  alsoLinux: boolean
  /** One of the names, for the sentence. */
  example: string
}

/** The warning to show, or `null` when the destination keeps every name the walk found. */
export function namePortabilitySummary(
  tally: NamePortabilityTally | null,
  rules: NameRules,
): NamePortabilitySummary | null {
  if (tally === null) return null
  const warned = (tier: NameRules['windowsCharacters'], cls: NameClassTally): NameClassTally | null =>
    tier === 'warn' && cls.count > 0 ? cls : null
  // A stand-in (a character or a trailing space or period) on a Windows-format drive reads
  // differently on Linux too; a reserved name is only Windows' trouble.
  const standIns = [
    warned(rules.windowsCharacters, tally.windowsCharacters),
    warned(rules.windowsCharacters, tally.endsWithSpaceOrPeriod),
  ].filter((cls) => cls !== null)
  const reserved = warned(rules.reservedNames, tally.reservedNames)
  const classes = reserved === null ? standIns : [...standIns, reserved]
  const count = classes.reduce((sum, cls) => sum + cls.count, 0)
  const example = classes.flatMap((cls) => cls.examples).at(0)
  if (count === 0 || example === undefined) return null
  return { count, alsoLinux: rules.format === 'windows' && standIns.length > 0, example }
}
