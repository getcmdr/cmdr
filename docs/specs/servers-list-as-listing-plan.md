# The Servers list becomes a real listing

Follows getcmdr/cmdr#1511 (a server's share list became a real listing; that plan is in git at
`24eb5c55b2:docs/specs/share-list-as-listing-plan.md`, and this one copies its shape). Tracking issue:
getcmdr/cmdr#1517.

**Problem.** The Servers view (the `network` volume at `smb://`) is still a hand-rolled table (`ServersHub.svelte`,
1,031 lines, plus nine sibling modules and `NetworkMountView.svelte`), while one level down, a server's share list is
now a standard listing. So the two levels look and behave differently: the hub has its own header, cursor, keys,
type-to-jump, footer, MCP push, and row menu host; it nests a connected server's shares (saved ones, then unsaved ones,
then a "More shares on…" row) under it; and it folds found-only hosts into a group. David's QA (2026-11-26): "feels like
too much magic". Entering a nested share and going up lands on the share list, not where you were. Servers that only
have an OS mount (David's NAS by `192.168.1.111` and by `100.127.48.122`) don't show at all.

**Goal (David, 2026-11-26).**

1. The Servers view is a FLAT list of servers, rendered by the standard pane views (Full/Brief, column header, sort,
   quick filter, type-to-jump, `SelectionInfo` footer, the standard MCP mirror), exactly as the share list now is. No
   share rows under a server. Enter on a server opens its share list; `..` from a share list lands on the Servers list
   with the cursor on that server; `..` out of a share's top folder lands on its share list (#1238, unchanged).
2. "+ Add server…" keeps a natural home: F7, the palette, the empty list's words, AND a last "+ Add server…" row that
   the views draw through one generic per-kind trailing-row seam (§ "Decisions", 1; branch 13).
3. The "NFS servers don't announce themselves…" line goes.
4. Every server with a mounted share (Cmdr's mount or the OS's) shows in the list.
5. One NAS reached by two addresses stays two servers. Samba derives its server GUID from the NetBIOS name, so a GUID
   match does NOT prove one machine; the person decides which address to use (§ "Decisions", 5).
6. Every protocol the hub lists today (SMB, SFTP, WebDAV, S3, NFS; saved and discovered) maps onto the standard views'
   columns with ❌ no `if server` in a view.
7. #324 still holds, now literally: listing servers never dials anything.
8. ⌘R refreshes a share list: it already re-reads a listed one, a refused one retries, and what an unchanged refresh
   shows is § "Questions", 1 (§ "Current state", the ⌘R finding; M2).
9. getcmdr/cmdr#1518: a Samba server GUID (derived from the NetBIOS name) never proves two shares are one (M2).

This is a just-in-time plan per `docs/guides/multi-agent-refactors.md` § "The loop". It captures intent: when the code
disagrees with a step here, follow the intent and say so in the report.

## Loud rules (every milestone)

- **Characterize, then convert.** No milestone deletes behavior that isn't pinned by a test that survives the deletion
  (a Rust test, a pure-module test, or an MCP-level E2E), and the pin lands BEFORE the rewrite.
- **The views stay generic.** `FullList`, `BriefList`, `FullListHeader`, `SelectionInfo`, `FileIcon`, and the listing
  engine get ❌ no `if server` / `kind === 'network'`. Server facts reach them the way share facts do: a per-entry meta
  read by the ONE cell rule (`extCellText`), the date cell, a reserved icon id (the `git:*` / `restic:snapshots` /
  `network:share` precedent), kind words through `ListWords` (`views/list-words.ts`, filled by
  `pane/list-kind-words.ts`), and the one trailing-row prop (branch 13). A need that fits none of those is a
  stop-and-report, ❌ not a new ad-hoc check.
- **❌ Never classify by a volume-id or path string** when a kind exists: `capabilitiesForPane(volumeId, path)` answers
  "is this the Servers list" (`network` + `smb://`) and "is this a share list" (`network` + `smb://<server>`).
- **#324, literal: building the Servers list never dials.** Its rows come from memory, the saved-server stores, the
  kernel's mount table, and discovery's browse (multicast, no session to any server). ❌ No share listing, no resolve
  that connects, ❌ no Keychain read. A server's shares are read only when the person opens its share list.
- **Row identity travels as the row's own id, end to end** (`network/DETAILS.md` § "Context menu and F8", QA
  2026-09-25): every action on a Servers row resolves the saved entry by the id the row carries, ❌ never by host, name,
  or address at action time.
- **❌ Never merge two servers by GUID.** Samba's GUID comes from the NetBIOS name. Server identity is address + port
  (`server_identity::same_server*`), as today.
- **FilePane.svelte and DualPaneExplorer.svelte are at their size cap**: new pane state is a `*.svelte.ts` factory, pure
  logic a `*.ts` helper (`apps/desktop/src/lib/file-explorer/pane/CLAUDE.md`).
- **Smart backend, thin frontend**: the merge that decides the rows (today `servers-hub-rows.ts`) moves to Rust, beside
  the volume that lists them. Words stay on the frontend: Rust ships facts (`ServerMeta`), the FE words them.
- **Docs move with the code**, every milestone: `network/` (FE and Rust), `servers/`, `pane/`, `views/`, `navigation/`,
  and `cmdr-fs` C+D.md files that describe a touched behavior. No milestone tags in code, comments, or docs.
- **Strings**: new English keys with good `@key` descriptions, ❌ never translated by the executor. Reuse existing keys
  where the words don't change (`servers.hub.status.*`, `servers.hub.neverUsed`, `servers.hub.addServer`, the account
  item labels).
- **`main` stays whole after every milestone**: no milestone removes a door whose replacement lands later. The saved
  share facts reach the share list (M6) BEFORE the switch deletes the hub's share rows (M8). ❗ "Whole" includes #324: a
  milestone that adds saved servers (M4) must not hand them to the hub's prefetch, which dials every saved SMB host
  until M8 stops or narrows it.
- Foreground commands only. Checks per `AGENTS.md` § "Checker script"; `desktop-e2e-playwright` (SMB and servers specs)
  from M8 on, Linux E2E on M8 and M9.

## Current state (fresh read, 2026-11-26, `aa3fa4bbf2`)

- **Just landed on the share list** (keep, don't regress): `ListWords.rowsHaveNoSizes` (the share list drops Size,
  `85372d4d7c`; the Servers list REUSES it, ❌ no second field); share rows carry the `network:share` icon id, which
  `FileIcon` draws as stacked platform folders through `#lib/stacked-icons.ts` (`c966f6413b`); the hub's SMB server rows
  draw the `monitor-filled` glyph (`MonitorFilledIcon`, `--color-server-icon`) chosen by
  `servers-hub-rows.ts::hubRowIcon`, other servers `server`, place rows `folder`; Cancel on a share list's sign-in sheet
  stays on the in-pane sign-in state (`ErrorPane`, focus on Sign in, `9b31e957a1`), and ⌘[ from it lands on the server's
  Servers row; a Servers visit no longer re-dials an OS mount whose direct upgrade failed (`c96149081f`).
- **The ⌘R finding (share list)**: by code AND the worktree's dev log (2026-11-26 10:03:36, `smb://naspolya`), ⌘R on a
  share list that lists DOES re-read: `pane-commands.ts::refreshPane` (caps kind `share-list` isn't the network view) →
  `refresh_listing(force: true)`, which skips the `EveryWriter` short-circuit → `Listings::refresh` →
  `SmbServerVolume::list_directory_for_refresh` → `Reader::Refresh` → `Freshness::Fresh` (cache dropped) → a fresh dial
  (guest refused, smbutil's Keychain fallback, the same 8 shares, about 76 ms). Same rows, so an empty diff, so nothing
  on screen: on Servers the hub's own ⌘R visibly re-scans, which is why the share list reads as dead. ❗ A real hole by
  reading (verify in M2): on the share list's sign-in or refusal state (`ErrorPane`) no listing is live, so
  `refreshPane` either has no listing id or hits `RefreshOutcome::ListingGone`, and ⌘R does nothing at all.
- **The GUID evidence for #1518**: the same log shows Naspolya (QNAP, Samba) negotiating `server_guid` data1
  `0x7073616e`, data2 `0x6c6f`, data3 `0x6179`, data4 zeros: little-endian, that's the ASCII `naspolya`, NUL-padded. So
  a Samba GUID is the lowercased NetBIOS name, recognizable by shape.

- **The pane**: `FilePane.svelte` routes `caps.kind === 'network'` (the `network` volume at `smb://`) to
  `paneViewKind === 'network'`, which renders `pane/NetworkMountView.svelte` (158 lines, now a thin wrapper): it holds
  `holdDiscoveryForServersView()`, consumes the hub landing (`pane/hub-landing.svelte.ts`, written by `writeTabPath`
  when a tab leaves a share list for `smb://`, and by `initialization.ts` on restore), turns a one-place row's Enter
  into `onVolumeChange` (`pathForPickedVolume`), runs the Add server sheet (`openAddServerSheet`: SMB hand-off → share
  list, `onConnected` → the place, `onAdded` → select the new row), and delegates `BrowserAPI` calls (`handleKeyDown`,
  `setCursorIndex`, `getItemCount`, `jumpToPrefix`, `findItemIndex`, `getNetworkCursorEntry`, `openCursorItem`,
  `openContextMenuAtCursor`, `refreshNetworkHosts`).
- **`network/ServersHub.svelte`** (1,031): the table (Name with "as <account>", Type, Address, Status, Last used), the
  nearby group header row, unsaved-share items, "More shares on…" rows, the searching line, the discovery-off line with
  its Settings link, the "+ Add server…" pseudo-row (italic, keyboard reachable), the NFS hint line
  (`servers.hub.nfsHint`
  - `nfsHintLink`), the empty state with Refresh, and `ServersHubStatusBar` (server count + refresh). Prefix
    type-to-jump through `getInMemoryJump`; arrows through `list-cursor.ts`; ⌘R through `handleRefreshClick`.
- **Its pure siblings**: `servers-hub-rows.ts` (534: the merge of saved servers × discovered hosts × volume list, dedup,
  statuses, order, `openMoveFor`, `savedHostFor`, `isServerOfShareList`), `servers-hub-items.ts` (nearby group, two
  index spaces), `servers-hub-keys.ts`, `servers-hub-shares.ts` (#1404 share items, cap 8), `servers-hub-list.svelte.ts`
  (the live list state, `prefetchedShareNames`), `servers-hub-columns.ts` (width breakpoints), `servers-hub-mcp.ts`
  (`protocol=` / `status=` / `address=` / `shares=` tokens, `smb://nearby`, `smb://add`), `servers-hub-actions.ts` (475:
  F8 per row kind, row menus, `runRowEntry`, `editHubRow`, `disconnectSmbHost`), `ServersHubRowMenu.svelte` (house
  `Menu` host), `host-status.ts` (an SMB host's status and tooltip from the prefetch's share state).
- **Row kinds today**: saved SMB host (`volumeId: null`, status `saved` / `found_nearby` / `not_connected_yet`), its
  saved share rows (`kind: 'place'`, `share:<volume id>`, status `connected` / `saved`, account from the live mount),
  its unsaved share items and More row (from the prefetched share list), one-place servers (SFTP, WebDAV, NFS place:
  status from the volume's `connectionState`), an S3 account row (no place; Enter toasts) with its place rows (root "All
  buckets" first, then buckets), and found-only SMB hosts in the nearby group.
- **Inputs**: `listSavedServers()` (Rust `commands/servers.rs::list_saved_servers` = `manual_servers::all` +
  `known_shares::get_all_known_shares` + the other protocols' stores, matched against `fresh_discovered_hosts`), the FE
  discovery store (`network-store.svelte.ts`), the volume list (`getVolumes()`), and `getListedAccount`. ❗
  `known_shares` holds only shares Cmdr itself mounted (`src-tauri/src/network/CLAUDE.md`: "Only Cmdr's own mounts …
  save an SMB share"), so a server with only Finder or `mount_smbfs` mounts has no row: that's David's two addresses.
- **The share-list volume already knows mounted servers**: `share_list_volume.rs::app_known_hosts` builds
  `KnownHosts { fresh, saved, mounted, cached }`, `mounted` from `smb_share_parent::mounted_hosts(smb_mounts())` (the
  kernel's mount table, no network). `SmbServerVolume::list(smb://)` answers `NotADirectory` today; `metadata(smb://)`
  answers the Servers name.
- **Prefetch** (`network-store.svelte.ts`): while a Servers view is up, every SAVED host (`savedSmbHostIds`) has its
  shares listed (`prefetchSharesCmd`, a dial), feeding the nested share items, `shares=`, and `host-status.ts`'s
  tooltip; `refreshAllStaleShares` on entering the view; ⌘R drops every list and re-reads the saved ones. The worktree's
  dev log (2026-11-26) shows it re-dialing `raspi`, `raspi.local`, and `192.168.1.111` on each Servers visit. First
  numbers for § "Decisions", 7 (`stall_probe::listing listing_done path=smb://<server>/`, `read_dir_ms`, cache hits of 0
  ms excluded): `raspi` 64–131 ms in 11 of 13 uncached reads, outliers 643 and 1,174 ms; `naspolya` 80–450 ms (guest
  refused, then smbutil's Keychain read). M1 measures properly.
- **Keychain on a share-list read**: the share list's own ladder reads Cmdr's stored password at most once per session
  (`share_list_sign_in.rs`, `stored_once`), but a guest-refused listing with no credentials falls back to `smbutil view`
  with macOS Keychain auth (`smb_client.rs`, `list_shares_cli_with_keychain`) on EVERY such dial, ⌘R included. Not new
  and not the Servers list's, but anything that keeps a prefetch inherits it.
- **Share list precedents to reuse** (#1511): `ShareMeta` on `cmdr_fs::FileEntry` worded by `extCellText`; `ListWords`
  (`extHeader`, `extIsProse`, `rowsHaveNoDates`, `parentRowName`, `emptyText`, `countRows`); the `share-list` row in
  `CAPABILITY_TABLE` and its refusal arms (`file-operation-commands.ts::readOnlyRefusal`, `transfer-entry.ts` guards);
  the share-row menu hosted by `ShareListAccountChip` in `VolumeBreadcrumb`'s ONE `openMenu`; `pane-footer.ts` giving a
  kind `SelectionInfo` without disk space; the `comment="…"` MCP name token; `share-list-pane.svelte.ts` as the per-kind
  open seam (Enter on a share mounts).
- **Server identity in the backend**: `server_identity::same_server*` (address + port). Separately,
  `location/share_identity.rs::same_share` treats two shares as one when their recorded server GUIDs and share names
  match (Ask Cmdr's LAN/Tailscale pairing); `redirectable` lets one answer a read for the other when both GUIDs came
  from signed-in sessions. Its callers are `location/same_share.rs::same_share_as` (volume snapshot, important folders,
  "volume not connected", `finish`). ❗ On Samba that proof only proves one NetBIOS name (#1518, the evidence above): in
  scope, M2. `network/server_guid.rs`'s module doc and `network/DETAILS.md` § "Which server a share is on" claim the
  GUID names one box, and that each Docker fixture's Samba "mints its own" GUID (it differs only because the hostnames
  do).
- **Address change already exists**: `manual_servers_relocate.rs` (`server_move::smb`) moves a saved SMB host to a new
  address in one write; the Edit server sheet drives it.
- **Commands**: `servers.connect` ("Connect to server…", ⌘K, also the dock) opens the Add sheet from anywhere;
  `servers.edit`, `servers.forgetSecret`, `servers.signInAs`, `servers.browseAsGuest`, `servers.disconnect`,
  `servers.togglePin` resolve through `servers/server-command-target.ts` (hub cursor row, else the share list's server,
  else the pane's volume).

## Fit check

**Verdict: it fits, with the same shape the share list used: `SmbServerVolume` lists `smb://` too, and the `network`
kind becomes a listing kind. The hub's merge moves to Rust as a pure module; the views need one new `ListWords` field
(`dateHeader`; `rowsHaveNoSizes` already exists), one entry-meta badge, two reserved icon ids, and one generic
trailing-row prop, no kind checks. The trailing row is the one genuinely new piece of view machinery, and the biggest
risk in the plan (branch 13).** The Servers list is a flat, read-only, memory-backed directory whose rows change when
discovery, the stores, or the volume list change. Unlike the share list, it needs live updates.

**The shape.**

- **`servers_listing.rs`** (app side, `src-tauri/src/network/`, beside `share_list_volume.rs`): a pure
  `server_rows(inputs) -> Vec<ServerRow>` ported from `servers-hub-rows.ts` (merge, dedup by id then name/hostname,
  unique ids, `primaryHost`, `displayName`'s name rank, statuses), minus the place rows (flat), plus mounted servers (§
  "Decisions", 4), plus "connected" for an SMB server any of whose shares is mounted. Inputs: the saved listing
  `list_saved_servers` already builds, `fresh_discovered_hosts`, the volume list's connection states, the mount table,
  and `SmbServerVolume::listed_as`. ❌ No dial anywhere in it; a test injects every input.
- **`ServerMeta` on `cmdr_fs::FileEntry`** (fact only, no words; `share_meta`'s sibling): `row_id`, `protocol`,
  `address`, `status` (the `HubRowStatus` enum), `account` (`SignedInAs` as data), `saved: bool`, `place` (the volume id
  a one-place row opens, `None` for an SMB host or an S3 account), `pinned`. `modified_at` carries `lastConnectedAt`, so
  the date column IS "Last used" with no new column code.
- **`SmbServerVolume::list(smb://)`** answers the server rows for EVERY reader (pane, refresh, MCP `list_dir`): they're
  memory-only, so there's nothing to gate. Entry path: an SMB server's share-list path (`ShareListAddress::of_host`, the
  writer the share list already uses), so Enter on it is a plain same-volume folder navigation; a one-place server's
  place `appRoot`; an S3 account per § "Decisions", 6. ❗ Entries whose path isn't under the listed directory are new
  for the engine: prove it with an engine test first (diffs by path, `findFileIndex`, the drop-foreign-listings rule);
  if it refuses them, fall back to an in-volume form for non-SMB rows and say so, ❌ not a special case in
  `cmdr-listing`. ❗ Every row is `is_directory: true`: a file entry's Enter is the OS opener, which would hand
  `sftp://…` to Launch Services. ❗ A row whose path is on another scheme meets every generic verb that takes a
  directory entry's path as a path ON THE PANE'S VOLUME (`network`): Enter / ⌘↓, open in new tab / other pane, history,
  Go to path, MCP `nav_to_path` / `open_under_cursor`, Copy path, drag-out. Each either takes the open arm (branch 5) or
  refuses with kind words; M6 pins every one.
- **Icons**: Rust sets a reserved icon id per row (`network:server-smb`, drawn as `MonitorFilledIcon`; `network:server`,
  the generic server glyph), resolved in `FileIcon` the way `git:*` and `restic:snapshots` already are, and kept out of
  `prefetchIcons`'s OS asks (the `network:share` precedent). `hubRowIcon`'s `folder` for place rows goes with the place
  rows.
- **Live updates**: the volume answers `listing_watch_coverage(smb://) = EveryWriter` and, on each input change
  (`network-host-found` / `-lost` / `-resolved`, `volumes-changed`, a saved-store write, a mount-table change), pushes a
  re-list of `smb://` through `Listings::directory_changed` (the path every other watched backend uses) with the
  "re-listed, these are its contents now" `DirectoryChange` variant (`cmdr-fs` `volume/types.rs`), debounced. ❌ No
  poll. The cursor follows its row by path across a rebuild, which today's `cursorAcrossRebuild` did by id. ❗ Because
  the volume answers `EveryWriter` for `smb://`, an unforced `refresh_listing` (every delete or trash tops up both
  panes) short-circuits; ⌘R (forced) re-lists from memory and the stores. Both are dial-free on `smb://`, so the
  forced/unforced split only matters one level down, on `smb://<server>` (M2).
- **The mount table is read without touching a mount**: `volumes::smb_mounts()` (`getfsstat`, no per-mount `statfs` or
  `stat`, which hang on a dead share), and `smb_share_parent::made_host` resolves an mDNS service name from fresh
  discovery only (`smb_server_address.rs`). Keep it that way: a dead NAS must not stall the Servers list.
- **The `network` kind becomes a listing**: `CAPABILITY_TABLE.network` gets `hasBackendListing: true`,
  `syncsToMcp: true`, `hasParentRow: false` (it's the volume root), everything write- and source-shaped `false`, like
  `share-list`. `paneViewKind` stops routing it to `NetworkMountView`.

**Every Servers-specific branch, named** (all at existing per-kind seams):

1. `volume-capabilities.ts`: the `network` row's cells. The ONE path read stays `isShareListLocation`.
2. The cells: `extCellText` gains a `serverMeta` arm (the "Details" text: protocol, address, account, per § "Decisions",
   2), worded by a pure `network/server-row-words.ts` it calls (restic and share precedent); the date cell reads
   `modified_at` as today (blank when never used); the existing `ListWords.rowsHaveNoSizes` drops the Size track (the
   share list's, `85372d4d7c`); a new `ListWords.dateHeader` labels the date track "Last used" (header, sort trigger,
   the row's accessible name, and the measurer all read it). Renderer and measurer stay one contract
   (`views/CLAUDE.md`). ❗ Sorting by the date column must sort by `modified_at` (Last used), and a never-used row (no
   date) sorts last ascending; check `EffectiveSort::resolve` doesn't remap it for a volume with no dates.
3. The status dot: `FileIcon` already takes per-row badges (`syncStatus`, `imageIndexBadge`) chosen outside it. A
   `rowBadgeFor(entry)` in `views/file-list-utils.ts` returns the server status badge (the `ConnectionDot` colors) from
   `serverMeta.status`, with its words as the badge's accessible name, so screen readers hear "Connected". ❌ No status
   branch inside `FileIcon`.
4. The kind's words (`list-kind-words.ts`): `extHeader` "Details" (prose), `dateHeader`, `rowsHaveNoSizes`, `countRows`
   ("5 servers", plus the discovery state, § "Decisions", 8), `emptyText`.
5. The open path (Enter, double-click, `openCursorItem`, MCP `open_under_cursor`): one `network` arm beside the
   `share-list` arm in `share-list-pane.svelte.ts`'s seam (or a `servers-pane.svelte.ts` sibling factory): a row with
   `serverMeta.place` lands the pane on that place (`onVolumeChange` with `pathForPickedVolume`, so `place-connect`
   dials where every wait renders, today's `handleServerSelect`); an S3 account per § "Decisions", 6; an SMB server
   needs no arm (it's a folder on the same volume). Other verbs that take a row somewhere: open in new tab / other pane
   take the same move (an SMB server's share list, a place); drag-out, Space's size walk, Quick Look, About (⌘I), and
   MCP `about_item` / `inspect_file` refuse with kind words. ❗ A drop ONTO a one-place server row would target its
   `appRoot` on another volume, which would dial and copy: Servers rows are no drop targets (the row-drop target is off
   for the kind), and the destination guard refuses the `network` kind. Pin both. ❗ The open arm must run BEFORE the
   generic directory open for every verb in § "Fit check"'s path list, or a place row navigates the `network` volume to
   `sftp://…`.
6. Keys and the F-key bar: F2 (`file.rename`, already Edit server on `network`), F8 (`readOnlyRefusal`'s `network` arm
   becomes "Forget server", today's hub F8), F7 (`file.newFolder` on `network` → Add server, § "Decisions", 1), ⌘R (a
   forced re-list: re-reads the stores, no dial; `refreshPane`'s `isInNetworkView` branch goes). The F-key bar's labels
   per kind (F7 "Add server", F8 "Forget") come from a per-kind table beside `list-kind-words.ts`; `FunctionKeyBar`
   already checks `caps.kind === 'network'` for `canRename`, which folds into that table, ❌ no second kind check.
7. The row menu: `file.contextMenu` (right-click, ⌃⏎) on a `network` row opens the server row menu from `row-menu.ts`
   (`smbHostRowMenu`, `volumeRowMenu`), hosted in `VolumeBreadcrumb`'s ONE `openMenu` the way the share-row menu is
   (`isHeaderMenuOpen()` stays the one key-suppression answer). The row comes from `serverMeta.row_id`; Edit reads the
   `SavedServer` by that id.
8. The palette target: `server-command-target.ts`'s `cursorRow` comes from the listing's cursor entry (`serverMeta`)
   instead of `getNetworkCursorEntry`; the rule (cursor row, else share list's server, else pane volume) is unchanged.
9. The landing: `..` from `smb://<server>` becomes the generic parent step (the volume root now lists), which selects
   the row it came from BY PATH (the entry's name is the friendly name, not the key). ❗ `parentOf('smb://x')` is
   `smb:/` in the TS grammar (#1511's landmine): the step must land on `smb://`. A restored share-list tab keeps landing
   on Servers (nothing lists a server at launch); its carrier (`landOnServer`) now feeds the same select-by-path.
   `hub-landing.svelte.ts` retires.
10. The MCP mirror: the standard one, with each server's facts as locale-independent name tokens (`protocol=`,
    `status=`, `address=`, `account=`), the share list's `comment="…"` precedent. `smb://nearby`, `smb://add`, and
    `shares=` go (§ "Coverage contract", MCP).
11. The footer: `pane-footer.ts` gives the `network` kind `SelectionInfo` without disk space, like `share-list`.
12. The discovery hold: `holdDiscoveryForServersView` follows the pane's LOCATION (a pane on `network` at `smb://`) from
    a pane factory, ❌ not a component mount.
13. The trailing "+ Add server…" row (§ "Decisions", 1b). A new generic, optional `trailingRow` prop on both views
    (label, icon, command id), filled per kind beside `list-kind-words.ts` (only `network` sets one). It's the mirror
    image of the synthetic `..` row: ❌ not an engine entry, so sort, quick filter, and type-to-jump never touch it by
    construction. Its frontend index is `parentOffset + rowCount`; the cursor reaches it (↓, End, PageDown, click), and
    Enter, double-click, and MCP `open_under_cursor` dispatch its command (`servers.connect`, the same sheet F7 opens).
    It's never selectable (Space, Insert, ⇧-ranges, ⌘A, select-same-kind), never counted (footer, `countRows`, stats),
    never an operation's source, target, or drop target, never draggable. It stays visible under a quick filter (it's an
    action, not a match), so a filter that matches nothing leaves "+ Add server…" as the obvious next step. The MCP
    mirror lists it as the last item at its screen index with `kind=action command=servers.connect`. a11y: an option in
    the listbox whose name is its label. ❗ This is real cursor machinery: every consumer of the effective row count or
    the parent offset (`move-cursor.ts`'s range, page and End keys, cursor restore across a rebuild, `findFileIndex` +
    offset in `pane-commands.ts`, selection ranges, MCP indices, virtual-scroll height, Brief's column flow, drag hit
    tests) must learn ONE `trailingRowCount`, from a pure helper, ❌ never per-site arithmetic. `FilePane` is at its
    cap: a `*.svelte.ts` factory.

Thirteen named branches, each at a seam that already branches per kind (or, for 13, one new generic prop), with one new
`ListWords` field, two reserved icon ids, and one generic badge hook. Zero in the engine and the views' row code.

**Considered and rejected**: keeping the merge in TS and pushing rows to Rust (two sources of truth and a startup race);
a frontend in-memory listing (the snapshot pane's six coupling points, and Rust owns every input anyway); real extra
columns (Type, Address, Status) in the views (column machinery, measurer, and fit rules for one pane kind; §
"Decisions", 2 keeps it as an option); keeping the nearby group as a pseudo-row (a row the engine doesn't list breaks
sort, filter, indices, and MCP); "+ Add server…" as a Rust-listed entry (sort and filter would move or hide it, and
pinning it last would need a special case in the engine's ONE comparator and filter point, so it's a view-side trailing
row, branch 13, which sits outside both by construction).

## Line estimate

**Production code about −1,150 (range −650 to −1,650); tests about −1,400; total about −2,550 (−1,600 to −3,500).**
(Revised up from −1,400 for the trailing row, ~+150 prod and ~+150 tests, and M2's ⌘R and #1518 fixes, ~+100 prod and
~+100 tests.)

- **Removed (~−2,950 prod)**: `ServersHub.svelte` 1,031, `NetworkMountView.svelte` 158, `servers-hub-rows.ts` 534
  (ported), `servers-hub-items.ts` 115, `-keys` 28, `-shares` 84, `-columns` 16, `-mcp` 174,
  `servers-hub-list.svelte.ts` 139, `ServersHubStatusBar.svelte` 88, `list-cursor.ts` 56, `hub-landing.svelte.ts` 50,
  the prefetch and stale refresh in `network-store.svelte.ts` (~120, § "Decisions", 7), `host-status.ts`'s
  prefetch-error status (~100), the hub-only half of `servers-hub-actions.ts` (~250; F8 and the row-menu runner move),
  the nearby-group setting plumbing (~40), CSS.
- **Added (~+1,550 prod)**: `servers_listing.rs` (~450, ported merge) + mounted servers (~120) + `ServerMeta` (~50) +
  the volume's `smb://` listing and live re-list (~150); FE: kind cells, words, badge, MCP tokens (~200), the open arm,
  row menu host, keys, refusals, palette target (~300), the Add server door (~40), saved-share facts on the share list
  (~150), discovery hold factory (~40), the moved F8/menu runner (~50 net).
- **Tests**: `ServersHub.test.ts` 1,028, `servers-hub-rows*.test.ts` 810 (ported to Rust, ~600), items/keys/shares/
  mcp/columns/list-cursor tests ~575, `NetworkMountView*.test.ts` 545, `hub-landing` 46, most of `network.a11y.test.ts`
  300 retire; new Rust tests (~800), kind/cell/menu tests (~300), E2E rewrites (flat).

The honest caveat: the Rust port is real work with a long test tail (the merge has a lot of regression anchors), and the
live re-list is new. The win is deleting a parallel list implementation and every bespoke key, cursor, and MCP path.

## Coverage contract (every current Servers flow → where it lives after)

A milestone that touches a flow keeps its pin green; M9 walks this list.

**Opening things**

- Enter / double-click on an SMB server → its share list. AFTER: generic folder open (the row's path IS `smb://<key>`).
  A saved host mDNS isn't seeing: Rust's resolver already falls back to the manual server (unchanged).
- Enter on a one-place server (SFTP, WebDAV, NFS, an S3 place) → the pane lands on the place, which dials in the pane.
  AFTER: the `network` open arm (branch 5), same `pathForPickedVolume`.
- Enter on an S3 account → toast today. AFTER: § "Decisions", 6.
- Enter on a saved share row, an unsaved share item, or "More shares on…" → mount / share list. AFTER: gone from the
  Servers list; the share list carries every share, and a saved one keeps its facts and its account (M6, § "Decisions",
  10). A saved share stays reachable from the switcher when pinned.
- Enter / click on the nearby group header → toggle. AFTER: no group (§ "Decisions", 3); `network.nearbyServersGroup`
  retires (a hidden setting: drop it quietly on load).
- Add server: the pseudo-row (Enter, click, double-click) and the empty state → the sheet; SMB hand-off → share list
  with a queued mount; `onConnected` → the place; `onAdded` → select the new row. AFTER: both doors of § "Decisions", 1:
  the trailing "+ Add server…" row (branch 13: Enter, click, double-click, MCP `open_under_cursor`) and F7 / the palette
  / ⌘K, all running the same `openAddServerSheet` callbacks from a pane factory; the empty list's words name F7, with
  the trailing row below them. "Select the new row" is the generic select by path once the re-list lands (the sheet
  closes before the re-list, so the select WAITS for the path, as `pendingSelection` does today).
- NFS hint and its link → gone (goal 3). The Add sheet's protocol picker still offers NFS.
- Discovery off line + Settings link, searching line → § "Decisions", 8.

**The list itself**

- Columns Name / Type / Address / Status / Last used, with width breakpoints → Full's Name / Details / Last used (§
  "Decisions", 2), `full-list-column-fit.ts` dropping Details then the date on a narrow pane. Brief: names, with the
  Details text as the row tooltip (share list precedent).
- "as <account>" after the name → in Details (`signed-in-as.ts` stays the one source of the words).
- Status words, colors (`needs-you`, `is-live`), and the SMB host tooltip → the status dot badge plus its words in
  Details and the accessible name; the prefetch-error tooltip goes with the prefetch (§ "Decisions", 7).
- Order (live, asking, saved by recency, nearby last) → the pane's sort (§ "Decisions", 9).
- Prefix type-to-jump → the folders' fuzzy type-to-jump and quick filter.
- Arrows, PageUp/PageDown, Home/End, Left/Right jumps → the file list's own keys.
- The status bar (count, refresh button) → `SelectionInfo` "N servers" (branch 11) and ⌘R.
- The empty state with Refresh → `EmptyFolderState` words (§ "Decisions", 1 and 8), with the trailing row under them.
- The "+ Add server…" pseudo-row (italic, keyboard reachable, last) → the trailing row (branch 13): last under any sort
  and any filter, skipped by type-to-jump, never selected or counted. Pin each of those in M7.
- Rows landing late keep the cursor on its row (`cursorAcrossRebuild`) → the engine's diff keeps the cursor by path; pin
  it with a host arriving above the cursor.
- Row id uniqueness (Svelte `each_key_duplicate` crash) → the Rust merge keeps "first writer wins" and the engine needs
  unique PATHS: two rows must never share a path (pin: a server in two stores yields one row).

**Acting on a row**

- F8 → forget per row kind (SMB host with password box, one-place via `forgetSavedServer`, S3 account with its secret,
  discovered → "Can't remove discovered hosts" toast). AFTER: same functions, reached through `readOnlyRefusal`'s
  `network` arm, keyed by `serverMeta.row_id`.
- F2 / ⌘E / palette Edit server → `editHubRow`. AFTER: same, with the row from the cursor entry.
- Right-click / ⌃⏎ → house `Menu` (`smbHostRowMenu`: Open, Edit…, Disconnect while mounted via `smb_host_has_mounts`,
  Sign in as…, Browse as guest, Forget saved password, Forget server; one-place rows: `volumeRowMenu` with pin,
  auto-reconnect, Connect directly fix). AFTER: the same builders, hosted per branch 7. ❗ Opening the menu still reads
  no Keychain.
- Account actions that list open the share list (`deps.openRow`) → AFTER: navigate the pane to the row's path.
- Palette server commands on the cursor row → branch 8.
- Disconnect (SMB host: unmount its shares) → unchanged.

**Coming back**

- `..` / `⌘↑` / Backspace on a share list (or its error screen) → Servers, cursor on that server (`hub-landing`). AFTER:
  generic parent step with select-by-path (branch 9). Pin: the existing E2E (by IP-added server too, `1a1d2598c9`).
- Restored share-list tab → Servers with the cursor on the server, nothing listed. AFTER: same carrier, generic landing.
- History Back to Servers → standard history (the generic cursor restore can now apply to `network` too:
  `commitHistoryWalk` skips it today).
- `select_volume` Servers (switcher, MCP) → always `smb://`. Unchanged; pin stays.

**#324 and the Keychain**

- Nothing lists at launch; found hosts never listed unasked. AFTER: stronger: building the Servers list never dials, for
  any server (§ "Decisions", 7, as M1's measurement settles it). Pin: a Rust test that `list(smb://)` with a counting
  `ServerShares` makes zero dials and zero Keychain reads for every reader (pane, refresh, other) and on a live re-list;
  an E2E that opening Servers with a saved SMB fixture issues no share listing (the backend's listing counter). If M1's
  numbers keep a prefetch for saved servers, the E2E instead pins "saved only, never a found or a mounted-only server".
- Servers M4 saves from the mount table are never dialed by the hub's prefetch while it still runs (M4 to M8). Pin in
  M4.
- Opening a row menu reads no password → unchanged pin.
- ⌘R on a share list (forced) dials past the share cache, and its Cmdr-store read stays at most once per session
  (`stored_once`); an unforced refresh (delete, trash, rename top-up) never dials (`share_list_volume.rs`'s
  `EveryWriter`). Pin both in M2.
- Share identity (#1518): a name-derived Samba GUID never makes two shares `same_share` / `redirectable`. Pin in M2.

**MCP**

- `cmdr://state` on Servers: today the hub's own push (`hubPaneState`): one entry per item incl. the group header and
  the add row, `protocol=` / `status=` / `address=` / `shares=` tokens, indices over the FULL list. AFTER: the standard
  mirror (`syncsToMcp: true`), tokens per branch 10, indices match the screen (no hidden rows), the trailing row last
  with `kind=action command=servers.connect` (branch 13). `move_cursor` by name and `open_under_cursor` work as on any
  list, the trailing row included. Specs that poll `shares=`, `kind=group`, or `smb://add` (`smb.spec.ts`,
  `servers.spec.ts`, `favorites-offline-smb.spec.ts`, `suggested-ops-server-card.spec.ts`,
  `i18n-capture-main-overlays.ts`) move to the new spelling in M8.
- MCP `refresh` on a share list → the same forced re-read as ⌘R (M2).
- MCP `list_dir` on `smb://` → today `NotADirectory`; AFTER: the server rows (memory-only, safe).

**Elsewhere, unchanged but must keep working**

- The share list, its sign-in ladder, the account chip, the share-row menu, share mounting, #1238's `..` out of a
  share's top folder, its stacked-folders share icon, its dropped Size column, and Cancel on its sign-in sheet staying
  on the in-pane sign-in state (⌘[ from there lands on the server's Servers row, now by the generic landing).
- Two panes on Servers at once (each lists; one shared live re-list).
- Direct-connect upgrade, the reconnect manager, the switcher's Servers row and dots.
- Linux and Windows: the mount-table source is per platform (`volumes::smb_mounts`); `clippy-windows`.
- a11y: the standard listbox; axe on a Servers pane; the dot's words reach the row's accessible name.

## Decisions (David)

**Decided (David, 2026-11-26):** 1 (a) AND (b): F7 / palette / empty-list words, plus a last "+ Add server…" row (the
views get one generic trailing action row through a per-kind seam, never an `if server`; it stays last under any sort,
and filter/type-to-jump skip it). 2 (a). 3 (a). 4 (a). 5 (b). 6 (a). 7 (a), UNLESS reading a server's shares is slow
enough to feel: measure a typical share-list read (fixtures plus the timings the dev log records for real servers); if a
saved server's list takes long enough that opening it visibly waits, keep a prefetch for SAVED servers only, under the
existing #324 gates, and record the numbers. 8 (a). 9 accept. 10 (a). Also in scope: ⌘R must refresh a share list (David
saw nothing happen; Servers already refreshes), and getcmdr/cmdr#1518 (share identity must not treat a Samba server GUID
as proof of one machine; it's derived from the NetBIOS name). The stacked-folders share icon and the filled monitor
server icon are approved. Where each answer lands: 1 → branch 13, M7, M8; 7 → M1 measures, M8 acts; ⌘R and #1518 → M2,
with two follow-up questions in § "Questions".

1. **Where "+ Add server…" lives.** (a) F7 on the Servers list (the F-key bar reads "F7 Add server", since F7 means "new
   item here" everywhere), plus ⌘K from anywhere (exists), plus the empty list saying "Press F7 to add a server"; (b)
   keep a last pseudo-row (sort, filter, and type-to-jump would move or hide it, so the views need a special row); (c) a
   button in the footer (a generic action slot in `SelectionInfo`); (d) the palette and ⌘K only. **Recommended: (a).**
2. **Columns.** (a) Name, then one "Details" column ("SMB · 192.168.1.111 · as testuser", in the slot the share list
   uses for descriptions), then "Last used"; the status as a colored dot on the row's icon, its words in the tooltip,
   the Details text, and what a screen reader hears; no Size column. (b) Real Type, Address, and Status columns, like
   today (new column machinery in both views for one pane kind). (c) Type in the Ext slot, Status in the Size slot, Last
   used as the date, address only in the tooltip. **Recommended: (a).**
3. **Servers Cmdr only found nearby** (not saved, nothing mounted): (a) ordinary rows reading "Found nearby", mixed in
   with the rest; (b) treated like hidden files: dimmed, and shown only with "Show hidden files" on (closest to today's
   folded group, but a new user with nothing saved would see an empty list); (c) not listed at all. **Recommended:
   (a).**
4. **Servers behind mounted shares.** (a) Saved the first time Cmdr sees one of their shares mounted (Finder,
   `mount_smbfs`, or Cmdr), so they stay listed after unmounting; Forget removes one, and Forget on a server with a
   share still mounted offers to disconnect it first (else it would come right back). (b) Shown only while a share is
   mounted, never saved. **Recommended: (a)**, since you asked for them as known servers. Only the SERVER is saved, ❌
   not its shares: the rule that only Cmdr's own mounts save a share stays.
5. **One NAS at two addresses** (LAN and Tailscale). (a) Two rows, nothing more. (b) Two rows, and Edit server lets you
   change a server's address (this already exists for saved SMB servers), so you keep the one you use and edit or forget
   the other. (c) One saved server that holds several addresses with a preferred one (a big change: mounts, saved
   shares, and passwords are all keyed per address today, and a GUID can't prove two addresses are one box).
   **Recommended: (b).** Revisit (c) only if two rows keep bothering you.
6. **S3 accounts and their buckets.** (a) One row per account; Enter opens the account's "All buckets" place in the pane
   (saving it if it isn't yet, since Enter is you asking for it), which lists the buckets as folders; saved buckets stay
   reachable through their switcher pins. (b) One row per saved bucket, no account row. (c) The account row opens a list
   of its saved places, like an SMB share list (a second kind of "server's places" listing). **Recommended: (a).**
7. **Background reading of saved servers' shares while Servers is open** (today's prefetch, which fed the nested share
   rows and the error tooltip): (a) stop it: nothing dials while the Servers list is up, and a share list is read when
   you open it; (b) keep it for saved servers, so their share lists open instantly. **Recommended: (a)**, your point 7.
8. **The "still searching" and "discovery is off" lines.** (a) The footer's count says it ("5 servers, looking for more
   nearby…" / "5 servers, nearby search is off"), and the Settings switch is where you turn discovery back on; (b) keep
   a clickable line, which needs a special non-row element in the views. **Recommended: (a).**
9. **Order.** Today: connected first, then ones needing you, then by last use, nearby last. After: the pane's normal
   sort (Name by default; click "Last used" to sort by recency). **Recommended: accept the normal sort.**
10. **Saved shares' new home.** Their rows under each server go away. (a) The share list marks a saved or mounted share
    with the same dot, its row menu gains "Pin to switcher" and "Forget share", and Enter on a saved share mounts it as
    its saved account; (b) the share list stays as it is, and saved shares are managed only from the switcher.
    **Recommended: (a).**

## Milestones

Sequential, one agent each. Each reports in ≤350 words: what landed, what deviated from this plan and why, which pins it
added. M2 is independent of the rest and may land first or in parallel with M1.

### M1: characterize what must survive, and measure the prefetch

- **Scope**: MCP-level E2E pins that outlive `ServersHub`: Enter on an SMB server opens its share list; `..` back lands
  on that server (by name and by IP-added server); Enter on an SFTP and a WebDAV fixture lands the pane on the place; F8
  on a saved server forgets it (and refuses on a found one); Edit server from F2 on a row; Add server via ⌘K then the
  new server is in the list; the row menu's account item lists the share list; `select_volume` Servers lands on
  `smb://`. Pin semantics (which server, which pane location), ❌ not the encoding: `protocol=` / `shares=` /
  `kind=group` spellings change on purpose. Add the backend's share-listing counter the #324 E2E reads (if none exists),
  and record today's count for "open Servers with a saved SMB fixture" as the baseline M8 flips.
- **The prefetch measurement (§ "Decisions", 7)**: time a saved server's FIRST, uncached share-list read, the wait a
  person sees when the prefetch is gone. Sources: the fixtures (`test/smb-servers/`: guest, auth, and a slow-network one
  if the stack has it) via `stall_probe::listing listing_done path=smb://<server>/ … read_dir_ms`, plus the dev and prod
  logs' real servers (exclude `read_dir_ms=0`, a prefetch cache hit). **Threshold: the pane's wait view reveals at
  `WAIT_REVEAL_AFTER_MS` (800 ms, `#lib/ui/wait-reveal.ts`), below which a person sees no wait at all. If the p90 of
  uncached reads for reachable saved servers is under 800 ms, (a) holds: M8 stops the prefetch. At or over 800 ms, M8
  keeps a prefetch for SAVED servers only (never found-only or mounted-only ones), under the existing #324 gates.**
  Unreachable servers don't count (no prefetch makes them faster; they wait in the pane either way). Record the numbers
  (n, p50, p90, max per server) in the report and in `network/DETAILS.md`, dated. First look (2026-11-26 dev log, §
  "Current state"): p90 around 650 ms on `raspi`, under 450 ms on `naspolya`, so (a) is likely.
- **Intentions**: the E2E proves the flows survive the swap; green on today's code first.
- **Landmines**: hub indices count the full list (nearby group); pick rows by name. Don't pin the hub's add row, the
  group, the NFS line, or nested share rows (they're meant to go; the trailing row gets its own pins in M7).
- **Test plan**: `pnpm check desktop-e2e-playwright` with the servers and SMB specs; each new spec red-first by breaking
  the behavior once.
- **DONE**: specs green on `main`'s behavior, mapped to coverage-contract lines in the report; the measurement and the
  (a)/(b) verdict recorded.

### M2: ⌘R on a share list, and #1518 (independent)

- **⌘R scope**: the finding in § "Current state": a listed share list re-reads on ⌘R but shows nothing when nothing
  changed; on its sign-in or refusal state (`ErrorPane`) ⌘R likely does nothing. First confirm the second from code and
  a test (what `refreshPane` gets from `getListingId()` there, and what `refresh_listing` answers). Then: ⌘R on a pane
  whose listing failed retries that navigation (generic, any kind in an error state, ❌ not a share-list check; reuse
  the Try again path if `ErrorPane` has one); and whatever visible sign § "Questions" Q1 settles on.
- **#1518 scope**: a GUID in Samba's name-derived shape (the 16 little-endian bytes are printable ASCII, then NUL
  padding: `naspolya` → `7073616e-6c6f-6179-0000-000000000000`) is recognized in `ServerGuid` (a typed flag or a
  `GuidOrigin`, ❌ never a string compare at the call site) and per § "Questions" Q2 never proves `same_share` or
  `redirectable`. Fix the stale docs: `server_guid.rs`'s module doc ("only the server's own GUID says they're one box"),
  `network/DETAILS.md` § "Which server a share is on" (incl. the "mints its own on first start" fixture line). This plan
  already says servers never merge by GUID; nothing in the Servers list reads it.
- **Pins**: Rust: a forced refresh of `smb://<server>` with `FakeShares` makes ONE `Freshness::Fresh` dial and an
  unforced one makes none (the `EveryWriter` short-circuit); a refused share list's ⌘R re-runs the navigation (FE test);
  `same_share` / `redirectable` false for two rows sharing a name-derived GUID, true for a Windows-shaped random GUID as
  today; each `same_share_as` caller's test gets a name-derived case.
- **Landmines**: don't add a Cmdr-store read to the forced path (`stored_once` stays once per session); the smbutil
  Keychain fallback already runs per dial and stays as is (§ "Current state"); `refresh_listing`'s
  `watcher_backed_listing_path` must keep short-circuiting unforced refreshes on `network`. The `GuidProof` vocabulary
  (Authenticated / Unauthenticated) is orthogonal to the origin: a signed-in Samba session still only proves a name.
- **Test plan**: `cargo nextest run share_list_volume same_share server_guid`; `svelte-tests-related`; `clippy`.
- **DONE**: ⌘R on a refused share list retries; #1518 closed by the commit.

### M3: the server rows in Rust (not yet wired)

- **Scope**: `src-tauri/src/network/servers_listing.rs`: `server_rows(ServerRowInputs) -> Vec<ServerRow>`, a port of
  `servers-hub-rows.ts`'s merge minus place rows, with its tests ported (`servers-hub-rows.test.ts`, `.s3.test.ts`);
  `ServerMeta` in `cmdr-fs` (specta + bindings regen); the row's reserved icon id (§ "Fit check", Icons); an SMB server
  reads `connected` when a share from it is mounted (mount table, `same_server`). S3 accounts per § "Decisions", 6.
- **Intentions**: every regression anchor in the TS merge survives (name rank, `primaryHost`, "Found nearby" only from a
  DISCOVERED match, `not_connected_yet`, unique ids, first writer wins, port in the address off 445, `hubRowIcon`'s
  monitor for SMB and server glyph for the rest).
- **Landmines**: `listed_as` lives in `SmbServerVolume` keyed by host id: read it without locking across an await.
  `matchingHosts` compares lowercase name/address; use `server_identity` where the TS compared strings only if the
  answers match on every ported test, else keep the TS semantics and say so. The S3 root label is a WORD
  (`servers.hub.s3AllBuckets`): Rust marks the root, the FE words it. Row PATHS must be unique, not just ids (the engine
  keys by path).
- **Test plan**: ported unit tests; `pnpm check clippy`; `cargo nextest run servers_listing`.
- **DONE**: Rust rows equal the TS rows on every ported fixture (flat).

### M4: servers behind mounted shares

- **Scope**: § "Decisions", 4 and 5. For (4a): a mount-table pass (the OS-mount pass the upgrade already runs at launch
  and on mount events) saves each mounted share's SERVER into `manual_servers` with a name source that ranks below a
  person's name and marks it as saved from a mount; ❌ no share row in `known_shares`. Forget on a server with a live
  mount offers Disconnect first. For (5b): verify Edit server's address change on an SMB row and that it never merges
  two saved servers (the relocate refusal). Two mounts of one NAS by two addresses yield two rows. The hub (still live
  until M8) shows these servers as ordinary saved rows.
- **Intentions**: David's `192.168.1.111` and `100.127.48.122` both show, as two servers, without anyone dialing them.
- **Landmines**: identity is address + port, ❌ never GUID. ❗ #324 until M8: the hub's prefetch lists every SAVED SMB
  host's shares on each Servers visit (`savedSmbHostIds`), so the servers this milestone saves (a Tailscale address
  included) would start being dialed; they're excluded from the prefetch by their mount-saved mark until M8 decides the
  prefetch (pin it). ❗ Key the saved server by the mount's own server spelling (`statfs`'s, port included), ❌ never by
  `made_host`'s discovery-resolved address: that one flips between an IP and a `.local` name with discovery timing, so
  launches would save duplicates. An mDNS service name in `statfs` (`friendly_server_name`) must not save a second row
  beside the discovered host; the write must be idempotent across launches; `smb_hosts` dedupes by ADDRESS while row ids
  come from the NAME (the duplicate-id crash in `network/DETAILS.md`); a manual entry injects a `manual` host into
  discovery at startup, so the new rows must not read "Found nearby". The pass reads the mount table only
  (`smb_mounts()`), ❌ never `statfs` / `stat` per mount (a dead share hangs it).
- **Test plan**: Rust tests with a fake mount table (LAN + Tailscale of one GUID → two rows; a share mounted from a
  discovered host → no extra row; one mount seen as IP then as `.local` across two passes → one saved row; mount-saved
  servers absent from the prefetch set); `clippy`, `clippy-windows`.
- **DONE**: with both mounts present, `server_rows` lists both servers, status connected, and a Servers visit dials
  neither.

### M5: `SmbServerVolume` lists `smb://`, live

- **Scope**: `list(smb://)` returns the M3 rows as entries (paths per § "Fit check", every row a directory);
  `get_metadata` on a row's path; `listing_watch_coverage(smb://) = EveryWriter` (it already answers that for every
  path); a debounced re-list through `Listings::directory_changed` on every input change; MCP `list_dir smb://` returns
  them.
- **Intentions**: the views can render the Servers list with no special code, and it stays current with no poll.
- **Landmines**: the engine test FIRST for entries whose path isn't under `smb://` (diffs, `findFileIndex`, the
  drop-foreign-listings rule, `indexOfPath`); the volume `root()` stays the constant `smb://` (`RootHistory`, #1511's
  landmine); the change hooks must not fire a re-list per discovery event storm (debounce, one per burst); `smb://<x>`
  share-list behavior is untouched (its `Reader` gate stays, and M2's forced-refresh pin stays green). Until M8 no pane
  opens a listing on `smb://` (the hub renders), so the re-list must cost nothing with no listing open
  (`listings_for_path_on_volume` empty → no work).
- **Test plan**: Rust tests: zero dials and zero Keychain reads for `smb://` from every reader; a host found → one
  re-list; an engine-level `Listings::start` on `smb://` completes with server rows.
- **DONE**: a listing of `smb://` returns the fixture servers through the engine and updates when a host appears.

### M6: the kind, the cells, the verbs, and the share list's saved facts (not yet reached)

- **Scope**: branches 1–8, 10, 11 behind the `network` kind (cells, `dateHeader`, badge, icon ids in `FileIcon`, words,
  open arm, keys, F-key labels, row menu host, palette target, MCP tokens, footer, every verb refusal incl. the
  drop-on-row pin and § "Fit check"'s cross-scheme path list), plus § "Decisions", 10 on the share list (saved/mounted
  dot, Pin and Forget share in the share-row menu, Enter on a saved share mounts as its saved account through its
  place). `paneViewKind` still renders `NetworkMountView`, so `main` behaves as before for Servers; the share list gains
  its saved-share facts live.
- **Intentions**: every generic seam knows the Servers list before the switch, so M8 is a switch, not a hunt.
- **Landmines**: `rg "=== 'network'"` and `rg "capabilitiesFor\("` are the real audit; each kept volume-only check says
  why in the report. ❗ Find out which handler wins F2 / F7 / F8 / ⌃⏎ on `network` TODAY (the hub's own keydown or the
  global command): the new arms must not double-fire beside the hub's while it still renders (M6 to M8). The F7 / F8
  arms must not change what F7 / F8 do on the share list (still refusals). The badge's words are the row's accessible
  name's, ❌ not a second live region. A saved share's mount uses its SAVED account, ❌ not the server's session
  credential (today the hub's place row does). Showing a saved share's dot on the share list reads `known_shares` and
  the volume list only, ❌ no Keychain.
- **Test plan**: unit tests per branch; a FilePane component test that a tab on `network` + `smb://` with a mocked
  listing renders Full with Name / Details / Last used and no Size, server glyphs, and the status badge; refusal tests
  per verb; `svelte-tests-related`.
- **DONE**: the component test green; the share list shows saved shares' dots and menu items in the app.

### M7: the trailing row (generic, not yet reached)

- **Scope**: branch 13: the `trailingRow` prop on `FullList` and `BriefList`, the ONE `trailingRowCount` helper every
  index consumer reads, cursor reach (↓, End, PageDown, click), Enter / double-click / MCP `open_under_cursor` dispatch,
  never selectable or counted or draggable or a drop target, visible under a quick filter, skipped by type-to-jump, the
  MCP mirror line, a11y name. Only the `network` kind sets it (to `servers.connect`), so nothing changes on `main` until
  M8.
- **Intentions**: one generic row the views draw for any kind that asks, the `..` row's mirror image, ❌ no `if server`.
- **Landmines**: § branch 13's consumer list; `hasParentEntry()`'s +1 and the trailing row's count must compose (a kind
  could have both; Servers has none); the virtual scroller's total height and Brief's column count include it; cursor
  restore across a rebuild must not land on it when the cursor's row still exists; a cursor ON it stays on it across a
  re-list. `FilePane` is at its cap: a `*.svelte.ts` factory.
- **Test plan**: component tests on a stub kind with a plain mocked listing (sort desc, filter matching nothing, ⌘A,
  Space, ⇧↓ through it, End, type-to-jump letters, MCP indices); axe on the stub; `svelte-tests-related`.
- **DONE**: the stub-kind tests green; no view file names a kind.

### M8: the switch

- **Scope**: `paneViewKind` routes `network` to the list; the discovery hold factory (branch 12); the landing (branch
  9); the Add server doors (F7 and the trailing row); the prefetch per M1's verdict (stop it, or narrow it to saved
  servers under the #324 gates) and the stale refresh; `refreshPane`'s `isInNetworkView` branch; delete
  `NetworkMountView.svelte`, `ServersHub.svelte`, `ServersHubStatusBar.svelte`, `ServersHubRowMenu.svelte` (if branch 7
  doesn't reuse it), `servers-hub-{rows,items,keys,shares,columns,mcp}.ts`, `servers-hub-list.svelte.ts`,
  `network/list-cursor.ts` (❌ not `lib/ui/list-cursor.ts`, which other lists use), `hub-landing.svelte.ts`, the NFS
  hint and its keys, the nearby-group setting, and their tests; move the E2E specs to the new MCP spelling; flip M1's
  share-listing-counter baseline into the #324 pin.
- **Rollback**: a tab state this build writes is still `network` + `smb://` or `smb://<server>`, which the previous
  build reads; check it once and say so.
- **Intentions**: every M1 pin green with the standard rendering.
- **Landmines**: `getNetworkCursorEntry`, `getNetworkItemCount`, `isInNetworkView`, and `NetworkCursorEntry` readers
  (`DualPaneExplorer`, `move-cursor.ts`, `pane-commands.ts`, `pane-mirror.ts`, `navigation-transaction-handlers`);
  `pane-mirror.ts`'s Copy-path-between-panes onto Servers; the pinned-tab fork (Servers on a pinned tab → share list
  opens a new tab, as any folder); `holdDiscoveryForServersView` must release when the pane leaves `smb://` (and both
  panes share one lease); `pane/CLAUDE.md`'s "`network` at `smb://` is the SERVERS HUB (`NetworkMountView`, own MCP
  push)" must-know changes in this commit.
- **Test plan**: M1 E2E on macOS and Linux; axe on a Servers pane; `svelte-tests-related`; `clippy`.
- **DONE**: `rg -w 'ServersHub|NetworkMountView|hubLandingFor|nearbyServersGroup|prefetchSharesCmd' apps/desktop/src`
  finds nothing (or, if M1 kept a saved-only prefetch, only that); the screenshot shows a flat Servers list ending in "+
  Add server…" with the footer band at the share list's height.

### M9: close-out

- **Scope**: dead i18n keys (`servers.hub.nfsHint*`, `col*`, `moreShares`, `nearbyGroup`, the status-bar words if
  unused), `knip`, the full SMB and servers E2E on macOS and Linux, `pnpm check --only-slow`, docs audit of every
  touched C+D.md (the network `DETAILS.md` hub sections shrink to the Rust merge and the kind), walk the coverage
  contract.
- **DONE**: the conformance review against the invariants finds nothing; this spec is ready to wipe per
  `docs/specs/DETAILS.md`.

## Questions (David)

1. **What should ⌘R on a share list show when nothing changed?** It does re-read today (the log shows a fresh read of
   Naspolya's eight shares in about 76 ms), but the rows come back the same, so nothing moves. (a) Nothing, like a
   folder; fix only the dead ⌘R on the sign-in or refusal screen. (b) A short "Up to date" in the footer after any ⌘R,
   on every pane, so a refresh always answers. (c) (b), but only on network panes, where a refresh is a real round trip.
   **Recommended: (a)** for consistency with folders, unless you were expecting to see new shares that didn't appear (if
   so, say which server: that would be a different bug).
2. **#1518: how strict should share identity get on Samba?** A Samba box's GUID is its NetBIOS name, so two Pis both
   named `raspberrypi` with a `share` share look like one share today. (a) A name-shaped GUID is only a hint: Cmdr never
   lets one share answer for the other. Safe, but your NAS by LAN and by Tailscale also stops pairing in Ask Cmdr (it
   lists the Tailscale one live instead of reusing the LAN one's index). (b) Keep pairing name-shaped GUIDs only when
   both sides also signed in as the same account. Keeps your NAS pairing, still wrong for two Pis with the default `pi`
   account. **Recommended: (a)**: a wrong answer from the wrong box is worse than a slower live listing.

## Invariants (the conformance register)

1. Building the Servers list never dials a server or reads the Keychain, for any reader; a server's shares are read only
   when a pane opens its share list (or by a saved-only prefetch, if M1's numbers keep one).
2. The Servers list is flat: one row per server, ❌ no share rows; every row's path is unique; every row is a directory.
3. Every server with a mounted SMB share is listed; two addresses are two rows; nothing merges servers by GUID.
4. The views, `SelectionInfo`, `FileIcon`, and the listing engine hold no Servers-specific code; server facts reach them
   only through `ServerMeta` via `extCellText`, the date cell, the badge hook, reserved icon ids, `ListWords`, and the
   generic trailing-row prop.
5. Every row action resolves its saved entry by the row's own id, never by host, name, or address.
6. `..` from a share list lands the Servers cursor on that server's row; a restored share-list tab lands there without
   listing anything.
7. The Servers list updates on discovery, store, mount, and volume changes by subscription, never a poll.
8. No file operation takes a Servers row as source or destination, and a Servers row is no drop target.
9. The MCP mirror is the standard one: indices match the screen, tokens are locale-independent.
10. The trailing row is no entry: never selected, counted, sorted, filtered out, dragged, or an operation's source,
    target, or drop target; it's always last.
11. ⌘R on a share list re-reads past the share cache; an unforced refresh never dials it.
12. A name-derived Samba GUID never makes two shares one (`same_share`, `redirectable`).

## Bugs spotted (outside this plan)

- None open beyond what this plan now covers: #1518 moved into M2. Worth knowing, not a bug: every guest-refused share
  listing with no credentials runs `smbutil view` against the macOS Keychain (`smb_client.rs`), so each ⌘R on such a
  share list reads the Keychain once more; Cmdr's own store stays once per session.
