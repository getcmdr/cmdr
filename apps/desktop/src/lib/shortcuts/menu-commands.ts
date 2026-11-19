/**
 * Commands that have corresponding native menu items with accelerators: the ones whose key the
 * shortcuts store (`shortcuts-store.ts`) pushes onto the menu bar on every change.
 *
 * Its own module so tests can verify the list matches the menu bar's command rows
 * (`crates/cmdr-menu/src/ids.rs`, checked by `rust-command-id-drift.test.ts`). Any command
 * listed here without a matching menu item silently drops the `updateMenuAccelerator` call on
 * the Rust side (no-op lookup), and any menu item not listed here misses out on accelerator
 * sync from custom shortcuts.
 */
export const menuCommands = [
  // View modes (CheckMenuItems, special handling in Rust)
  'view.fullMode',
  'view.briefMode',
  // Zoom (text-size) presets and step
  'view.zoom.set75',
  'view.zoom.set100',
  'view.zoom.set125',
  'view.zoom.set150',
  'view.zoom.in',
  'view.zoom.out',
  // View > Full screen (Windows and Linux; macOS's is AppKit's own item, so nothing syncs there)
  'view.toggleFullScreen',
  // File > Quit / Exit Cmdr (Windows and Linux; macOS quits from AppKit's app menu)
  'app.quit',
  // App-level
  'app.commandPalette',
  'app.settings',
  'app.checkForUpdates',
  'cmdr.openOnboarding',
  'app.toggleCliCommand',
  // Help (registered in MenuState.items so a future custom binding syncs its accelerator)
  'help.whatsNew',
  'help.viewDebugLog',
  'help.accessibility',
  // View > Operation queue (default ⌥⌘Q; registered so a custom binding syncs its accelerator)
  'queue.show',
  // View > Operation log (default ⌥⌘L; registered so a custom binding syncs its accelerator)
  'log.operationLog',
  // View > Suggested ops (no default; registered so a binding the user adds syncs its accelerator)
  'suggestedOps.show',
  // View > Ask Cmdr (default ⌥⌘A; registered so a custom binding syncs its accelerator)
  'askCmdr.toggle',
  // Edit menu (custom MenuItems; the focus-routed dispatch exception in
  // handle_menu_event doesn't change that their accelerators sync generically)
  'edit.cut',
  'edit.copy',
  'edit.paste',
  'edit.pasteAsMove',
  // File operations
  'file.view',
  'file.edit',
  'file.copy',
  'file.move',
  'file.duplicate',
  'file.compress',
  'file.newFolder',
  'file.newFile',
  'file.delete',
  'file.deletePermanently',
  'file.rename',
  'file.multiRename',
  'file.showInFinder',
  'file.openTerminalHere',
  'file.getInfo',
  'file.tellMeAbout',
  'file.quickLook',
  'file.copyPath',
  'file.copyFilename',
  'edit.undoTrash',
  'file.copyShareLink',
  'file.copyShareLinkOneDay',
  'file.copyShareLinkOneHour',
  // Cloud actions (macOS File Provider, items only show when the right-clicked file is in a cloud folder)
  'cloud.makeOffline',
  'cloud.removeDownload',
  // Google Drive (items only show when the right-clicked item resolves to a Drive ID)
  'cloud.openInGoogleDrive',
  'cloud.copyGoogleDriveLink',
  'cloud.askGemini',
  // Selection
  'selection.selectAll',
  'selection.deselectAll',
  'selection.selectSameKind',
  'selection.compareDirectories',
  'selection.invert',
  'selection.selectFiles',
  'selection.deselectFiles',
  // Panes
  'pane.switch',
  'pane.swap',
  'pane.clone',
  'view.calculateFolderSizes',
  // Search
  'search.open',
  // Sort
  'sort.byName',
  'sort.byExtension',
  'sort.byModified',
  'sort.bySize',
  // Navigation
  'nav.back',
  'nav.forward',
  'nav.parent',
  'nav.goToRoot',
  'nav.goHome',
  'nav.goToPath',
  // Downloads
  'downloads.goToLatest',
  // Favorites
  'favorites.add',
  'favorites.open',
  // Servers
  'servers.connect',
  'servers.show',
  // Tabs
  'tab.new',
  'tab.close',
  'tab.reopen',
  'tab.next',
  'tab.prev',
  'tab.closeOthers',
]
