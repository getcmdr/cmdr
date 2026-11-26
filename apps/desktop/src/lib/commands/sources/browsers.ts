/**
 * Share, server, and volume browsers (main window) command sources. Pure data (i18n message keys, not English); see
 * `../command-registry.ts` for how the scope arrays are concatenated into the
 * registry and resolved into `Command`s.
 */
import type { CommandSource } from '../types'
import { cmdOrCtrl, sameOnAll } from '../platform-shortcuts'
import { BLOCKED_BY_DIALOGS } from '../while-dialog-open'

export const browsersCommands: CommandSource[] = [
  // ============================================================================
  // A share's mount, on a server's share list
  // ============================================================================
  {
    // Escape cancels a running mount; Backspace and `⌘↑` (the file list's parent key)
    // step back from a refused one to the share list. Only while the mount's state is up
    // (`pane/share-mount.svelte.ts`); the share list itself goes back with `..`, so this
    // is off the palette. Display-only — `fixedKey` handling is in the controller.
    id: 'share.back',
    nameKey: 'commands.shareMountBack.label',
    scope: 'Main window/Places',
    showInPalette: false,
    shortcuts: {
      macos: ['Backspace', 'Escape', '⌘↑'],
      windows: ['Backspace', 'Escape', 'Alt+↑'],
      linux: ['Backspace', 'Escape', 'Alt+↑'],
    },
    whileDialogOpen: BLOCKED_BY_DIALOGS,
    fixedKey: true,
  },

  // ============================================================================
  // Servers
  // ============================================================================
  {
    // Takes the focused pane to the Servers list, wherever it is. The one command
    // here that needs no server under the cursor.
    id: 'servers.show',
    nameKey: 'commands.serversShow.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    id: 'servers.togglePin',
    nameKey: 'commands.serversTogglePin.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    id: 'servers.disconnect',
    nameKey: 'commands.serversDisconnect.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    id: 'servers.forgetSecret',
    nameKey: 'commands.serversForgetSecret.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    // An SMB server's account, as its Servers row menu and its share list's account menu offer
    // it: the Servers row under the cursor, else the server whose share list is up.
    id: 'servers.signInAs',
    nameKey: 'commands.serversSignInAs.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    id: 'servers.browseAsGuest',
    nameKey: 'commands.serversBrowseAsGuest.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    // The focused pane's SMB share's "Show “..” in the top folder" (#1238), the same switch
    // its row menu in the volume switcher carries. Offered only on an SMB share.
    id: 'servers.toggleShareParentRow',
    nameKey: 'commands.serversToggleShareParentRow.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: sameOnAll(),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
    paletteCondition: 'focusedPaneOnSmbShare',
  },
  {
    // ⌘K, Finder's binding for the same thing. ❗ Window-wide, ❌ not the Servers
    // scope: adding a server is about no server in particular, so it works from
    // a file list as readily as from the Servers list.
    id: 'servers.connect',
    nameKey: 'commands.serversConnect.label',
    scope: 'Main window',
    showInPalette: true,
    shortcuts: cmdOrCtrl('⌘K'),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },
  {
    // ⌘E lives in the Servers scope, a sibling of the file list's, so it can
    // never meet a file-list binding.
    id: 'servers.edit',
    nameKey: 'commands.serversEdit.label',
    scope: 'Main window/Servers',
    showInPalette: true,
    shortcuts: cmdOrCtrl('⌘E'),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
  },

  // ============================================================================
  // Volume chooser
  // ============================================================================
  {
    id: 'volume.select',
    nameKey: 'commands.volumeSelect.label',
    scope: 'Main window/Volume chooser',
    showInPalette: false,
    shortcuts: sameOnAll('Enter'),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
    fixedKey: true,
  },
  {
    id: 'volume.close',
    nameKey: 'commands.volumeClose.label',
    scope: 'Main window/Volume chooser',
    showInPalette: false,
    shortcuts: sameOnAll('Escape'),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
    fixedKey: true,
  },

  // ============================================================================
  // Favorites menu
  // ============================================================================
  // The two digit rows the open menu answers itself, here so Settings > Keyboard shortcuts
  // and the Help window say the menu HAS them. `fixedKey`, like the chooser's Enter: the
  // house `Menu`'s accelerator column decides which digits exist from the caller's data, so
  // a rebind would be an illusion. The command that OPENS the menu is `favorites.open`
  // (`main-window.ts`), which is window-wide.
  {
    id: 'favorites.openByNumber',
    nameKey: 'commands.favoritesOpenByNumber.label',
    scope: 'Main window/Favorites menu',
    showInPalette: false,
    shortcuts: sameOnAll('1', '2', '3', '4', '5', '6', '7', '8', '9'),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
    fixedKey: true,
  },
  {
    id: 'favorites.addFromMenu',
    // The one `nameKey` outside `commands.*`, and deliberately so: this row doesn't name a
    // command, it quotes the menu's `0` row back to the reader. The row owns the wording
    // (`favorites-menu.svelte.ts` draws it from the same key), so a second copy under
    // `commands.*` was one English edit away from the two disagreeing, and five locales
    // already rendered them byte-identical.
    nameKey: 'fileExplorer.navigation.favoritesAddCurrent',
    scope: 'Main window/Favorites menu',
    showInPalette: false,
    shortcuts: sameOnAll('0'),
    whileDialogOpen: BLOCKED_BY_DIALOGS,
    fixedKey: true,
  },
]
