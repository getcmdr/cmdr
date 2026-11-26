# Network browser details

Depth and rationale. `CLAUDE.md` holds the must-knows; the architecture, flows, and full decision detail are here.

## `network-store.svelte.ts`

Module-level `$state`, consumed via exported getter functions (never import raw state).

Key state:

- `hosts: NetworkHost[]`: discovered hosts, sorted alphabetically by getters
- `discoveryState: DiscoveryState`: `'idle' | 'searching'`
- `resolvingHosts: SvelteSet<string>`: host IDs currently being resolved
- `shareStates: SvelteMap<string, ShareState>`: per-host share listing status + result
- `prefetchingHosts: SvelteSet<string>`: hosts being background-prefetched
- `credentialStatuses: SvelteMap<string, CredentialStatus>`: `'unknown' | 'has_creds' | 'no_creds' | 'failed'`

Lifecycle:

- `initNetworkDiscovery()`: call once at app startup. Idempotent. Subscribes to Tauri events (`network-host-found`,
  `network-host-lost`, `network-host-resolved`, `network-discovery-state-changed`).
- `cleanupNetworkDiscovery()`: unlisten all events, reset `initialized`.

Resolution → prefetch pipeline (fire-and-forget):

1. `startResolution(host)`: calls `resolveNetworkHost`, updates host, then calls `startPrefetchShares`.
2. `startPrefetchShares(host)`: while a Servers view is on screen, for a host the person SAVED, calls
   `prefetchSharesCmd` (backend caches result), then `fetchSharesSilent` to populate `shareStates`. With no Servers view
   up, or for any other host, it does nothing.
3. `holdDiscoveryForServersView()`: on the FIRST view shown, runs `startPrefetchShares` over every known host, so the
   saved ones list as the view opens (each row shows its own loading state until its list lands).

❗ **Nothing is listed at launch, and a host Cmdr only found is listed when the person opens it, and at no other time**
(#324). Listing a host's shares means connecting to it and signing in, as a guest where it lets one in, so prefetching
every resolved host made each launch sign in to every SMB machine on the network, and the hub then labelled a machine
the person never touched "as guest". A saved server's prefetch waits for a Servers view too: at launch nobody is about
to pick a share, and the browse window (`docs/notes/performance/mdns-browse-gating-2026-09-27.md`) would otherwise turn
into a connection per saved host per launch. Three places hold the found-host line, and a fourth caller of
`listSharesOnHost` has to hold it too:

- `startPrefetchShares`: no prefetch.
- `refreshAllStaleShares` (entering the Servers view): a saved host's stale list is re-read; a found host's is DROPPED,
  so opening it reads a fresh one (the share list answers from the cache without asking how old it is).
- The hub's refresh (`ServersHub.handleRefreshClick`, ⌘R): every host's list is dropped, and only the saved ones are
  re-read.

"Saved" is one question with one answer: `servers-hub-rows.ts::savedSmbHostIds`, the match the hub's merge makes, so a
host that gets no prefetch is exactly a host the hub puts in its nearby group, plus a saved host still
`awaitingFirstConnect` (saved without contact by "Add anyway" or an approved Ask Cmdr card): it's the person's, but
nothing contacts it until they connect, so it's listed only when they open it (#777), and minus a host Cmdr saved off
someone else's mount (`savedFromMount`, Rust `network/DETAILS.md` § "Servers behind mounted shares"), whose shares
nobody opened in Cmdr. The store reads `listSavedServers()` fresh per decision (`readSavedServers`, one read shared by
every host that asks while it's out), because a host becomes the person's the moment they add it or mount one of its
shares, and nothing tells the store when. A read that breaks answers "none saved": not knowing whose a host is must
never be what signs Cmdr in to it. Pinned by `network-store.prefetch.test.ts`. The backend has no other caller that
lists unasked: `smb_client::list_shares` is reached through the three listing commands, all frontend-invoked, and the
share-list volume's sign-in ladder (`share_list_sign_in.rs`), which runs only for a pane that opened a share list
(verified by reading its call sites, 2026-11-26). E2E builds count every call per address
(`apps/desktop/src-tauri/src/network/share_listing_tally.rs`), which `servers-list-flows.spec.ts` reads: opening Servers
asks a saved server (the prefetch) and never a found one.

**How long a share list's first read takes** (the wait the prefetch hides; measured 2026-11-26 for getcmdr/cmdr#1517 §
"Decisions", 7: `stall_probe::listing listing_done path=smb://<server>/ read_dir_ms`, a `read_dir_ms` of 0 or 1 excluded
as a share-cache answer). The threshold is the pane's wait reveal, 800 ms (`WAIT_REVEAL_AFTER_MS`):

- Real servers, a dev log (macOS, a home LAN plus Tailscale): a Raspberry Pi saved by name (signs in with Cmdr's stored
  password) n=13, p50 88, p90 643, max 1,174 ms; a QNAP NAS saved by LAN IP n=3: 83, 87, 517; the same Pi by its IP (a
  saved share; guest refused and no Cmdr credentials, so every read runs `smbutil view` with the macOS Keychain, about
  1.15 s of it) n=5: 88, 1,183, 1,189, 1,418, 1,422; the NAS by its mDNS name (found) 80, 425, 450; the NAS by its
  Tailscale IP (mounted only) 452, 495, 554. Saved servers pooled: n=21, p50 88, p90 1,189, max 1,422. Without the Pi by
  IP: n=16, p90 643. The prod and other dev logs held no share-list reads (older builds).
- Docker fixtures (Linux E2E lane, `servers-list-flows.spec.ts`): Guest 307, Guest by IP 517 and 586, Unicode 739 and
  740, 50 Shares 844 and 1,048 ms (n=7, p90 1,048).

Verdict by the threshold: the saved servers' p90 is at or over 800 ms, so the prefetch stays for SAVED servers only. The
slowest real reads (the Pi by IP) are the `smbutil` Keychain fallback, so prefetching such a server reads the macOS
Keychain on every Servers visit.

Key exported functions: `getNetworkHosts()` (sorted copy), `fetchShares(host)` (explicit, throws on error),
`refreshSharesIfStale(host)`, `refreshAllStaleShares()` (call on entering network view),
`forgetCredentials(serverName)`, `setCredentialStatus` / `getCredentialStatus` (in-memory only), `setShareState` /
`clearShareState`, `getDiscoveryState()`, `isHostResolving(hostId)`, `getShareState(hostId)`, `getShareCount(hostId)`,
`isListingShares(hostId)`, `isShareDataStale(hostId)`.

## `ServersHub.svelte`

The pane state behind the switcher's "Servers" row: a table of Name, Type, Address, Status, and Last used over every
server the user saved, then the hosts mDNS is seeing that they didn't, folded into one group (§ "The nearby group"),
with an "Add server…" pseudo-row at the bottom (keyboard navigable, "+" icon, italic), firing `onConnectToServer`.
Keyboard nav via `handleNavigationShortcut` (`../navigation/keyboard-shortcuts`); Left/Right jump to first/last.

Row icons come from `servers-hub-rows.ts::hubRowIcon`: an SMB server is `monitor-filled` (a monitor with a filled
screen, the way macOS draws one) in `--color-server-icon` (the accent, darkened in light mode to clear 3:1; gated by
`scripts/check-a11y-contrast`), any other server the Lucide `server`, a share a `folder`. All decorative: the name
beside them says what the row is. On a server's share list, each share draws as the platform folder stacked
(`../selection/DETAILS.md`, the `FileIcon.svelte` section).

❗ **The hub lists nothing until its first read of the saved list answers** (`servers-hub-list.svelte.ts`). Which
servers are saved decides where every row goes, so listing the discovered hosts first would put them all in the nearby
group, open it, and leave the cursor on a row that then moves. A read that breaks counts as answered, with none saved.

Three inputs, and one of them is a command rather than a store: `listSavedServers()` (re-read whenever the volume list
is reassigned, which every pin, forget, connect, and disconnect causes through `volumes-changed`), the discovery store's
hosts, and the volume list, which is where a place's standing lives.

❗ **Why a pane state and not a Settings list**, and the rule that comes with it: the hub is this app's pilot for
putting a manageable list where the user already navigates, so the saved-server list does NOT also live in Settings (a
Settings card keeps only what the hub can't show, the trusted host keys). The rule the pilot carries: **a pane state may
be a LIST, ❌ never a FORM.** Enter, F8, ⌘E, and the row menus act on the row under the cursor; anything that asks a
person to TYPE opens the sign-in sheet (`../../servers/DETAILS.md` § "The four rules", rule 3). A form inside a pane is
what the SMB login form used to be, where Tab meant "switch panes" to Cmdr and "next field" to macOS and the form lost
either way.

### What Enter does, per row kind

- **An SMB host** opens its share list (`onHostSelect` → the pane's `onOpenServer`, which sends the pane to
  `shareListPath(host)`). A saved host mDNS isn't seeing right now is handed over as a synthesized `NetworkHost` whose
  `hostname` is the saved address, the only spelling anything has for it; the backend's resolver falls back to the
  manual server the same way.
- **A one-place server** (SFTP, WebDAV) takes the pane to its place (`onServerSelect` → `NetworkMountView` →
  `onVolumeChange` with the place's `appRoot`). ❗ The PANE does the dialing, not the hub: landing on a `saved` volume
  is what `../pane/place-connect.svelte.ts` watches for, so the connecting view and its Cancel render where every other
  wait does.
- **A saved SMB share** (a row right under its host, `docs/specs/saved-smb-shares.md`) takes the pane to its place when
  the volume list has one, exactly like a one-place server, so an unmounted one is mounted IN THE PANE. One no mount
  went through yet (Add named it) opens its host's share list and mounts that share for the visit (`onShareViaHost`, the
  arrival's queued mount).
- **An unsaved share** of a saved server (§ "A saved server's other shares") goes the same way as a saved share no mount
  went through: its host's share list, mounting that share, and that mount is what saves it.
- **The "More shares on …" row** opens its server's share list, exactly what Enter on the server row does.
- **An S3 place** (a bucket, or the account root, right under its account) takes the pane to its place, exactly like a
  one-place server. **The S3 account row** is no place, so Enter answers a toast pointing at the places under it.
- **The add row** opens the one sign-in sheet in add mode (`../../servers/open-sign-in.ts`). An SMB address comes back
  as a hand-off, and `NetworkMountView` opens the injected host's share list.

`servers-hub-rows.ts::openMoveFor` is the one place that decides which of these a row gets
(`servers-hub-shares.ts::shareItemMove` for the two share items); the component only carries it out.

### A saved server's other shares

Under each SAVED SMB server the hub lists every share its prefetched list holds, not only the saved ones (#1404): a
person who saved one share of a Mac read the server as having just that one.

- **Off the list the prefetch already made, ❌ never a new listing** (`servers-hub-list.svelte.ts::prefetchedShareNames`
  reads `getShareState`). A server Cmdr only found gets none: the #324 line holds, and this adds no `listSharesOnHost`
  caller.
- **They're ITEMS, ❌ not `HubRow`s** (`servers-hub-shares.ts::ShareItem`, spliced in by `hubItems`). Nothing about them
  is saved, so `getRowUnderCursor()` is `null` on them, like on the group's header: F8, the row menus, and the palette's
  server commands find nothing, and no account, pin, or Forget share shows. An unsaved share is dimmed, reads "Not
  connected", and is a type-to-jump name.
- **Order and cap**: saved shares first, then the rest by name (case-insensitive; a saved share matches case- and
  Unicode-form-blind, so it never shows twice). At most `MAX_SHARE_ROWS_PER_SERVER` (8) share rows per server, the saved
  ones counted but ❌ never hidden: past the cap only the More row is added.
- **The More row** ("More shares on {server}…", italic like "Add server…") stands last whenever a share isn't on screen
  or nobody knows yet: capped, still loading, needs a sign-in, couldn't be read, or never read (a saved server mDNS
  isn't seeing, or one still `awaitingFirstConnect`).
- **Rows land late, under the cursor**: the prefetched list arrives after the view opens. `cursorAcrossRebuild` follows
  the item by id, so the cursor stays on the same item (`ServersHub.test.ts` pins it). Item ids are
  `unsaved-share:<server id>:<name>` and `more-shares:<server id>`.

### The nearby group

A host Cmdr only FOUND (no saved server, no saved share, nothing the person added: `servers-hub-rows.ts::isNearbyOnly`)
sits under one header row, below everything saved. The reason: a saved server and a machine that happens to be on the
same Wi-Fi had the same weight in the list, and most people never connect to the second kind. A saved host that mDNS
also sees stays among the saved ones with its "Found nearby" status.

- **Open or collapsed** (`servers-hub-items.ts::isNearbyGroupExpanded`): what the person last chose, else collapsed for
  someone with a saved server and expanded for someone with none, for whom the nearby servers are the whole view. The
  choice lives in the hidden `network.nearbyServersGroup` setting (`auto` until the first toggle), read reactively
  through `getNearbyServersGroupChoice`, so the two panes' hubs fold together.
- ❗ **An untoggled group goes by whether a server was saved when the view OPENED, ❌ not by the live count.** A first
  save (Add, or mounting a found host's share) would otherwise fold the group away under the person looking at it. The
  next visit opens collapsed.
- **Keys**: the header is a row to the cursor. Enter and Space toggle it (Space only there), a click toggles it and
  takes the cursor, and Left/Right stay the list's first-and-last jump, as on every row. F8, ⌃⏎, and the palette's
  server commands find no row on it (`getRowUnderCursor()` is `null`), like on "Add server…".
- ❗ **Two index spaces** (`servers-hub-items.ts`). The cursor counts what is ON SCREEN (`visibleHubItems`), so no key
  can land it on a hidden row. `cmdr://state`, `findItemIndex`, `setCursorIndex`, and `getItemCount` count the FULL list
  (`hubItems`), which keeps the nearby servers whatever the group's state: an agent asking which servers exist gets the
  truth, and the group's entry says `state=collapsed`. `setCursorIndex` onto a hidden server opens the group for this
  view (`nearbyRevealed`) and ❌ doesn't record a choice; `selectServer` goes the same way.
- **A group that collapses over the cursor** (the other pane's hub toggled it, `set_setting` did) puts the cursor on the
  header: `servers-hub-keys.ts::cursorAcrossRebuild` treats the hidden row as one that left.
- **The status bar still counts every server**, hidden ones included; the header says how many of those are nearby. Its
  height, top edge included, is every pane footer's (`../pane/DETAILS.md`, "One footer height everywhere").
- **Discovery off** empties the discovery list, so there is no group and the "discovery is off" line stands where it
  would be. **Searching** shows its line under an open group or when nothing was found yet; a collapsed group shows a
  spinner on its header instead.
- **Type-to-jump looks through a collapsed group** (`jumpToPrefix` walks the full list). Its rows come last, so a name
  on screen wins, and a jump into the group opens it the way an agent's `move_cursor` does, without recording a choice.

### Typing jumps

The hub answers type-to-jump from memory: `jumpToPrefix` moves the cursor to the first row whose name starts with the
buffer (`list-cursor.ts::nameStartsWith`, case-insensitive) and skips the group header and "Add server…". The buffer,
timeout, and indicator are the folders' own (`../pane/type-to-jump-controller.svelte.ts`, through `getInMemoryJump`). A
prefix rather than the folders' fuzzy match: the hub holds a handful of names you read off the screen. With "typing
filters" on the hub still jumps (`typingFilters`), since it has no backend listing to narrow. A server's share list is a
real listing, so it takes the folders' own type-to-jump and quick filter.

### The pure modules beside it

`ServersHub.svelte` is the table, the cursor, and the keys. Everything that can go quietly wrong lives next door and is
unit-tested:

- **`servers-hub-rows.ts`**: the merge, the status derivation, and the order. ❗ The merge is the part that goes wrong:
  a manually-typed SMB host is BOTH a saved server and a discovered host (adding one injects it into the discovery
  state), so concatenating the two sources shows a person's NAS twice. The dedup matches on the id first, then on the
  name or resolved hostname, because `known_shares` files a host under the server name `statfs` reported while mDNS
  files the same machine under its Bonjour name. Status comes off the VOLUME LIST, ❌ never off `SavedPlace.connected`,
  which is a snapshot from when the listing was built; the switcher's dot reads the same field, and two surfaces
  disagreeing about whether a server is up is worse than either being briefly stale. Order: live sessions, then the ones
  asking something of the user (`signed_out`, `waiting_for_key`), then the rest of what they saved by recency, then the
  hosts Cmdr only found, which stay contiguous at the end whatever their name or recency because the nearby group's
  header sits in front of the first one. ❗ A saved SMB host is "Found nearby" only when a DISCOVERED host matches it,
  ❌ never because of its own manual entry in the discovery list: Cmdr injects every typed-in host there at startup,
  reachable or not. The Address column carries the port off 445 (`localhost:11482`), and `savedHostFor` reads it back
  when mDNS sees nothing.

  ❗ **A saved SMB host still `awaitingFirstConnect` reads `not_connected_yet`** ("Not connected yet"), even when mDNS
  sees it, ranked with `saved`: "Saved" with no shares under it read like an add that didn't work (#777). Enter opens
  its share list like any host's, which is the user connecting; its first mount clears the flag.

  ❗ **A many-place server's places are rows right under it** (`hasManyPlaces`: an SMB host's saved shares, an S3
  account's saved buckets and root; `kind: 'place'`, id `share:<volume id>`), placed AFTER the sort so they never drift
  from their server. Each place's status comes off the volume list by its id like every place's. The server row keeps
  `volumeId: null` and `pinned: false`: its places are the place rows. A share row names the account it opens as; an S3
  place names none, since the account row above it shows the key.

  ❗ **An S3 account row carries the NAME, its place rows their buckets**, as an SMB host carries its name over shares
  that read as themselves: a bucket row reads as the bucket the provider spells, and the account ROOT's row reads
  `servers.hub.s3AllBuckets` ("All buckets") and leads the places. The backend labels the root as its account (what the
  switcher and a pane show), which under the account's own row would repeat the name; `isS3AccountRoot` reads the root
  off its app root's empty server path. The model: `src-tauri/src/network/DETAILS.md` § "The S3 twin, a place per
  entry".

  ❗ **An S3 account row is no place.** Its id is the account ROOT place's id whether or not the root is saved, so
  acting through it would dial or pin the root behind the person's back. Each S3 place row connects, pins, edits, and
  forgets through its own volume id with the regular server-place menu (`servers-hub-actions.ts`); the account row's
  Enter answers a toast pointing at its places (`openMoveFor`'s `account` move), its Edit opens the account's editor
  (rename it, change its secret: `#lib/servers/DETAILS.md` § "An S3 edit is the ACCOUNT's or a PLACE's"), it has no
  right-click menu, and F8 forgets every place plus, box checked, the secret they share (written FIRST, while a place
  still names the entry). The account's status is its most urgent place's. Forgetting one place offers that shared
  secret UNCHECKED (`server-row-actions.ts::forgetSavedServer`), since the account's other places sign in with it.

  ❗ **The merge also guarantees every row id is UNIQUE**, first writer wins. The hub keys its `{#each}` on `row.id`,
  and Svelte throws `each_key_duplicate` on a repeat, so a duplicate crashes the whole pane rather than showing a row
  twice. `listSavedServers()` unions three stores, so a server recorded in two of them arrives twice and the name/id
  dedup above doesn't catch it: `smb_hosts` (`commands/servers.rs`) dedupes by ADDRESS while the row id comes from a
  normalized server NAME, so two rows for one host can differ in address and agree on id. Seen for real in the Linux E2E
  suite: the hub went blank and every `move_cursor` onto a host afterwards reported the row missing.

  ❗ **The Name column ranks three names**, in `servers-hub-rows.ts::displayName`: a name a PERSON chose (an SFTP or
  WebDAV account someone named), then the DISCOVERED Bonjour name, and last a stand-in nobody chose (an unnamed
  account's derived `username@host`, which keeps its place because only SMB rows match a discovered host). The rank
  comes off `SavedServer.nameSource` (`commands/servers.rs`'s `ServerNameSource`), a fact the store that wrote the label
  publishes, ❌ never a guess at the string's shape. Without it a person's NAS renames itself the moment they use it: an
  unnamed SMB label is a stand-in, either the way `statfs` spells the server (written to `known_shares` on the first
  share listing, `smb-consumer-guest`) or the address typed into "Add server" with the Name field left empty
  (`manual_servers` derives `host` or `host:port`), and the friendly name they recognize (`SMB Test (Guest)`, `Testnas`)
  would drop out of the column. A name typed into the add or edit sheet is `user` and wins. Four `smb.spec.ts` specs
  poll on the Bonjour name and are the regression guard.

  ❗ **A saved SMB server claims EVERY host that matches it, not the first.** One machine sits in the discovery list
  twice once a person types a host mDNS already found: the manual entry injects a `manual` host beside the `discovered`
  one. `primaryHost` then picks the DISCOVERED one for the row's name and address, because a manual host is named after
  the address that was typed.

- **`servers-hub-list.svelte.ts`**: the one stateful sibling. `createHubList()` holds what a hub lists as live state
  (the saved servers it read, the rows and items built from them, whether the nearby group is open), one instance per
  hub, so the component keeps only the cursor and the keys.
- **`servers-hub-items.ts`** (+ `servers-hub-keys.ts`): the list as the cursor walks it, over the rows: the nearby
  group's header, which items are on screen, the two index spaces, and where the cursor goes across a rebuild (§ "The
  nearby group").
- **`servers-hub-mcp.ts`**: the `name` encoding. MCP's `PaneFileEntry` has only `name` / `path` / `isDirectory`, so the
  columns are encoded as `protocol=` / `status=` / `address=` tokens (plus `shares=` on an SMB host whose shares were
  listed, which is what `smb.spec.ts` polls on). The nearby group's header is an entry too:
  `Found nearby  kind=group  state=<expanded|collapsed>  servers=<n>` at `smb://nearby`, which `move_cursor` finds by
  that name and `open_under_cursor` toggles. ❗ The status token is locale-independent even though the column beside it
  is translated: an agent parses these strings and a translation landing in the wire would break both silently. A
  one-place row's path is the place's `appRoot` (from `SavedPlace`, which Rust mints in one function); an SMB host keeps
  the `smb://<address>` spelling the host list publishes; an S3 account row publishes the account's own prefix
  (`s3://<key>@<host>:<port>`) and its place rows `kind=place` (a share row keeps `kind=share`); the add row is
  `+ Add server…` at `smb://add`. A saved server's unsaved share carries `status="not_connected"` and
  `kind=unsaved_share` at `smb://<address>/<name>`, its More row is `More shares on <server>…  kind=more_shares` at
  `smb://<address>` (English, like the group's name). `hubItemMcpName` is the name `findItemIndex` matches.
- **`servers-hub-actions.ts`**: F8 and the row menus (an SMB host's included), behind live getters (❌ never snapshots:
  the rows change under a menu that is still open). ❗ A one-place row and an SMB host take different paths at every
  branch, which is why they live in one unit: a one-place row is a PLACE the servers family speaks for, an SMB host is a
  manual-server entry whose "disconnect" unmounts shares rather than dropping a session.
- **`../navigation/servers-hub-rows` consumers**: `../pane/types.ts`'s `NetworkCursorEntry` gains a `server` arm, which
  is how the palette's server commands reach the row under the cursor (`#lib/servers/server-command-target.ts` owns the
  rule: the hub IS a pane, so "the focused pane's volume" would answer the synthetic hub row). The `host` arm carries
  its row too, so Edit and Rename reach an SMB host (`editHubRow`, `#lib/servers/DETAILS.md` § "Which server a command
  acts on").

### Discovery off

`network.enabled` gates mDNS and SMB, which is what the macOS Local Network permission is about; SFTP, WebDAV, and S3
need none of it. So the hub opens either way, keeps listing saved servers, and shows one line plus a link to the switch
in place of the nearby hosts. ❗ No "(disabled)" label, and no redirect to Settings; `network-toggle.spec.ts` is the
regression guard.

### Context menu and F8

F8 forgets the SAVED server under the cursor: a one-place row through `forgetSavedServer` (so the hub asks exactly what
the switcher's menu asks), an SMB host through `forgetSavedSmbHost` (its manual entry, its sign-in history, and its
saved shares; nothing unmounted unasked, but while a share from it is mounted the question offers "Unmount and forget",
since Cmdr would save a mounted share's server right back, and a share that won't go keeps the host), a saved share
through `forgetServer` on its id (the row and its pin only), an S3 place like a one-place row, an S3 account as every
place under it (§ "The pure modules beside it"), and a host only mDNS knows about gets the "Can't remove discovered
hosts" toast. A share row's right-click is an in-app menu too: Open, the pin, and Forget share. Right-click on a
one-place row opens the house `Menu` at the pointer, holding the same list the switcher row's → submenu shows
(`../navigation/row-menu.ts`; Open moves THIS pane, like Enter). An SMB host's is the same house `Menu`, from
`row-menu.ts::smbHostRowMenu`: Open, Edit… for a saved host, Disconnect while a share from it is mounted (the one fact
the menu asks on open, `smb_host_has_mounts` over the kernel's mount table), the server's account items (Sign in as…,
Browse as guest where a guest listing worked, Forget saved password), and Forget server for a saved one. ❗ Opening it
reads no password: the account items come from what this session's listings said, and Forget saved password is offered
unconditionally, answering "there was none" itself (`share-list-sign-in.ts::forgetPassword`). An account action runs
`runServerAccountAction`, the one runner the path bar's account menu and the palette share, and a Sign in as… or Browse
as guest that lists opens the server's share list, as activating the row does. Cursor auto-clamps when a row disappears.

`⌃⏎` (`file.contextMenu`) opens the cursor row's menu from the keyboard, the same menu a right-click opens, placed just
under the row by `../pane/context-menu-anchor.ts`; a right-click opens it at the pointer. The "Add server…" row has
none. On a server's share list, right-click and `⌃⏎` on a share open the share-row menu (§ "A server's share list").

❗ **Every row action carries the row's own id, end to end.** `ServersHubRowMenu` holds the row by its exact id and
reads it live (a row gone meanwhile gets nothing), `runRowEntry` acts on that row, and the backend's
`forgetSavedSmbHost` / `updateSavedSmbHost` take that id alone and find the host in the same listing. ❌ Never resolve
"which row" from a host id, name, or address at action time: two rows can stand on one discovered host, and a lookup by
host picked the first, so "Edit server…" on the second row saved into the first (QA 2026-09-25).

Exports for parent: `setCursorIndex(index)`, `findItemIndex(name)`, `handleKeyDown(e)`, `refresh()`,
`getHostUnderCursor()`, `getRowUnderCursor()`, `getItemCount()`, `openCursorItem()`. The three that speak in indexes
count the full list (§ "The nearby group"). `refresh()` is `pane.refresh`'s entry point from the command layer and is
the same body ⌘R runs locally, which is why the local branch stops propagation (see § Gotchas).

**Decision**: `.row-list` is a `role="list"` and everything directly in it is a `listitem`, the hint lines (searching,
discovery off, NFS, empty state) too. **Why**: a list may only hold items, so a hint's bare `LinkButton` trips axe's
`aria-required-children`. `network.a11y.test.ts` pins it.

## A server's share list

A server's shares are a real listing: the `network` volume at `smb://<server>` (`#lib/servers/share-list-path.ts` writes
and reads the path; the backend's `SmbServerVolume` lists it, `src-tauri/src/network/share_list_volume.rs`), and the
standard views render it with the kind's own words (`../pane/list-kind-words.ts`: the Description column, "8 shares",
"Back to Servers"). The hub opens one by sending the pane there; ❌ no component here holds a host. What a share list
does that a folder doesn't is `../pane/share-list-pane.svelte.ts` (pane `DETAILS.md` § "A server's share list"): Enter
mounts a share, a door's arrival lands, a sign-in refusal asks.

**Signing in to list.** The backend's ladder signs the listing in (session credential, the typed account's stored
password read ONCE per session, else guest: `src-tauri/src/network/share_list_sign_in.rs`) and answers a typed refusal
the pane shows as the calm sign-in screen (`ErrorPane`'s Sign in, `share-list-sign-in.ts::askToSignInToList`). ❗ The
frontend reads no Keychain and keeps no password: every stored read is the backend's, and a typed answer crosses once,
for its own attempt (a mount's Try again sends none: `share_list_sign_in.rs::mount_with_sheet_answer`). A listing the
person started (a door, a navigation, Retry) opens the sheet on its own once; cancelling it leaves the pane ON the
in-pane sign-in state (`ErrorPane`'s Sign in, with keyboard focus on it: Enter asks again, ⌘[ goes back), the honest
"you're not signed in" place. A cancel that went back to Servers made that state unreachable while the sheet covered it.
"Sign in as…" opens the same sheet with no refusal and no guest, prefilled with the listed account, and its cancel stays
on the list. A refused MOUNT lands the same way: its sheet opens over the mount's calm sign-in state ("Sign in to open
…", `RemoteConnectView`), and cancelling stays there with focus on its Sign in (`share-mount.svelte.ts`). Every door
that opens the sheet over an in-pane sign-in state leaves the pane on it; doors with none (a hub row's menu, the path
bar's account, the palette) stay where they were.

`share-list-sign-in.ts`, the sheet round: every `attempt` is `listWithAccount`, which answers `handed_off` on success
(nothing here connects a VOLUME, and the loaded list IS the result), keeps the sheet open on `authentication_rejected` /
`needs_credentials`, and closes it onto the pane's own error state for anything else, because that is where the retry
and a missing dependency's install command live. The round answers the attempt that closed it (`listed` or `failed`), or
`cancelled`; ❗ a `listed` carries no password back (the backend keeps the typed account as the server's session
credential, which the re-list and a mount sign in with). A listing that worked leaves every record behind in one place:
the share cache, the credential status, the listed account, the account preference (the choice sticks), the Keychain
when Remember, and the known-shares store. "Browse as guest" (`useGuest`) clears the account preference FIRST, since the
backend never lists a server with an account as guest. ❗ One ask at a time is the MODULE's guard (`sheetRound`): the
error screen, the auto-ask, the hub's row menu, the path bar's account menu, and the palette all open the sheet, and a
second sheet would close the first as `cancelled`, read as "the person said no" from under a sheet that's still up. A
round asked for while one is up answers `null` and asks nothing.

**The account doors** (Sign in as…, Browse as guest, Forget saved password) are one action vocabulary
(`../navigation/row-menu.ts`'s `ServerAccountAction`, built by ONE `serverAccountEntries(facts)`) with one runner,
`runServerAccountAction`, behind every door: the hub's SMB host row menu, the path bar's account menu on a share list
(`ShareListAccountChip.svelte`, a member of the chip's ONE `openMenu`), the share-row menu (`shareRowMenu`, hosted by
the same chip, `openRowMenu`), and the palette (`servers.signInAs`, `servers.browseAsGuest`, `servers.forgetSecret`).
The facts are `serverAccountFacts(host)`: the listed account and whether a guest listing worked, both this session's own
answers, ❌ never a Keychain read. Browse as guest shows only while signed in as a user on a server a guest listing
worked on, and the runner refuses it otherwise (a rebound palette key gets a sentence), because it drops the account
FIRST. Forget saved password shows unconditionally, in every door: a delete raises no prompt, and the store's typed
`not_found` words "there was none". A door that isn't on the share list opens it once an action listed (the hub row: as
activating it; the palette from a hub row: `openSmbHandOffInFocusedPane`); a door on the share list re-lists it.

**Who it signed in as, and what it holds**: a share list's `listing-complete` carries `shareListAccount`, noted into the
store by `share-list-label.ts::noteShareListAccount`, so the path bar's account and the hub's row say the same thing. A
landed share list also refreshes the hub's record of the server's shares (`noteShareListListed`: `fetchShares`, which
the backend answers from the share cache the pane's listing just filled, by the same host id), so the hub row's
`shares=` count and a saved server's share rows follow a visit the way they did when the hub listed it itself.

The error state's sentence, and the servers list's tooltip for the same failure (`host-status.ts`), come from
`share-list-error-messages.ts`: one `errors.shareList.*` key per `ShareListError` type, naming the host. ❌ Never
`error.message`: the backend documents it as diagnostic detail for the log, and it's English, often a fallback tool's
own stderr. `listSharesOnHost` and `listSharesWithCredentials` throw the typed value as a `ShareListFailure`
(`share-list-error.ts`), `fetchShares` throws its own for a host with no hostname yet (`resolution_failed`), and every
catch reads the value back through `shareListErrorOf`. ❌ Never `e as ShareListError`: anything untyped that lands in
the catch (a runtime exception, an IPC call that broke) had no `type`, and the renderer threw on it; it reads as
`protocol_error` now, its text kept for the log. Pinned by `share-list-error-messages.test.ts`.

❗ **A queued share is a one-shot for the visit it was queued with.** The pane's arrival
(`../pane/share-list-arrival.svelte.ts`) holds it per pane, spends it once the list lands (and toasts "not found" when
the list lacks it), and drops it the moment the pane is anywhere but where it was queued from or for. Opening a host
from the servers list only ever lists its shares. The bugs this closes: cmdr-reports#7 ("Cmdr tried to open the share on
that server"), where the value outlived its visit and fired again each time the list came back, so a host row the person
meant to browse mounted a share they never picked. Pinned by `../pane/share-list-arrival.svelte.test.ts`,
`../pane/share-list-pane.svelte.test.ts`, and the SMB E2E.

## `share-list-label.ts`

Which server a share-list path (`smb://<key>`) names, and what a person calls it. The server is the discovery-state host
whose `shareListPath` is that path, ❗ first, since the backend resolves to it and caches shares and sign-in under its
id; else the server a known share's `..` leads to (`share-parent-row.svelte.ts::shareParentServers`). That second source
is the only frontend record of a server nothing but a mount knows (a share macOS mounted by a Tailscale IP): without it,
its share list opened with no host to mount from or sign in to. The label is a discovered host's Bonjour name, a manual
host's name from `..` (the name the person typed, since discovery names a manual host after its address), else the
path's key. The tab label, the path bar, and the error screen read it (`../tabs/tab-label.ts::tabLabelFor`); ❌ never
show the key itself, which is identity only.

## `..` on top of a server's share list

The share list's `..` is the standard parent row (`computeHasParent`: the volume's root is `smb://`), so every index
counts it like any list's. Enter, `⌘↑`, and Backspace on it, or on the error screen where there's no row, go to the
Servers hub (`FilePane`'s `leaveShareList`, which commits `smb://` with no load), and the hub lands its cursor on that
server's row (`../pane/hub-landing.svelte.ts`, derived from the path it left). Escape doesn't: on a file list it never
means "parent". It names the hub in the MCP mirror (`smb://`) and says "Back to Servers" to a screen reader
(`fileExplorer.network.share.parentRowName`, through `ListWords.parentRowName`).

## `..` out of a share's top folder

In an SMB share's top folder, `..` (and Go to parent, which follows the same switch) opens the server's share list with
the cursor on the share it came from (#1238): a person on one share of a Mac couldn't find the drive shared beside it.
Every SMB share, Cmdr-mounted or OS-mounted; SMB only.

- **The switch is one default plus per-share overrides.** The default is `network.showShareParentRow` (Settings > File
  systems > SMB, Behavior card). A share's override lives in Rust (`src-tauri/src/network/smb_share_parent.rs`, the
  canonical description of storage and identity), stored only while it differs from the default, so the row menu's plain
  checkbox never needs a "use default" state. `share-parent-rule.ts` is the frontend's read of the rule.
- **`share-parent-row.svelte.ts` caches Rust's answer per volume id** (`getSmbShareParent`: the server's host, its name
  in words, the share's name as the mount spells it, and the override), so both panes, the switcher's row menu, and the
  palette command read one value. Every write goes through Rust first. Two ways in: the pane asks when it lands on a
  share, and every top-folder listing hands over the answer it carries (`listing-loader.ts`, the `shareParent` of
  `listing-complete`). ❗ `noteShareParent` never lets a `null` replace an answer (a share's volume id names that share
  for good, so a later `null` only means it was asked too early again), and any answer replaces a `null`. Regression
  anchor for #1238's missing `..` on a saved share the pane landed on before its mount was up.
- **The pane side is `../pane/share-parent-pane.svelte.ts`**: it adds the `..` row through `has-parent.ts`'s
  `topFolderLeadsOut`, and leaves for the server's share-list path (`share-list-pane.svelte.ts::openServer`), so the
  history entry holds the path and Back returns to the share. ❗ That `..` row has no parent path, so it's no drop
  target and opens no context menu: its `entry.path` would otherwise be the mount's parent on the boot disk
  (`/Volumes`).
- **Opening the share list is the person opening the host** (#324 holds): it lists, and ❌ never mounts anything. The
  host comes from discovery when it knows the server, else one made from the mount's server the way Add server makes
  one.
- **The landing is the pane's arrival, a one-shot like a queued share** (`../pane/share-list-arrival.svelte.ts`). Once
  the list lands, the cursor goes to the share's row (the listing's own name lookup) and only then does
  `share-parent-hint.ts` raise the once-per-install explainer (`behavior.shareParentRowHintSeen`), so the person sees
  where they landed.
- **"Turn off for all shares" turns the default off and keeps every override**, so a share someone turned on keeps its
  `..`. Default flips never prune overrides either: each is a choice someone made about that share.
- **The toast's buttons are written in macOS order** (dismissive first, primary last). `ToastActions` doesn't flip them
  for Windows or Linux the way `ModalDialog`'s footer does.

## `smb-sign-in.ts`

SMB's side of the one sign-in sheet. The sheet contract, the three SMB sites and what each `attempt` runs, and the
endpoint header per site live in `../../servers/DETAILS.md`; this section is what SMB alone decides.

`openSmbSignInSheet({ host, label?, shareName?, guestAllowed, initialUsername?, refusal?, attempt })` builds the request
(titled by `label`, the name the person gave the server, when there is one; `host.name` stays every lookup's key), and
the caller's `attempt` gets an `SmbCredentialAnswer` (`{ username, password, remember }`) rather than the sheet's
generic submission. ❗ `username: null` IS guest: all three SMB commands take a nullable username and read it that way,
so a separate flag could only disagree with it.

- **The username** is resolved in one order everywhere: what an earlier attempt tried, then `getKnownShareByName()`'s
  last username for this share, then `getUsernameHint()` (the account typed in the add or edit sheet, else the host's
  share history). ❗ Both lookups take the server BY NAME and match on its stable identity in Rust, so a hint saved
  under one spelling (`Testnas`) is found when the sheet opens under another (`Testnas._smb._tcp.local`). ❌ Don't
  rebuild the key in TypeScript: that is what made the two sides disagree once.
- **Remember starts ON, and is ❌ never probed**, because `has_smb_credentials` is `get_credentials(…).is_ok()` and
  asking costs the same Keychain prompt as reading. Why the other protocols seed the box differently:
  `../../servers/DETAILS.md` § "The sheet contract".
- **`guestAllowed` is a per-site call, ❌ not a property of SMB.** The listing offers guest where the host's `authMode`
  says one is allowed; the mount and the upgrade both pass `false`.
- **`refusalForShareError` / `refusalForMountError`** put every SMB failure in `../../servers/connect-refusals.ts`'s
  vocabulary, and ❗ `auth_required` stays distinct from `auth_failed`: telling someone who has never entered a password
  that theirs is wrong is what collapsing the two does. A mount's `permission_denied` is a third answer,
  `account_not_permitted`: the account signed in and the share turned it away, so the password isn't what to fix.

## Data flow

```
App startup
  └─ initNetworkDiscovery() → listNetworkHosts() + event listeners
       └─ startResolution() → resolveNetworkHost()
            └─ startPrefetchShares() → only with a Servers view up, a saved host only:
                 prefetchSharesCmd() → fetchSharesSilent()

User opens the Servers volume → NetworkMountView: holdDiscoveryForServersView()
       └─ first view shown → startPrefetchShares() over every known host
     → ServersHub mounts → listSavedServers() + refreshAllStaleShares()

User double-clicks an SMB host → the pane goes to smb://<server> → its listing (SmbServerVolume)
       ├─ the share cache answers (a saved host's prefetch, or an earlier visit), else the backend's ladder lists
       └─ a sign-in refusal → the calm error screen, and the sheet once (a listing the person started)

User activates a one-place server → onVolumeChange → the pane lands on a `saved`
       volume → ../pane/place-connect dials, RemoteConnectView renders the wait

User activates the "Add server…" row → the sign-in sheet opens in add mode
       ├─ SFTP / WebDAV → connectServer(target) → the pane lands on the volume
       └─ SMB → connectToServer(address, name) → TCP check → save + inject host
            │   ("Add anyway": no check, saved without contact, nothing injected)
            └─ the sheet answers `handed_off`
                 └─ the pane opens the host's share list, its arrival mounting sharePath once it lands
```

## Connect directly

`direct-connect.ts::connectDirectly({ volumeId, shareName })` is the single implementation behind every "turn this
OS-mounted share into a direct smb2 session" affordance: the yellow-dot popup in
`../navigation/VolumeBreadcrumb.svelte`, checking the switcher's "Use Cmdr's fast direct connection" row on an
OS-mounted share (`../navigation/direct-connection-switch.svelte.ts`), the switcher's "Connect directly now" fix on a
share whose switch is on but still OS-mounted (`../navigation/row-menu.ts`'s `runRowFix`), and the retry button on the
OS-mount fallback notice.

The sequence, and who speaks at each step:

1. `triggerNetworkDiscovery()`, because the direct connect opens a TCP socket to a private IP, which fires the macOS
   Local Network prompt anyway, so this is the honest moment to also start mDNS.
2. A persistent "Connecting directly…" toast goes up and comes down on every exit path.
3. `upgradeToSmbVolume(volumeId)` answers a typed `UpgradeResult` and never throws for an outcome. `success` → success
   toast + `requestVolumeRefresh()`. `networkError` → the `upgrade-messages.ts` sentence for that `UpgradeFailure`, at
   `error`. `mountNotResponding` → `mountNotRespondingMessage` naming the share, also at `error` and `stillOnOsMount`:
   nothing was dialed and nothing is gone, so a later press is worth making. `volumeGone` / `notSmbMount` →
   `nothingToUpgradeMessage`, at `warn`: nothing broke, there was just no OS-mounted share left to connect (an unmount,
   an eject, or a network drop between the offer and the press).
4. `credentialsNeeded` → `systemHasSavedSmbPassword` (a prompt-free probe). If macOS/Finder saved one, a native primer
   dialog ("Use the saved password?") cushions the system Keychain consent dialog, whose own text we can't customize. On
   "Use saved password", `upgradeToSmbVolumeUsingSavedPassword` reads consent → direct smb2 → copies the password into
   Cmdr's store.
5. Anything still short of a session goes to the one sign-in sheet.

Three properties callers rely on:

- **It never resolves without having said something.** Every branch, a thrown IPC error included, raises a toast first.
  That's what lets a button call it bare and be sure a press can't look inert. A throw is the one case with no typed
  reason: `announceBreakdown` logs it and toasts the unnamed sentence.
- **The returned `DirectConnectOutcome` describes the volume, not the call.** `connected` / `askingForCredentials` /
  `stillOnOsMount` / `gone` is exactly the distinction a notice needs to decide whether it still has anything to say.
- **The caller names the share.** A volume that's gone has no name left for the backend to look up, so `shareName` is
  what the pressed control showed: the notice's `share`, or the breadcrumb row's name read at click time.

The credential ask is the one app-global sign-in sheet, so the flow raises it itself. ❗ It returns
`askingForCredentials` as soon as the sheet is UP, ❌ not when the user is done with it: the OS-mount notice retires on
that outcome, and awaiting the sheet would leave the notice stacked under it. The sheet's `attempt` is
`upgradeToSmbVolumeWithCredentials`; a server that stopped answering or a share that went away closes it with the same
typed sentence the flow's own paths toast, because no credential can fix either.

**What the sheet says comes from the answer's typed `reason`**, through `REFUSAL_FOR_REASON`, on the opening round and
on every retry: `noCredential` → `needs_credentials`, `credentialRejected` → `authentication_rejected`, and
`accountNotPermitted` → `account_not_permitted`. The backend reads which one from who the attempt went out as and which
step refused it (`src-tauri/src/network/DETAILS.md` § "An auth rejection says what was actually rejected"). ❌ Never
collapse the last two: an account the share turns away SIGNED IN, and answering "wrong password" kept the sheet asking
for a password that worked (ERR-SHUSC). A refusal opens the sheet on the account it turned away (`usernameHint` as
`initialUsername`), because the sentence names that account; with nothing offered, the remembered username pre-fills
instead.

## The OS-mount fallback notice

**The gap it fills.** A direct connect that fails on an AUTO path someone is watching (the FSEvents mount watcher,
Cmdr's own mount) leaves the share working on the macOS kernel mount, at a fraction of the speed and outside Cmdr's
control, announced by nothing louder than a yellow dot. That silence once cost an evening of debugging a "slow"
transfer. The manual "Connect directly" path is deliberately NOT a source here: it already reports its own failure to
the person who clicked it. Nor is the startup pass over mounts macOS already made: nobody asked, and a notice at launch
reads as something breaking.

**The flow.** The backend emits `smb-fell-back-to-os-mount { volumeId, share, reason, displayName }` at most once per
SERVER per app run, and only for a caller that says someone is watching (who speaks, the ledger, and the rationale:
`src-tauri/src/network/DETAILS.md` § "Telling the user about a kernel-mount fallback"). `os-mount-notice-bridge.ts`,
mounted from `routes/(main)/+page.svelte` beside the other event bridges, turns it into a persistent INFO toast
rendering `SmbOsMountFallbackToastContent.svelte`, dedup id `smb-os-mount:<volumeId>`.

**A notice that can't offer a retry doesn't.** The bridge turns `reason` into a `retryable` prop, false for
`shareNotOnServer` and the `STUCK_REASONS` (`upgrade-messages.ts`: `otherServerNotAllowed`, `negotiationInvalid`,
`encryptionUnavailable`, `encryptionRequired`), and the toast then renders a sentence with no button instead of
`osMountFallback.message` with one: `osMountFallback.shareNotOnServer` by default, or the stuck reason's own
(`stuckReason` prop). The same ask gets the same answer, so a button could only fail; the copy says why instead. ❌
Don't pass the raw `reason` to the toast: the component's questions are "is there anything to offer" and, when not,
"which sentence", and every retryable variant answers them the same way. Which reasons are which, and the two
look-alikes that never arrive as this one: `src-tauri/src/network/DETAILS.md` § "Telling the user about a kernel-mount
fallback".

**This Mac blocked the connection.** For `blockedByThisMac` (the backend's evidence rule and the ERR-XGS9X incident:
`src-tauri/src/network/DETAILS.md` § "This Mac refusing the route") the bridge also passes `blockedServer` (the event's
`displayName`), and the notice swaps its sentence for `directConnectionBlockedByThisMacToast` (names the server and the
OS-localized `systemStrings.localNetwork`), leads with an "Open {localNetwork} settings" button
(`openLocalNetworkSettings` in `#lib/tauri-commands`, the same opener Settings > Network uses), and keeps the retry
beside it as the secondary button: the switch works at once, so pressing the retry right after is the whole fix. On the
first launch after an update the event carries `afterUpdate`, and the bridge passes `blockedAfterUpdate`: the sentence
becomes `directConnectionBlockedAfterUpdateToast`, which names the update as the cause and drops the firewall aside
(#1019, backend side in `src-tauri/src/network/DETAILS.md` § "This Mac refusing the route"). "Connect directly" words
the same answer through `LocalNetworkBlockedToastContent.svelte` (sentence plus the settings button), raised by
`announceNoUpgrade` instead of a plain string toast, so the sign-in sheet's rounds get it too.

That is the macOS shape. The backend reads the same evidence on Linux and Windows, where no Local Network permission
exists and the cause left is a firewall, so `directConnectionBlockedByThisMacToast` is a platform block ("this PC" /
"this computer", "Check your firewall settings."), and both components render the settings button only under
`hasCapability('localNetworkPermission')`. Without it the notice's retry is the primary button and the "Connect
directly" toast carries only the report offer. `afterUpdate` never arrives there (the backend sets it only where the
permission exists), so `directConnectionBlockedAfterUpdateToast` and `openLocalNetworkSettings` stay macOS text.

**Dismissal watches the volume list, not the button.** A share can reach a direct session five ways: this notice's
button, the chip's yellow dot, the switcher's direct-connection switch or its "Connect directly now" fix, and the pane's
credential form after a working password. All of them end in `register_replacing_predecessor`, which broadcasts the
volume list, so the bridge dismisses on any `volumes-changed` carrying that volume as `direct`. One rule covers every
route, and a new route can't forget it. A share that goes AWAY broadcasts the list too, so a notice whose volume is no
longer listed retires the same way: its button could only say the share is gone. The bridge asks the toast store which
notices are up (`getToasts`, matched by content component and `props.volumeId`) rather than keeping a list, so there's
no frontend ledger to fall out of step with the backend's or with the user closing one.

**Switching the share's direct connection off withdraws it too.** The notice's button would then do exactly what the
user just opted out of. The volume list doesn't carry the per-share switch, so the backend owns this one: it emits
`smb-os-mount-notice-withdrawn { volumeId }` from `smb_direct_switch`, where the switch goes off whatever route flipped
it, and the bridge dismisses `smb-os-mount:<volumeId>`. Switching ON leaves the notice up: the connect it starts may
still fail, and the `direct` broadcast retires the notice when it doesn't. Backend side:
`src-tauri/src/network/DETAILS.md` § "Telling the user about a kernel-mount fallback".

❗ **Only a listing that finished may retire a notice by absence.** A `timedOut` payload is the last complete list
standing in for a fresh one, and a `discoveryPending` one carries the cached local part beside fresh server rows while
discovery is still out (`src-tauri/src/volumes/DETAILS.md`, "Server rows never wait on local discovery"), so a share
missing from either proves nothing. The rule's one gap: a discovery that started before a brand-new mount and finished
slowly (inside its timeout) after that mount's fallback could retire the fresh notice. The share keeps its yellow dot
either way.

**What the button does on a second failure.** It runs `connectDirectly`, which raises its own error toast naming the
typed reason, and the notice STAYS UP with the button live again: the situation it describes hasn't changed, and the
next press is worth making once the server wakes or the password is fixed. It retires itself on `connected` (the share
is fast now), on `askingForCredentials` (the form is a better surface for the same job, and two stacked prompts for one
share is noise), and on `gone` (the share unmounted before the press, so no retry can help). A press while an attempt is
in flight is ignored.

## Mount-phase auth failures

The share-mount controller (`../pane/share-mount.svelte.ts`) opens the sign-in sheet instead of dead-ending in its error
pane whenever `mountNetworkShare` rejects with a failure another credential can answer
(`smb-sign-in.ts::isMountSignInQuestion`): `auth_failed` (including the NetAuth -6600 code the backend maps),
`auth_required`, and `permission_denied`. The sheet opens carrying that refusal, pre-filled with the username the failed
attempt tried; what its `attempt` then runs is `../../servers/DETAILS.md` § "The sheet contract".

A share guests can list but not open reaches here as `auth_required`, not as the "not found" NetFS reports: the backend
asks the server itself which it was (`src-tauri/src/network/DETAILS.md` § "A share that says not found"). That is what
turned ERR-SHUSC's dead-end pane into a sign-in. A signed-in account the share refuses comes back `permission_denied`
and keeps the sheet open on `account_not_permitted`, so the user can try another account.

Two properties are load-bearing:

- **`mountError` is set for an auth failure too**, so the pane behind the sheet holds the failure and the MCP mirror
  reports it. Cancelling clears it through the controller's `back`, back to the share list.
- **`auth_required` gets a calm pane, ❌ never the red one.** Behind the sheet it reads "Sign in to open …" with a lock,
  Sign in, and Back, and no Try again (without an account it gets the same answer). `auth_failed` and
  `permission_denied` keep the red "Couldn't mount share" pane: there an answer the person gave was turned away. A share
  list's listing does the same for `auth_required` / `signing_required` ("Sign in to {host}", no Retry). Red is feedback
  on something the person did (cmdr-reports#10).
- **Only a credential refusal keeps the sheet open.** A retry that comes back `share_not_found`, `host_unreachable`, or
  `mount_missing` (the system reported the share connected and no mount of it is there:
  `src-tauri/src/network/DETAILS.md` § "A reported mount counts once it's there") answers `handed_off`, closing the
  sheet onto the pane's error state with its own "Try again" / "Back": the sheet has no words for a share that went
  missing. Non-auth failures never open it in the first place. Pinned by `../pane/share-mount.svelte.test.ts`.

**The pane's words are the frontend's.** The share-mount controller renders `renderMountError(error, host.name)`
(`mount-error-messages.ts`): one `errors.mount.*` key per `MountError` variant, in a record typed over the variants so a
new one can't ship without words. It names the host the way the pane shows it, since the backend only knows the address
it mounted by. `mountNetworkShare` throws the typed value as a `MountFailure` (`mount-error.ts`), and a caught value
that isn't one (the IPC call itself broke) reads as `unexpected`, its text going to the log. The MCP mirror carries the
same sentence plus the typed `reason`. Why the variants are cut the way they are: `src-tauri/src/network/DETAILS.md` §
"A mount refusal is data, and the frontend words it".

## A mount that waits

A NetFS mount can hang behind a stuck macOS sign-in helper for the whole mount timeout (#604), so the share-mount
controller renders the wait through `../pane/RemoteConnectView.svelte`, the ONE remote-wait view: a `connecting` state
named after the share, with the server's friendly name beside it ("testshare on Testnas": the hub's label, else the
host's name) and the `host:port` it dials under that, and that view's Cancel (`share.back`, Escape, cancels too: the
pane's key router hands the controller every key while its state is up). Each mount mints an attempt id
(`newServerAttemptId`) and passes it to `mountNetworkShare`; Cancel hands it to `cancelServerConnect`. After 10 s
(`MOUNT_SLOW_AFTER_MS`, the backend's `SUSPECT_AFTER`) the state's `hint` swaps the generic "This can take a few
seconds." for a line saying it's still waiting for macOS.

- **The wait keeps its own copy of what it names** (`mounting`: share, server, address), captured at mount start: a
  mount that went through clears `lastMountAttempt`, the host, and its label before it lands on the share, and the wait
  is still on screen then.
- **A host known only by its address names the share alone**: its name would repeat the `host:port` line right under it
  (`friendlyServerName`).

- **A cancel the person asked for goes back to the share list quietly**, ❌ never the error pane: `cancelled` with
  `mountCancelRequested` set returns as if nothing happened.
- **A cancel or timeout the backend reads as the wedge comes back `sign_in_helper_stuck`**, which the error pane words
  with the command that restarts the helper. Try again re-mounts; while the old NetFS call is still stuck in the same
  helper, the backend answers the same right away. How the backend decides: `src-tauri/src/network/DETAILS.md` § "A
  mount Cmdr gave up on, and a wedged sign-in helper".

## SMB live-reconnect flow (cross-component)

When a direct-SMB session drops mid-use, four pieces coordinate to recover:

1. **Backend** (`SmbVolume::handle_smb_result` in `crates/cmdr-smb/src/volume/session.rs`) detects `ConnectionLost` /
   `SessionExpired`, flips to `Disconnected`, and emits `volume-connection-changed { volumeId, state: "disconnected" }`.
   The event is backend-neutral (any connecting backend emits it); everything below is what the frontend does with it,
   and a new backend inherits all of it. (See `crates/cmdr-smb/DETAILS.md` § SMB live-reconnect lifecycle.)
2. **`stores/volume-store.svelte.ts`** patches the matching volume's `connectionState`, keeping the picker dot, the
   breadcrumb, and `currentVolumeInfo` reactive without waiting for the next `volumes-changed`.
3. **`smb-reconnect-manager.svelte.ts`** (if any subscribers are present) starts a per-volume backoff cycle calling
   `reconnectVolume(volumeId)` per tick. Resolves when the BE emits a follow-up `state: "connected"` event. ❗ The first
   attempt waits for nothing and runs QUIET: `getState` answers `null` for it until `QUIET_ATTEMPT_GRACE_MS` (1 s)
   passes, so a drop the first dial fixes (most are; it takes about 0.2 s) shows no view at all, and the pane just
   reloads. One that fails moves to the next attempt, which shows at once; "Retry now" always shows. Why: a countdown
   bar before the first dial made a healthy NAS look broken on every return to the app. Only its per-tick command is
   SMB-specific; the cycle itself is backend-agnostic, so it keeps the SMB name until a second backend needs it and the
   command generalizes. A tick that doesn't take arrives as a typed `ReconnectError` (`volumeNotFound`, or `volume`
   wrapping the whole `VolumeError`), thrown as a `ReconnectFailure` by `reconnect-error.ts` and read back with
   `asReconnectError`. The manager LOGS it through `describeReconnectRefusal`, which names the reason
   (`volume/needsPassword`) rather than a sentence a copy edit could change under it. None of this is user-facing: the
   reconnecting view, the gave-up banner, and the sign-in form each carry their own translated copy. The error-path map
   is `docs/guides/error-handling.md`. ❗ A pane hears a transition because the entry's `state` and `signIn` are
   `$state.raw` fields that each transition REASSIGNS, ❌ never through `map.set` of the same entry: `SvelteMap.set`
   stays silent on an identical value, and that froze the view on its first read (`ERR-VZ4CV`: Retry now disabled over a
   cycle that had long given up, Cancel and Disconnect leaving the view up). A Cancel bumps the entry's `cycle`, so an
   attempt in flight when it lands drops its answer rather than scheduling the next tick. Besides a pane, the backend
   can start a cycle: `volume-reconnect-requested` (Ask Cmdr's `connect_volume` on a dropped volume) calls the same
   `startCycle` a pane landing on it does, so recovery keeps this one owner.
4. **`FilePane.svelte`** subscribes via `$effect` whenever the pane is on an SMB volume. Subscription is refcounted
   (both panes on one share share a cycle), and on success the `onSuccess` callback re-runs `loadDirectory`.
   `../pane/smb-view-state.svelte.ts` turns the manager's status into the ONE `RemoteConnectState` the pane renders, in
   a single exhaustive `switch`: `waiting` / `attempting` become `connecting` carrying the cycle's own lines, its
   countdown to the next attempt, and its Try now / Disconnect; `needs-auth` becomes `signed_out`; `needs-host-key`
   becomes `host_key_changed`. ❗ `gave-up` maps to `null` on purpose, because `VolumeUnreachableBanner`'s `gaveUp`
   variant is the app's one "couldn't reach this" surface, and two renderers for one state is worse than one in the file
   next door. The exhaustive switch replaced a list of per-status booleans, where a new status silently rendered a plain
   listing over a dead session.

Auth-failure give-up → "Sign in", not "unreachable" (`needs-auth` status): when reconnect fails on an auth error the
saved password can't fix, the backend emits `state: "needs_credentials"`. The manager's `handleNeedsAuth` stops the
backoff (retrying a stale password is futile) and flips to `needs-auth`; FilePane shows `RemoteConnectView`'s
`signed_out`, whose button opens the one sign-in sheet as a REGISTERED place, ❗ or offers no button at all when the
stored shape is `nothing`, since no secret a person could type would bring that session back. The sheet's attempt calls
`reconnectVolumeWithCredentials(volumeId, …)`, which refreshes the stored password and reconnects; success arrives as a
`connected` event that clears the state and reloads. Pinned by `smb-reconnect-manager.svelte.test.ts`.

❗ **`handleNeedsAuth` also asks `getVolumeSignInState(volumeId)` and stores the answer on the entry**
(`getSignInShape(volumeId)` reads it back). It is asked HERE, at the flip, and ❌ never carried over from earlier: the
credential a remote volume comes back on is decided per dial, so an answer kept from the connect that opened it
describes a session that has since ended. On SFTP that is the difference between a volume the user can sign in to and
one with no way in at all; the reasoning and the per-rung table are `crates/cmdr-sftp/DETAILS.md` § "What the banner
shows, per rung". The flip itself stays synchronous with the event (the `await` comes after), so `runAttempt`'s
in-flight `needs-auth` check is unaffected.

The sheet reads the shape when it renders rather than taking this stored one, for the same reason the flip re-asks: it
describes THIS session. The stored value is what the pane's banner has to hand before the sheet opens.

❗ `needs_host_key_approval` is the fourth `volume-connection-changed` state, and it gets its OWN status
(`needs-host-key`), ❌ never the sign-in path: it only ever describes an SFTP volume whose host key stopped matching,
and a password box in front of a possible man-in-the-middle is how a password gets typed into one. `handleNeedsHostKey`
ends the backoff and flips; ❗ it asks no `getVolumeSignInState`, because nothing about this is a credential question
and asking one would be the first step toward putting a password box in front of it. The pane renders
`RemoteConnectView`'s `host_key_changed`, which offers "Check the key" and Disconnect (`../pane/DETAILS.md` § the
connect views says why, and why never "Trust it"). `crates/cmdr-sftp/DETAILS.md` § "Connecting from the frontend".

❗ `guest_session_refused` is the fifth state, SMB only: a reconnect signed in with a named login and the server
answered with a guest session, so the backend refused it (usually a wrong password, rarely tampering). It gets its OWN
status (`guest-refused`) rather than `needs-auth`, because the pane names what Cmdr refused. `handleGuestRefused` ends
the backoff like `handleNeedsHostKey` (the same login gets the same guest offer), and the pane renders
`RemoteConnectView`'s `guest_session_refused`: "Try another password…" is the signed-out banner's own Sign in (arm 2),
and Disconnect the cycle's. The volume store shows it as `needs_sign_in`.

❗ `encryption_required` is the sixth state, SMB only: "Always require SMB encryption" is on and the rebuilt session
can't encrypt (a guest session, or a server with no cipher), so the backend refused it (#966). Its own status
(`encryption-required`), set by the event OR by the command's typed refusal (`volume` / `encryptionRequired`), so the
cycle stops even if the event lands after the failed attempt returns. `STOPPED_STATUSES` is the one list of statuses the
backoff stops at. The pane renders `RemoteConnectView`'s `refused` with the sentence, "Open Settings" (deep-linked to
the switch through `smb-encryption-setting.ts`), and Disconnect, ❌ no retry: the server answers the same until it or
the setting changes. The volume store shows it as `disconnected`.

`negotiation_invalid` and `encryption_unavailable` (#931) stop the cycle the same way, each with its own status
(`negotiation-invalid`, `encryption-unavailable`) set by the event or the typed refusal
(`SESSION_REFUSAL_FOR_VOLUME_ERROR`): the server didn't confirm the negotiation, or the share only accepts encryption
the session couldn't agree on. The pane renders `refused` with a sentence worded like the share list's, and Disconnect
only: no retry, and no Cmdr setting to point at.

Lazy-nav path: opening a share that's already `Disconnected` (no fresh event in flight), the `smb-view-state.svelte.ts`
subscription `$effect` notices `currentVolumeInfo?.connectionState === 'disconnected'` and calls
`connectPlace({ volumeId, connectionState: 'disconnected' })`. ❗ Through `#lib/servers/connect-flow.ts`'s arm 1 rather
than `manager.startCycle` directly, because that module documents itself as the ONE caller of the manager's lazy start
and a second caller makes the guardrail a lie. Arm 1 is that call and nothing else, and the manager is idempotent, so
landing on the same share twice still costs nothing. The signed-out banner's Sign in goes through the same flow's arm 2,
which is what carries the `needs_credentials` reason into the sheet's first round.

Disconnect button: `disconnectSmbVolume(volumeId)` shells out to `diskutil unmount` (macOS) → FSEvents fires →
`SmbVolume::on_unmount` → volume removed from `VolumeManager` → `volumes-changed` removes it from the picker.

## Lazy mDNS trigger, and the Servers view's hold

`triggerNetworkDiscovery()`:

1. No-ops if `network.enabled === false`.
2. Calls `noteNetworkAction()` (backend: reload the manual servers, run the existing-SMB-mount upgrade pass, which holds
   the browse while it resolves).
3. Sets `network.firstTriggerDone = true` so later launches warm the server list up briefly (returning users see known
   servers at once, without re-prompts).

Call sites: `ServersHub.onMount` and `VolumeBreadcrumb.handleSubmenuAction` (the OS-mount → direct-smb2 upgrade also
opens a private-IP socket).

The browse itself runs while a Servers view is on screen: `NetworkMountView` holds it through
`holdDiscoveryForServersView()` (an `$effect` whose cleanup releases it), and the store tells the backend only on the
first view shown and the last one gone (`setServersViewShown`). Two panes can each show one. `initNetworkDiscovery`
re-sends the store's count, so a reloaded page corrects what the one before it left. Between browses the backend keeps
the hosts it found, so a view opens on them at once and the fresh browse adds and drops hosts live. Backend side:
`src-tauri/src/network/DETAILS.md` § "Discovery runs only while something needs it".

## Key decisions

- **Lazy discovery on first user intent, not at startup**: avoids the macOS Local Network prompt on fresh installs
  before the user has context; `network.firstTriggerDone` persists so returning users get the launch warm-up.
- **Resolution and share prefetch are fire-and-forget**: hosts come and go, so a timeout / unreachable during prefetch
  is normal, not worth surfacing. The UI shows "Not checked" / "Waiting..." until data arrives; only user-initiated
  actions surface errors.
- **Prefetch is for saved servers only, and only once a Servers view opens** (#324's option (b), David's call): the
  first open of the Servers view may show a saved server's list loading, and the first open of a host Cmdr only found
  waits for its listing. The trade is those waits against a file manager that connects to SMB machines at each launch
  whether or not anyone looks.
- **State via getters, not raw `$state` exports**: raw exports lose reactivity when imported from a plain `.ts`; getters
  work everywhere and make the API boundary explicit.
- **A reconnect is silent: no toast fires for one.** The recovery already shows itself where the user is looking, in the
  pane (the reconnect cycle's countdown, then the give-up banner) and on the volume picker's dot, and every laptop
  lid-close drops the session, so a toast per reconnect would fire on routine sleep/wake and teach the user to dismiss
  it unread. The evidence stays available on demand instead: the debug window's SMB diagnostics dashboard reads the
  session counters through `src-tauri/src/commands/smb_diagnostics.rs`.

## Gotchas

- **Which server a pane shows is its path alone** (`smb://<server>`): no component holds a host copy beside it, so the
  stale-host bug a second copy once caused (leaving Servers and coming back re-mounted with an old host, E2E "unicode
  shares render") can't happen.
- **`network` volume ID is virtual**: the discovery UI has no real mount point until a share is mounted via
  `mount_smbfs`. Mounted shares then appear as separate `VolumeInfo` entries with real IDs.
- **Credential status keyed by lowercase `host.name`**: the same physical host can change IP (DHCP) and hostname (mDNS
  vs DNS); the Bonjour service name is the stable identifier. Lowercasing avoids case mismatches.
- **⌘R in `ServersHub` calls `stopPropagation()` too, and one round of shares depends on it.** The document-level
  dispatcher runs after this handler and has no `defaultPrevented` guard, so `pane.refresh` would ALSO dispatch into
  `refreshPane` → `refreshNetworkHosts()` → `ServersHub.refresh()`, the same `handleRefreshClick()` the local branch
  just ran, giving every host two `clearShareState` + `fetchShares` rounds per keypress. Pinned by `ServersHub.test.ts`;
  the general rule is in `#lib/shortcuts/DETAILS.md` § "Local handlers resolve through the registry too".
- **The hub's `handleKeyDown` returns no "handled" boolean** (`BrowserAPI` in `../pane/types.ts`). Nothing above it
  branches on one: `NetworkMountView` and `pane-key-router` hand the hub every key and return either way, so the claim
  is `preventDefault()` + `stopPropagation()` on the event.
- **The hub's MCP sync encodes metadata into the `name` field** as a flat string because MCP `PaneFileEntry` has only
  `name` / `path` / `isDirectory`; the encoding lets agents read what the UI shows without a schema change.
  `servers-hub-mcp.ts` owns it, and its status token stays untranslated on purpose.
- **The hub row's identity is the id `network`; its name is display.** The pane pushes the catalog word
  (`fileExplorer.navigation.networkVolume`, `Servery` in Czech), while agents read Rust's
  `volume_listing::SERVERS_VOLUME_NAME` (`Servers`) in `cmdr://volumes` in every language.
  `../pane/volume-selection.ts::selectVolumeByName` takes both, because the row is synthetic and no `findIndex` over the
  volume list reaches it. ❌ Never tell where a pane is by this name: `tools/actions/nav.rs` did, and every
  `select_volume` on the hub failed outside English.

## Dependencies

- `#lib/tauri-commands`: `listNetworkHosts`, `resolveNetworkHost`, `listSharesOnHost`, `listSharesWithCredentials`,
  `prefetchShares`, `saveSmbCredentials`, `deleteSmbCredentials`, `getUsernameHint`, `getKnownShareByName`,
  `updateKnownShare`, `updateLeftPaneState`, `updateRightPaneState`, `connectToServer`
- `#lib/settings/network-settings`: `getNetworkTimeoutMs`, `getShareCacheTtlMs`
- `#lib/utils/confirm-dialog`: `confirmDialog`
- `#lib/ui/toast`: `addToast`
- `../navigation/keyboard-shortcuts`: `handleNavigationShortcut`
- `../types`: `NetworkHost`, `DiscoveryState`, `ShareInfo`, `ShareListResult`, `ShareListError`, `AuthMode`
