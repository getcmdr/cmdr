# Network browser details

Depth and rationale. `CLAUDE.md` holds the must-knows; the architecture, flows, and full decision detail are here.

## `network-store.svelte.ts`

Module-level `$state`, consumed via exported getter functions (never import raw state).

Key state:

- `hosts: NetworkHost[]`: discovered hosts, sorted alphabetically by getters
- `discoveryState: DiscoveryState`: `'idle' | 'searching'`
- `resolvingHosts: SvelteSet<string>`: host IDs currently being resolved
- `listedAccounts`, `guestListingHosts`: what this session's share listings said about each host (the account items)
- `credentialStatuses: SvelteMap<string, CredentialStatus>`: `'unknown' | 'has_creds' | 'no_creds' | 'failed'`

Lifecycle:

- `initNetworkDiscovery()`: call once at app startup. Idempotent. Subscribes to Tauri events (`network-host-found`,
  `network-host-lost`, `network-host-resolved`, `network-discovery-state-changed`), and resolves each host with no
  hostname yet (`startResolution`, fire-and-forget).
- `cleanupNetworkDiscovery()`: unlisten all events, reset `initialized`.
- `holdDiscoveryForServersView()`: the mDNS browse's lease while a pane shows the Servers list (§ "Lazy mDNS trigger").

❗ **Nothing here lists a server's shares unasked** (#324). Listing a host's shares means connecting to it and signing
in, as a guest where it lets one in. The Servers list never dials (Rust builds it from memory, the stores, and the mount
table: `src-tauri/src/network/DETAILS.md`), and a server's shares are read only when a pane opens its share list. There
is no prefetch: getcmdr/cmdr#1517 stopped it (§ "The Servers list's decisions"), because the reads below stay under the
pane's wait reveal for reachable saved servers, and the one slow server is slow because of an `smbutil` read through a
session macOS has open, which no background path may make. The backend has no other caller that lists unasked:
`smb_client::list_shares` is reached through the two listing commands, both frontend-invoked by a person's door, and the
share-list volume's sign-in ladder (`share_list_sign_in.rs`), which runs only for a pane that opened a share list
(verified by reading its call sites, 2026-11-26). E2E builds count every call per address
(`apps/desktop/src-tauri/src/network/share_listing_tally.rs`), which `servers-list-flows.spec.ts` reads: opening Servers
asks no server at all. Pinned too by `network-store.servers-view.test.ts`.

**How long a share list's first read takes** (the wait a prefetch would have hidden; measured 2026-11-26 for
getcmdr/cmdr#1517: `stall_probe::listing listing_done path=smb://<server>/ read_dir_ms`, a `read_dir_ms` of 0 or 1
excluded as a share-cache answer). The threshold is the pane's wait reveal, 800 ms (`WAIT_REVEAL_AFTER_MS`):

- Real servers, a dev log (macOS, a home LAN plus Tailscale): a Raspberry Pi saved by name (signs in with Cmdr's stored
  password) n=13, p50 88, p90 643, max 1,174 ms; a QNAP NAS saved by LAN IP n=3: 83, 87, 517; the same Pi by its IP (a
  saved share; guest refused and no Cmdr credentials, so every read runs `smbutil view` on the open session Finder held,
  about 1.15 s of it) n=5: 88, 1,183, 1,189, 1,418, 1,422; the NAS by its mDNS name (found) 80, 425, 450; the NAS by its
  Tailscale IP (mounted only) 452, 495, 554. Without the Pi by IP: n=16, p90 643.
- Docker fixtures (Linux E2E lane, `servers-list-flows.spec.ts`): Guest 307, Guest by IP 517 and 586, Unicode 739 and
  740, 50 Shares 844 and 1,048 ms (n=7, p90 1,048).

Key exported functions: `getNetworkHosts()` (sorted copy), `getResolvedShareListHost(path)` /
`noteResolvedShareListHost` (the host the backend resolved a share list to, for a server discovery doesn't list),
`forgetCredentials(serverName)`, `setCredentialStatus` / `getCredentialStatus` (in-memory only), `getDiscoveryState()`,
`getListedAccount(hostId)`, `guestListingWorked(hostId)`.

## Acting on a Servers row

The Servers list (`smb://`) is a listing; § "The Servers list as a listing" is how it's shown and routed. This is what
an action on one of its rows does.

❗ **Every row action carries the row's own id, end to end** (`server-row-of-entry.ts`). The row is read off the cursor
entry's `ServerMeta` (`ExplorerAPI.getFocusedPaneServerRow`), the saved entry found by `ServerMeta.rowId` in a fresh
`listSavedServers()`, and the backend's `forgetSavedSmbHost` / `updateSavedSmbHost` take that id alone. ❌ Never resolve
"which row" from a host id, name, or address at action time: two rows can stand on one discovered host, and a lookup by
host picked the first, so "Edit server…" on the second row saved into the first (QA 2026-09-25).

❗ **An SMB row's host is the one its share list resolves to, by the row's path.** Rust writes the row's path as the
share-list path of the host the row opens (`share_list_path.rs`, the same bytes as the frontend's writer), so
`share-list-label.ts::shareListServerOf(row.path)` finds discovery's host under that path, the one the share list itself
signs in to and caches under; with none, the host the path names stands in (its key as the hostname, its port). So a
server saved by IP and shown under its Bonjour twin's name signs in, notes its account, and reads its credential status
as the share list at the saved address does, ❌ never as the twin or as a host parsed out of the Details text.

- **F8 and ⇧F8** (`file.delete` and `file.deletePermanently` on the `network` kind, since a Forget is for good anyway;
  an agent's `delete` keeps its own road and refusal) forget the SAVED server under the cursor
  (`servers-list-actions.ts`): a one-place server through `forgetSavedServer` (so it asks exactly what the switcher's
  menu asks), an SMB server through its manual entry, sign-in history, saved shares, and who its share lists signed in
  as (Rust's and `forgetShareListsOfMachine`'s copies), an S3 account as every place under it plus, box checked, the
  secret they share (written FIRST, while a place still names the entry), and a host only mDNS knows about gets the
  "Can't remove discovered hosts" toast. ❗ An SMB server with a share still mounted offers "Disconnect and forget":
  Cmdr saves a mounted share's server right back (`src-tauri/src/network/DETAILS.md` § "Servers behind mounted shares"),
  and a share that won't unmount keeps the host. Nothing is unmounted unasked. The password box shows only where one may
  be stored, known without a Keychain read.
- **F2 / ⌘E / the palette's Edit server** open the sheet on the row's saved entry (`editServerRow`); on a share list, on
  the saved server whose Servers row opens it (Rust's `saved_server_of_share_list`, the list's own match). A row with
  nothing saved says why, ❌ never nothing.
- **The row menu** (right-click, `⌃⏎`) is `ServersListRowMenu.svelte`, beside the path in the chip's ONE `openMenu`:
  `row-menu.ts::smbHostRowMenu` for an SMB server (Open, Edit… for a saved one, Disconnect while a share from it is
  mounted, the account items, Forget server), `volumeRowMenu` for a one-place server (the switcher row's → submenu). ❗
  Opening it reads no password: the one fact it asks is the kernel's mount table (`smb_host_has_mounts`), and the
  account items come from what this session's listings said. An account action that lists opens the server's share list,
  as activating the row does.
- **Disconnect** on an SMB server unmounts its shares (`disconnectSmbHost`), ❌ never a session the way a place's does.

**Decision**: the Servers list is a pane state, not a Settings list, and the rule that comes with it: **a pane state may
be a LIST, ❌ never a FORM.** Enter, F8, ⌘E, and the row menus act on the row under the cursor; anything that asks a
person to TYPE opens the sign-in sheet (`../../servers/DETAILS.md` § "The four rules", rule 3). A form inside a pane is
what the SMB login form used to be, where Tab meant "switch panes" to Cmdr and "next field" to macOS.

## A server's share list

A server's shares are a real listing: the `network` volume at `smb://<server>` (`#lib/servers/share-list-path.ts` writes
and reads the path; the backend's `SmbServerVolume` lists it, `src-tauri/src/network/share_list_volume.rs`), and the
standard views render it with the kind's own words (`../pane/list-kind-words.ts`: the Description column, "8 shares",
"Back to Servers"). An SMB server's Servers row opens one by being its path (a plain folder open); ❌ no component here
holds a host. What a share list does that a folder doesn't is `../pane/share-list-pane.svelte.ts` (pane `DETAILS.md` §
"A server's share list"): Enter mounts a share, a door's arrival lands, a sign-in refusal asks.

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
that opens the sheet over an in-pane sign-in state leaves the pane on it; doors with none (a Servers row's menu, the
path bar's account, the palette) stay where they were.

**The password macOS saved** (Finder's "Remember this password in my keychain"). When a listing the person started comes
back `auth_required` (no password of Cmdr's own was offered), `askToSignInToList` first tries the one macOS saved, on
its own, before the sheet asks: `listSharesWithSystemSavedPassword`, whose read raises the macOS consent dialog. That's
why it sits behind the same gate as the sheet: only a person's action reaches it (the auto-ask, the error screen's Sign
in), ❌ never a pane repaint, a restored tab, or a re-list nobody started. The backend asks at most once per server per
run, a denial included, and a listing that went through becomes the server's session credential, so the re-list signs in
with it. ❌ Not tried on `auth_failed`: that's Cmdr's own password turned down, and the person should see so. Every
sheet round also asks `systemSavedSmbAccount` (attributes only, no prompt): the account pre-fills the username when
nothing Cmdr remembers names one, and while the password may still be tried this run (`canTry`), the sheet offers "Use
the password macOS saved" (the sign-in request's `alternative`, `../../servers/DETAILS.md` § "The sheet contract").
Backend half: `src-tauri/src/network/DETAILS.md` § "The password macOS saved".

`share-list-sign-in.ts`, the sheet round: every `attempt` is `listWithAccount`, which answers `handed_off` on success
(nothing here connects a VOLUME, and the loaded list IS the result), keeps the sheet open on `authentication_rejected` /
`needs_credentials`, and closes it onto the pane's own error state for anything else, because that is where the retry
and a missing dependency's install command live. The round answers the attempt that closed it (`listed` or `failed`), or
`cancelled`; ❗ a `listed` carries no password back (the backend keeps the typed account as the server's session
credential, which the re-list and a mount sign in with). A listing that worked leaves every record behind in one place:
the share cache, the credential status, the listed account, the account preference (the choice sticks), the Keychain
when Remember, and the known-shares store. "Browse as guest" (`useGuest`) clears the account preference FIRST, since the
backend never lists a server with an account as guest. ❗ One ask at a time is the MODULE's guard (`sheetRound`): the
error screen, the auto-ask, a Servers row's menu, the path bar's account menu, and the palette all open the sheet, and a
second sheet would close the first as `cancelled`, read as "the person said no" from under a sheet that's still up. A
round asked for while one is up answers `null` and asks nothing.

**The account doors** (Sign in as…, Browse as guest, Forget saved password) are one action vocabulary
(`../navigation/row-menu.ts`'s `ServerAccountAction`, built by ONE `serverAccountEntries(facts)`) with one runner,
`runServerAccountAction`, behind every door: an SMB server's Servers row menu, the path bar's account menu on a share
list (`ShareListAccountChip.svelte`, a member of the chip's ONE `openMenu`), the share-row menu (`shareRowMenu`, hosted
by the same chip, `openRowMenu`), and the palette (`servers.signInAs`, `servers.browseAsGuest`, `servers.forgetSecret`).
The facts are `serverAccountFacts(host)`: the listed account and whether a guest listing worked, both this session's own
answers, ❌ never a Keychain read. Browse as guest shows only while signed in as a user on a server a guest listing
worked on, and the runner refuses it otherwise (a rebound palette key gets a sentence), because it drops the account
FIRST. Forget saved password shows unconditionally, in every door: a delete raises no prompt, and the store's typed
`not_found` words "there was none". A door that isn't on the share list opens it once an action listed (the Servers row:
as activating it; the palette from a Servers row: `openSmbHandOffInFocusedPane`); a door on the share list re-lists it.

**Who it signed in as, and what it holds**: a share list's `listing-complete` carries `shareListAccount`, noted into the
store by `share-list-label.ts::noteShareListAccount`, so the path bar's account and the Servers row say the same thing
(Rust reads the same account for the row, `SmbServerVolume::listed_as`). A guest listing's account also marks the host
as one guest works on (`guestListingWorked`), so the account items offer "Browse as guest". ❌ The store lists no shares
itself: everything it knows came off a pane listing or a sheet's.

**A server no discovered host stands for** (one saved without contact, `awaitingFirstConnect`, which discovery never
lists) still has a host to sign in to and act on: `share-list-label.ts::resolveShareListServer` asks the backend for the
host its own listing resolved (`share_list_host`, memory and the stores only), and notes it in the store, so the sign-in
ask, the path bar's account, and the share rows' menus all reach it by the id the share cache and the session credential
are keyed by. ❌ Never a host made up from the path: a saved address keeps its case in its id, and the path's key
doesn't. A landed listing resolves it (`noteShareListListed`), and a refused one's ask does (`askToSignInToList`).
Pinned by `the_share_list_host_is_the_one_its_listing_resolves` and the E2E "a server saved without contact asks for an
account".

The error state's sentence comes from `share-list-error-messages.ts`: one `errors.shareList.*` key per `ShareListError`
type, naming the host. ❌ Never `error.message`: the backend documents it as diagnostic detail for the log, and it's
English, often a fallback tool's own stderr. `listSharesWithCredentials` throws the typed value as a `ShareListFailure`
(`share-list-error.ts`), and every catch reads the value back through `shareListErrorOf`. ❌ Never
`e as ShareListError`: anything untyped that lands in the catch (a runtime exception, an IPC call that broke) had no
`type`, and the renderer threw on it; it reads as `protocol_error` now, its text kept for the log. Pinned by
`share-list-error-messages.test.ts`.

**A saved share on its share list** (§ "The Servers list's decisions", its first bullet): its row carries
`ShareMeta.saved` (its place's volume id, status, account, and pin; Rust reads them, `src-tauri/src/network/DETAILS.md`
§ "A server's share list is a listing", and re-lists an open share list from the cache when they change). The row wears
the status dot (`server-row-words.ts::rowStatusBadge`, drawn by `FileIcon`), Enter on it lands on its PLACE when the
volume list holds one (`../pane/network-row-open.ts`), so it opens as its saved account; a saved share with no place in
the volume list (unpinned, unmounted) mounts from the share list like any share. Its menu (`shareRowMenu(facts, saved)`,
hosted by `ShareListAccountChip`) adds Pin to switcher / Unpin and Forget share, run on the place (`runServerRowAction`,
`servers-list-actions.ts::forgetSavedShare`). ❌ No Keychain read for any of it.

❗ **A queued share is a one-shot for the visit it was queued with.** The pane's arrival
(`../pane/share-list-arrival.svelte.ts`) holds it per pane, spends it once the list lands (and toasts "not found" when
the list lacks it), and drops it the moment the pane is anywhere but where it was queued from or for. Opening a server
from the Servers list only ever lists its shares. The bugs this closes: cmdr-reports#7 ("Cmdr tried to open the share on
that server"), where the value outlived its visit and fired again each time the list came back, so a host row the person
meant to browse mounted a share they never picked. Pinned by `../pane/share-list-arrival.svelte.test.ts`,
`../pane/share-list-pane.svelte.test.ts`, and the SMB E2E.

## The Servers list as a listing

`smb://` lists one folder per server (`src-tauri/src/network/servers_listing.rs`, live through `servers_relist.rs`), and
the standard views render it like any folder (`capabilitiesForPane` gives it the `network` kind, a listing kind that
mirrors to MCP). ❌ No view holds an `if server`; every Servers fact reaches the views through a generic seam:

- **Words**: `server-row-words.ts` words Rust's `ServerMeta` (the Details cell, "SMB · 192.168.1.111 · as testuser",
  through `extCellText` / `rowDescription`; the status words; the dot). The kind's words (`../pane/list-kind-words.ts`):
  Details (prose), Last used (`ListWords.dateHeader`, the row's last use as its date), no Size, "5 servers" plus the
  nearby search's state (where the old "searching" and "discovery is off" lines went), and an empty list naming F7. The
  F-key bar's F7 "Add server" and F8 "Forget" come from the same file's `functionKeyWordsFor`.
- **Icons and the dot**: the reserved ids `network:server-smb` / `network:server` are the app's own glyphs
  (`#lib/glyph-icons.ts`, never asked of the OS); the status dot is `views/file-list-utils.ts::rowBadgeFor`, decorative,
  its words in the row's accessible name (`file-row-a11y.ts`, said even with no dot: "Found nearby").
- **Sort**: the pane's own sort, Name by default. The date is a LAST USE (`SupportedDates::modified_is_last_use`, which
  the volume answers for `smb://`), so a Last used sort ranks the never-used last in both orders and isn't held in name
  order by "Folders always by name" (every row is a folder). The rule is the engine's, `EffectiveSort::resolve`.
- **Opening a row** (Enter, double-click, MCP `open_under_cursor`): an SMB server's row IS its share list's path, so it
  opens as a folder; a one-place server or an S3 account goes off the volume first (`../pane/network-row-open.ts`, from
  the share-list pane's open seam, BEFORE the generic folder open, or a place row would navigate the `network` volume to
  `sftp://…`). Get info, Tell me about this, Show in Finder, and Quick Look say a server is no file; a drag out is
  refused by the transfer source guard (the `network` volume is no source); no row is a drop target.
- **Acting on a row**: § "Acting on a Servers row".
- **MCP**: the standard mirror; `../pane/mcp-row-name.ts` adds `protocol=` / `status=` / `address=` / `account=` to a
  row's name.
- **"Add server…"**: F7 and ⌘K open the sheet; the list also ends in a trailing action row
  (`../pane/list-kind-words.ts::trailingRowFor`, running `servers.connect`), drawn by the views' generic trailing row
  (`../views/DETAILS.md` § "The trailing action row"): last under any sort and filter, never selected or counted.
  `cmdr://state` lists it last as `+ Add server…  kind=action command=servers.connect`. A server the sheet saved without
  opening ("Add") is selected once the re-list lists it, by its row id (`../pane/servers-list-pane.svelte.ts`); an SMB
  address hands off to its share list in the focused pane.
- **The discovery hold**: a pane on `smb://` holds the mDNS browse while it's there
  (`../pane/servers-list-pane.svelte.ts`, keyed on the pane's location, ❌ never a component mount).
- **Copy path to the other pane**: the cursor on an SMB server opens its share list there (the row's path); any other
  row the Servers list (`../pane/pane-mirror.ts`).
- **Not routed to Ask Cmdr**: `location/facts.rs` lists no volume for `smb://` (`BackendKind::ShareList`), so the
  agent's `list_dir` doesn't reach the Servers rows; an agent reads them from `cmdr://state`. A share list's path would
  otherwise be a live read that dials (#324).

### The Servers list's decisions

The Servers view was a hand-built table (its own header, cursor, keys, type-to-jump, footer, MCP push, and row menu
host) that nested each connected server's shares under it and folded found-only hosts into a group. David's QA
(2026-11-26) called it "too much magic": the two levels behaved differently, `..` from a nested share landed on the
share list, and servers known only by an OS mount didn't show. getcmdr/cmdr#1517 made it this flat listing, with these
decisions (David, 2026-11-26):

- **Flat, one row per server, the standard views.** ❌ No share rows: a server's shares live on its share list, which
  carries every share and a saved share's facts (§ "A server's share list": the dot, Pin, Forget share, Enter mounting
  as the saved account).
- **Columns: Name, one Details column, Last used.** The status is a dot on the icon, its words in Details and the row's
  accessible name; no Size. Rejected: real Type / Address / Status columns (column machinery in both views for one pane
  kind), and Type in the Ext slot with Status in the Size slot.
- **"+ Add server…" has two homes**: F7 (the F-key bar reads "Add server"), ⌘K, the palette, and the empty list's words;
  AND a trailing action row through the views' one generic per-kind seam. Rejected: a Rust-listed entry (sort and filter
  would move or hide it, and pinning it last would need a special case in the engine's one comparator and filter point),
  a footer button, and the palette alone.
- **A server Cmdr only found nearby is an ordinary row** reading "Found nearby". Rejected: hidden-files treatment (a new
  user with nothing saved would see an empty list), and not listing it.
- **A server behind a mounted share is saved** the first time Cmdr sees the mount (Finder's, `mount_smbfs`'s, or
  Cmdr's), so it stays after the unmount; Forget offers Disconnect first while a share is still mounted. Only the
  SERVER, ❌ never its shares (`src-tauri/src/network/DETAILS.md` § "Servers behind mounted shares").
- **One NAS at two addresses (LAN and Tailscale) is two rows**, and Edit server changes a saved server's address. ❌
  Never merge two servers by GUID: Samba derives its server GUID from the NetBIOS name (#1518), so a match proves one
  name, not one machine. One server holding several addresses was rejected for now (mounts, saved shares, and passwords
  are all keyed per address); revisit only if two rows keep bothering David. A server saved by IP DOES merge with the
  discovered host at exactly that address and port (plain equality, David, after the mounted-servers work).
- **An S3 account is one row**; Enter opens its "All buckets" root, saving it if it isn't yet (Enter is the person
  asking for it). Saved buckets stay reachable through their switcher pins. Rejected: a row per bucket, and an account
  row that opens a second kind of "server's places" list.
- **No prefetch** (§ "`network-store.svelte.ts`" has the numbers that decided it).
- **The count line says the discovery state** ("5 servers, looking for more nearby…" / "…, networking is off", naming
  the "Enable networking" switch), and the Settings switch is where discovery goes back on. Rejected: a clickable line,
  a non-row element in the views.
- **Order is the pane's sort** (Name by default; Last used for recency). The old fixed order (connected, then asking,
  then by use, nearby last) went.
- **⌘R on a share list that lists shows nothing extra** when nothing changed, like a folder; ⌘R on its refused or
  sign-in state retries.
- **The merge lives in Rust, the words here**: Rust ships facts (`ServerMeta`), `server-row-words.ts` words them.
  Rejected: the merge in TS pushing rows to Rust (two sources of truth and a startup race), and a frontend in-memory
  listing (the search snapshot's six coupling points).

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
counts it like any list's. Enter, `⌘↑`, and Backspace on it, or on the error screen where there's no row, load the
Servers list (the listing says so, `../pane/DETAILS.md` § "`..` out of a server's top folder") with the cursor on that
server's row, found BY PATH (`LoadDirectoryArgs.selectPath`, Rust's `find_path_index`): the row reads the server's
friendly name, its path is the share list's. A path no row has (a Bonjour twin's share list, whose server is saved by IP
and listed under the IP's path) lands on the row that stands for the host it resolves to, by identity
(`src-tauri/src/network/DETAILS.md` § "The Servers list's rows"). A restored share-list tab comes back as the Servers
list on that row the same way (`../pane/restored-landing.svelte.ts`), with nothing listed at launch. History Back
restores the cursor like any folder's. Escape doesn't: on a file list it never means "parent". It names the Servers list
in the MCP mirror (`smb://`) and says "Back to Servers" to a screen reader (`fileExplorer.network.share.parentRowName`,
through `ListWords.parentRowName`).

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
- **The pane side is `../pane/top-folder-parent-pane.svelte.ts`**, every server kind's one seam: it adds the `..` row
  through `has-parent.ts`'s `topFolderLeadsOut`, and a share leaves for the server's share-list path
  (`share-list-pane.svelte.ts::openServer`), so the history entry holds the path and Back returns to the share. ❗ That
  `..` row has no parent path, so it's no drop target and opens no context menu: its `entry.path` would otherwise be the
  mount's parent on the boot disk (`/Volumes`).
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

User opens Servers → the pane lists smb:// (SmbServerVolume: memory, the stores, the mount table; no dial)
       └─ servers-list-pane: triggerNetworkDiscovery() + holdDiscoveryForServersView()

User opens an SMB server's row → the pane goes to smb://<server> → its listing (SmbServerVolume)
       ├─ the share cache answers (an earlier visit), else the backend's ladder lists
       └─ a sign-in refusal → the calm error screen, and the sheet once (a listing the person started)

User opens a one-place server's row → network-row-open → onVolumeChange → the pane lands on a `saved`
       volume → ../pane/place-connect dials, RemoteConnectView renders the wait

User presses F7, ⌘K, or "Add server…" → the sign-in sheet opens in add mode
       ├─ SFTP / WebDAV → connectServer(target) → the pane lands on the volume
       └─ SMB → connectToServer(address, name) → TCP check → save + inject host
            │   ("Add anyway": no check, saved without contact, nothing injected)
            └─ the sheet answers `handed_off`
                 └─ the pane opens the host's share list, its arrival mounting sharePath once it lands
            ("Add" with nothing to open: the new row is selected once the re-list lists it)
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
named after the share, with the server's friendly name beside it ("testshare on Testnas": the Servers row's label, else
the host's name) and the `host:port` it dials under that, and that view's Cancel (`share.back`, Escape, cancels too: the
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

## Lazy mDNS trigger, and the Servers list's hold

`triggerNetworkDiscovery()`:

1. No-ops if `network.enabled === false`.
2. Calls `noteNetworkAction()` (backend: reload the manual servers, run the existing-SMB-mount upgrade pass, which holds
   the browse while it resolves).
3. Sets `network.firstTriggerDone = true` so later launches warm the server list up briefly (returning users see known
   servers at once, without re-prompts).

Call sites: a pane showing the Servers list (`../pane/servers-list-pane.svelte.ts`) and
`VolumeBreadcrumb.handleSubmenuAction` (the OS-mount → direct-smb2 upgrade also opens a private-IP socket).

The browse itself runs while a pane shows the Servers list: `../pane/servers-list-pane.svelte.ts` holds it through
`holdDiscoveryForServersView()` (an `$effect` on the pane's location, whose cleanup releases it), and the store tells
the backend only on the first view shown and the last one gone (`setServersViewShown`). Two panes can each show one.
`initNetworkDiscovery` re-sends the store's count, so a reloaded page corrects what the one before it left. Between
browses the backend keeps the hosts it found, so a view opens on them at once and the fresh browse adds and drops hosts
live. Backend side: `src-tauri/src/network/DETAILS.md` § "Discovery runs only while something needs it".

## Key decisions

- **Lazy discovery on first user intent, not at startup**: avoids the macOS Local Network prompt on fresh installs
  before the user has context; `network.firstTriggerDone` persists so returning users get the launch warm-up.
- **Resolution is fire-and-forget**: hosts come and go, so a timeout during resolution is normal, not worth surfacing.
- **No share prefetch** (#324; getcmdr/cmdr#1517): a server's shares are read when a pane opens its share list, and a
  first open waits for that read (the numbers in § "`network-store.svelte.ts`").
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
- **A Servers row's MCP name carries its facts as tokens** (`../pane/mcp-row-name.ts`: `protocol=` / `status=` /
  `address=` / `account=`), because MCP `PaneFileEntry` has no field for them; the status token stays untranslated on
  purpose, since an agent parses it.
- **The Servers row's identity in the switcher is the id `network`; its name is display.** The pane pushes the catalog
  word (`fileExplorer.navigation.networkVolume`, `Servery` in Czech), while agents read Rust's
  `volume_listing::SERVERS_VOLUME_NAME` (`Servers`) in `cmdr://volumes` in every language.
  `../pane/volume-selection.ts::selectVolumeByName` takes both, because the row is synthetic and no `findIndex` over the
  volume list reaches it. ❌ Never tell where a pane is by this name: `tools/actions/nav.rs` did, and every
  `select_volume` on the Servers list failed outside English.

## Dependencies

- `#lib/tauri-commands`: `listNetworkHosts`, `resolveNetworkHost`, `listSharesWithCredentials`, `shareListHost`,
  `saveSmbCredentials`, `deleteSmbCredentials`, `getUsernameHint`, `getKnownShareByName`, `updateKnownShare`,
  `updateLeftPaneState`, `updateRightPaneState`, `connectToServer`
- `#lib/settings/network-settings`: `getNetworkTimeoutMs`, `getShareCacheTtlMs`
- `#lib/utils/confirm-dialog`: `confirmDialog`
- `#lib/ui/toast`: `addToast`
- `../types`: `NetworkHost`, `DiscoveryState`, `ShareInfo`, `ShareListResult`, `ShareListError`, `AuthMode`
