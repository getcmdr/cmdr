/**
 * The text-field gate: a command that moves or removes files (`FILE_MOVING_COMMAND_IDS`) never
 * runs from a key or the menu bar while a text field has focus, on any OS.
 *
 * Two roads carry a key into a command, and both ask here. The keyboard resolver
 * (`global-keydown.ts`) leaves the key unclaimed, so the field gets it (`⌘⌫` deletes to the line
 * start, `Delete` forward-deletes). The dispatch core refuses the menu road too: a bar accelerator
 * (macOS's `F8`, `⇧F8`, `F6`, `⌥⌘V`, or any key a user bound) fires before the field sees the
 * key, and the menu event can't say whether a key or a click sent it, so on macOS a click on File >
 * Delete with a field focused does nothing either. Off macOS the page draws the bar and does know:
 * a row the person CLICKED there arrives as `barClick` (`pick_menu_bar_item`'s `via: 'click'`) and
 * runs. Every key stays refused: the command's own key (`keyboard`, or `menu` when another window
 * fires it) and the open menu's own keys too (Alt or F10 then letters, Alt+letter then a letter,
 * arrows then Enter or Space, in the bar or the main-menu button: `via: 'key'`, the `menu` road),
 * so typing never moves files, however the menu was reached (review of #1281). The palette, the
 * F-key bar, MCP, and a page bar's click run: each is a deliberate pick of the command itself.
 *
 * A page bar's key from a menu opened over a text field arrives as `barKeyInTextField` and is
 * refused whatever has focus when it lands: focus is read at dispatch, a round trip after the pick,
 * and once an app effect pulled it into the file list meanwhile, so File > Delete trashed a file
 * from a field (#1464). The pick's target is where focus was when the menu opened.
 */
import { movesFiles, type CommandId } from '#lib/commands/index.js'
import type { DispatchSource } from './command-dispatch-context'

/** Whether the dispatch core refuses `commandId` from `source` because a text field has (or had) focus. */
export function isRefusedInTextField(commandId: CommandId, source: DispatchSource, textInputFocused: boolean): boolean {
  if (source === 'barKeyInTextField') return movesFiles(commandId)
  if (!textInputFocused) return false
  if (source !== 'keyboard' && source !== 'menu') return false
  return movesFiles(commandId)
}
