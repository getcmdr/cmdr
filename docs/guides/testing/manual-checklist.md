# Manual test checklist for releases

Use this checklist before each release to verify features that cannot be automatically tested.

> **Note:** Many features now have automated tests. This checklist focuses only on what requires manual verification due
> to macOS/Tauri limitations (native menus, drag-and-drop, real file system).

## Pre-release verification

### App startup

- [ ] App launches within 5 seconds
- [ ] Both panes display home directory contents
- [ ] No error dialogs on startup

### Context menu (native macOS, cannot be automated)

- [ ] Right-click shows native context menu
- [ ] "Open" works
- [ ] "Show in Finder" opens Finder
- [ ] "Copy path" copies full path to clipboard
- [ ] "Get info" opens system Info panel
- [ ] "Quick Look" previews file (space bar alternative)
- [ ] "Duplicate" is in the context menu and in the File menu, and both make a `name (1)` copy beside the item

### Sort menus (native macOS, cannot be automated)

- [ ] View > Sort by checks the focused pane's column and order, and follows a focus change, a tab switch, and a pane
      swap
- [ ] In a fallback (sort by Date added inside a zip), View > Sort by still checks Date added
- [ ] After rebinding ⌘3 in Settings > Keyboard shortcuts, Name is still a checked check item
- [ ] The checkmark draws beside each Sort by item's icon
- [ ] Right-clicking the column header opens the Sort by popup (Date modified, Date created, Date added) in both Full
      and Brief, the pane's sort checked
- [ ] The Full date header's chevron opens the same popup by click, and by ←/→ then Enter or Space, anchored under it
- [ ] A right-click on the unfocused pane's header, then a pick, sorts that pane

### Drag-and-drop (Tauri plugin, cannot be automated)

- [ ] Drag file to Finder copies/moves it
- [ ] Drag file to other apps (like Mail) attaches it
- [ ] Drag icon shows file preview
- [ ] Dragging items onto their own pane does nothing (a sloppy drag can't reorder or copy anything)
- [ ] Holding ⌥ while dropping items on their own pane duplicates them as `name (1)`

### File watcher (requires real file system events)

- [ ] Create file in Finder → appears in Cmdr
- [ ] Delete file in Finder → disappears from Cmdr
- [ ] Rename file in Finder → updates in Cmdr

### Visual verification (requires human eye)

- [ ] Icons display correctly for common file types (.pdf, .txt, .app, folders)
- [ ] Symlink badge appears on symlinks
- [ ] Dropbox/iCloud sync icons appear in Dropbox/iCloud folders
- [ ] Columns in Brief mode fit filenames without truncation
- [ ] Loading indicator appears for large directories

### External volumes (requires physical hardware)

- [ ] External USB drive appears in volume picker
- [ ] Can navigate to external drive contents
- [ ] Network volumes appear (if mounted)
- [ ] Ejecting a volume gracefully switches pane to default

### The Servers list

- [ ] The volume switcher's Network group holds one row called "Servers"
- [ ] It lists saved servers and nearby hosts together, one row per server, in the standard file list (Name, Details,
      Last used), in both Brief and Full views, with the pane's own sort and filter
- [ ] "Add server…" is the last row under any sort or filter; arrow keys reach it and Enter opens the dialog
- [ ] Enter on an SFTP or WebDAV row takes the pane to the server (it dials in the pane, with a Cancel)
- [ ] Enter on an SMB server opens its share list; `..`, Backspace, and ⌘↑ go back to the Servers list, with the cursor
      on that server's row
- [ ] Right-click an SFTP or WebDAV row: Disconnect, Forget saved password, Forget server
- [ ] Turning off Settings > File systems > SMB/Network shares > network discovery leaves the Servers list open with
      only the servers you added, and its footer says the nearby search is off
- [ ] Palette: "Show servers", "Pin / unpin server", "Disconnect server", "Forget saved password" act on the Servers row
      under the cursor, or on the focused pane's own server
- [ ] Connect with hostname, IP, IP:port, smb:// URL
- [ ] Error shown for unreachable server
- [ ] Error shown for unsupported protocol (afp://, nfs://)
- [ ] Successful connect adds host, navigates to share list
- [ ] smb://server/share auto-mounts the specified share
- [ ] F8 on manual host shows confirmation, removes on confirm
- [ ] F8 on discovered host shows "Can't remove" toast
- [ ] Right-click manual host shows "Remove" confirmation
- [ ] Manual hosts persist across app restart
- [ ] Removing a manual host persists across app restart
- [ ] MCP `connect_to_server` tool works
- [ ] MCP `remove_manual_server` tool works
- [ ] MCP state shows `source=manual` for manual hosts

### Performance

- [ ] Directory with 1000 files loads in < 1 sec
- [ ] Directory with 50,000 files loads in < 5 sec
- [ ] UI remains responsive during loading
- [ ] Scrolling through 50k files is smooth (60 fps)
