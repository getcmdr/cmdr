# A server's share list becomes a real listing

**Problem.** A server's share list is its own hand-rolled list (`PlacesBrowser.svelte` + `PlacesHeader.svelte`), so it
lacks what every file list has (column header, sort, quick filter, the real `..` row, the SelectionInfo footer, the
standard MCP mirror), carries near-duplicates of the list's cursor, parent row, landing, and MCP code, and its header
grows buttons (Back, Sign in as…, Forget saved password, Browse as guest) no other pane has.

**Goal (David, 2026-11-25).** The backend lists a server's shares as folder entries on the `network` volume at
`smb://<server>`; the standard Brief/Full views render them; the description is a column. `PlacesBrowser`,
`PlacesHeader`, and the plumbing that only exists for them retire. Every user flow keeps working (§ "Coverage
contract"), and the account actions move to the server row's menu, a clickable account in the path bar, and two palette
commands.

This is a just-in-time plan per `docs/guides/multi-agent-refactors.md` § "The loop". It captures intent: when the code
disagrees with a step here, follow the intent and say so in the report.

## Loud rules (every milestone)

- **Characterize, then convert.** No milestone deletes behavior that isn't pinned by a test that survives the deletion
  (a Rust test, a pure-module test, or an MCP-level E2E), and the pin lands BEFORE the rewrite.
- **The views stay generic.** `FullList`, `BriefList`, `SelectionInfo`, and the listing engine get no `if share`.
  Share-specific behavior hangs off the per-kind seams that already exist (§ "Fit check", the branch list). A branch
  that doesn't fit one of those seams is a stop-and-report, ❌ not a new ad-hoc check.
- **❌ Never classify by a volume-id or path string** when a kind exists: `capabilitiesForPane(volumeId, path)` answers
  "is this a share list". The ONE place that reads `smb://<server>` as a share list is that routing.
- **#324 holds**: a server's shares are listed only when the person opens it. The share-list volume is reached only by a
  pane listing it; prefetch, the stale refresh, and ⌘R on the hub keep their `savedSmbHostIds` gate; a restored pane
  never lists at launch (§ "Decisions", 1).
- **❌ Never ask the Keychain twice**, and never from a background path. The stored-password read happens once, inside a
  listing the person opened (§ M4b).
- **One sign-in sheet, ❌ no in-pane credential form.** Every door (the error/empty state, Sign in as…, the row menu,
  the palette) goes through ONE share-list sign-in module.
- **FilePane.svelte and DualPaneExplorer.svelte are at their size cap**: new pane state is a `*.svelte.ts` factory, pure
  logic a `*.ts` helper (`apps/desktop/src/lib/file-explorer/pane/CLAUDE.md`).
- **Docs move with the code**, every milestone: the network, pane, views, servers, `listing_host`, `cmdr-smb`, and
  `cmdr-fs` C+D.md files that describe a touched behavior. No milestone tags in code, comments, or docs.
- **Strings**: new English keys with good `@key` descriptions, ❌ never translated by the executor. Reuse the existing
  keys where the words don't change (`fileExplorer.network.share.parentRowName`, `shareCount`, `signInAs`,
  `forgetPassword`, `useGuest`, the `errors.shareList.*` set).
- Foreground commands only. Checks per `AGENTS.md` § "Checker script"; `desktop-e2e-playwright` (SMB specs) on every
  milestone from M5a on, and Linux E2E on M5c and M7.
- **`main` stays whole after every milestone**: no milestone removes a door (a button, a key, a menu item) whose
  replacement lands later. That's why the account doors (M5b) land BEFORE the switch that deletes `PlacesHeader` (M5c).

## Current state (fresh read, 2026-11-25, `f08cd856bb`)

- **The pane**: `FilePane` renders `NetworkMountView` when `caps.kind === 'network'`. The `network` volume is
  frontend-virtual: no backend `Volume`, path sentinel `smb://`, `hasBackendListing: false`, `syncsToMcp: false`.
- **`NetworkMountView.svelte`** (786 lines): holds `currentNetworkHost` + `currentHostLabel`, swaps between `ServersHub`
  and `PlacesBrowser`, owns the mount flow (`mountShare`, `RemoteConnectView` wait with Cancel and the 10 s slow hint,
  the calm `auth_required` pane, the red mount-error pane, `askForMountCredentials`/`remountWith`, retry), the
  `autoMountShare` one-shot (`retireAutoMount`), the add-server hand-off, `handleNetworkBack` (cursor back to the
  server's row), the `mountError` MCP mirror, and the `BrowserAPI` delegation (`handleKeyDown`, `setCursorIndex`,
  `getItemCount`, `jumpToPrefix`, `findItemIndex`, `getNetworkCursorEntry`, `openCursorItem`, `openContextMenuAtCursor`,
  `refreshNetworkHosts`, `setNetworkHost`).
- **`PlacesBrowser.svelte`** (874 lines) + **`PlacesHeader.svelte`** (129): the share list, its loading/sign-in/error/
  empty states, the listing auth ladder (`loadShares` → cached state → `settleListingAuth` → `tryStoredCredentials` (one
  `getSmbCredentials`) → `askForCredentials` (sheet; cancel = back) → `listWithCredentials`), `signInAs`, `useGuest`,
  `handleForgetPassword`, `authenticatedCredentials` handed to the mount, the `..` row (`PARENT_ROWS`), the
  `landOnShare` landing, the `autoMountShare` effect (with the "not found" toast), the hand-rolled MCP push
  (`Network > host`, `comment="…"` tokens, `..` at `smb://`), keys (`share.back`, `share.selectShare`, navigation
  shortcuts, arrows), and type-to-jump by prefix.
- **Host plumbing outside the view**: `pane/network-host-state.svelte.ts` (host, `autoMountShare`, `shareLanding`),
  `NavigationHistory` entries' `networkHost` (not persisted: a restart lands on the hub), `navigate.ts` /
  `navigate-return.ts` / `navigate-commit.ts` `setNetworkHost` calls, `pane-mirror.ts`'s network arm (host + auto-mount
  mirroring), `share-parent-pane.svelte.ts` (`openServer` with `networkHost`, then `setLanding`), FilePane's
  `initialNetworkHost` / `onNetworkHostChange`.
- **Backend listing**: `commands/network.rs` `list_shares_on_host` (account ladder `account_listing_for`: typed account
  → session-cached password only, else guest), `list_shares_with_credentials`, `prefetch_shares`, all over
  `smb_client::list_shares` (per-host TTL cache, keyed by host id). `ShareListError` lives in `crates/cmdr-smb`.
- **SMB server row menu in the hub**: a NATIVE menu (`commands/menu.rs::show_network_host_context_menu` →
  `network-host-context-action` event → `servers-hub-actions.ts::runHostAction`), while SFTP/WebDAV/S3 place rows use
  the house `Menu` with `navigation/row-menu.ts`'s `volumeRowMenu`.
- **Footers**: `ServersHubStatusBar` (a `<button>` inside the hub's grid, rendered only when the hub has rows) pads
  `--spacing-xs --spacing-sm` with a 1px top border; `SelectionInfo` (FilePane's footer slot, gated by `pane-footer.ts`)
  pads `--spacing-sm --spacing-md`, no border, and on a normal pane sits under the 2px disk usage bar. Same font size
  and `min-height`. Their heights differ, and so do their top edges (1px border vs 2px track).
- **Precedents to reuse**: per-volume entry facts on `FileEntry` (`git_meta`, `restic_meta`, worded on the FE);
  `extCellText` as the one Ext-cell rule (restic puts its host there); `VolumeError::LinkNeedsConsent` as a typed
  listing refusal that `ErrorPane` turns into an action; `capabilitiesForPane` path routing (archive, git portal,
  restic); `listing-complete` carrying extra facts (`shareParent`); `has-parent.ts`'s `topFolderLeadsOut`.

## Fit check

**Verdict: it fits, with a provider behind the existing `Volume` trait and a path-routed pane kind. The listing engine
should need no change, but that's unproven until M4a's engine test passes** (see the app-path landmine there). The share
list is a flat, read-only, non-watched directory of folders, which is the easiest shape `cmdr-listing` serves. All
share-specific behavior lands on seams that already branch per kind, but there are more of those seams than a first read
suggests: the navigation refusals, the parent step, the remembered path, and every volume-only capability lookup all
assume today that the `network` volume never lists (branches 10–15 below).

**The shape.**

- A Rust **`SmbServerVolume`** (app side, `src-tauri/src/network/share_list_volume.rs`; it needs discovery, manual
  servers, and the Keychain, so it can't live in `cmdr-smb`) registered in the `VolumeManager` under the id `network`.
  `list_directory(smb://)` answers `NotADirectory` (the hub never lists; a stray read gets a typed no).
  `list_directory(smb://<server>)` resolves the server, runs the account ladder, and returns one directory `FileEntry`
  per share: name = share name, path = `smb://<server>/<share>`, `share_meta: Some(ShareMeta { description })`.
  `list_directory_for_refresh` bypasses the share cache (⌘R on a share list re-reads, as the hub's ⌘R does today). No
  watcher (nothing to watch), not indexable, not writable.
- **Path identity**: `smb://<host>[:<port>]`, the port only off 445, `host` a key that never changes for one server (§
  "Decisions", 5). ❌ Not the IP (DHCP drift) and ❌ not "the resolved hostname, else the Bonjour name": resolution is
  async, so one server would get two paths depending on whether it had resolved when clicked, splitting history, tabs,
  `landOnServer`, and the restore normalization (`network/CLAUDE.md`: "IP and hostname both drift", which is why
  credential status is keyed by lowercase `host.name`). ONE TS module (`share-list-path.ts`, beside
  `server-path-utils.ts`) holding the writer `shareListPath(host)` AND its parser `shareListHostKey(path)` (the path bar
  and tab labels, `landOnServer`, Edit server, and the palette target all read the host back from the path; ❌ no second
  parse at a call site), tested as a round trip; and ONE Rust reader (resolve to a `NetworkHost` via discovery, else the
  manual servers, the way `smb_share_parent.rs` already makes a host from a mount's server). A Bonjour name can hold
  spaces, apostrophes, and capitals ("Test_user_1's NAS"): the writer/reader pair defines the escaping, and both path
  grammars (`#lib/path/grammar.ts`, `cmdr_fs::app_path`) must treat it as one authority token.
- A **routed pane kind `share-list`** in `volume-capabilities.ts`: `capabilitiesForPane('network', 'smb://<x>')` answers
  it (`hasBackendListing: true`, `syncsToMcp: true`, `hasParentRow: true`, `canWrite`/`canBeSource`/
  `canBeIndexed`/`canShareLinks`/favorites `false`); `smb://` stays the `network` (hub) kind. Since the network volume's
  root is `smb://`, `computeHasParent` gives the share list a `..` with no new rule.

**Every share-specific branch, named** (all at existing per-kind seams):

1. `volume-capabilities.ts`: the `share-list` kind and its routing (the one path read).
2. The pane's open path (Enter, double-click, `openCursorItem`, MCP `open_under_cursor`): a folder on a `share-list`
   pane MOUNTS (the share-mount controller) instead of navigating. One branch where the pane already forks archives. ❗
   Every OTHER verb that takes a folder row somewhere must refuse or route to the mount, never reach
   `smb://<server>/<share>` as a path: open in a new tab, open in the other pane, drag a share row out (to the other
   pane or Finder), drop onto a share row (a folder row is a drop target), Space's folder-size walk, Quick Look, About
   (⌘I) / MCP `about_item` / `inspect_file`, and the row context menu. M5a's audit lists them with a pin each. ❗
   **`canBeSource: false` enforces nothing today.** It's a "structurally-true-no-guard cell" (the `network` row's own
   comment): only `FunctionKeyBar` reads it, to grey the buttons. The F5/F6 key path, ⌘C/⌘X, F8, duplicate, drag-out,
   and the MCP tools never ask it, and `command-dispatch.ts::blockedByCapabilities` toasts only for `search-results`
   (every other kind falls through to the handler). The share list is the first kind where `canBeSource: false` meets a
   real list with a cursor row and a listing id, so each verb needs its refusal at the seam that already refuses per
   kind, worded per kind, typed to MCP through `refuseOpening`:
   - Pane-side writes (F7, ⇧F4, F8/⌘⌫): a `share-list` arm in `file-operation-commands.ts::readOnlyRefusal`, beside the
     archive/git-portal/restic arms.
   - Anything landing IN a share list (F5/F6 from the other pane, paste, drop on the pane or on a share row, duplicate):
     a `share-list` arm in `transfer-entry.ts::checkTransferDestinationGuard`. A drop on a share ROW targets
     `smb://<server>/<share>`, which routes to the `network` kind, not `share-list`, so the guard must refuse the
     `network` kind too (or the row-drop target must be off for the kind); either way, pin the row drop.
   - Anything taking shares OUT (F5/F6 opener, ⌘C/⌘X, drag-out start, MCP copy/move): no source guard exists, so add ONE
     (`!canBeSource` from `capabilitiesForPane`) in the shared transfer entry, called by `transfer-dialog-opener.ts`,
     `clipboard-operations.ts`, and the drag start; ❌ not three ad-hoc checks.
   - Rename: `file.rename` (F2, palette, and MCP `rename`) already routes `network` to Edit server, which is what F2
     means on a share list; MCP `rename` with `{ initialName, expectedName }` would then open Edit server instead of
     answering, so it refuses typed there. Mouse rename activation goes through `startRename` → `readOnlyRefusal`
     (covered by the arm above).
   - Quick Look (⇧Space) and its cursor-follow: the backend gate is `volume_supports_local_fs_access`, and an unknown
     volume counts as local, so `SmbServerVolume` must answer `supports_local_fs_access() == false` (the trait default
     is `true`); then ⇧Space behaves like it does on an MTP phone.
   - The context menu: today a share row has none; after, `file.contextMenu` (right-click and ⌃⏎ both reach
     `pane-pointer.ts::handleContextMenu`) would open the standard file menu on it. § "Decisions", 10. The `..` row's
     own menu ("Add to favorites" for `smb://`) is already gated by `paneFolderCanBeFavorited`; pin that it pops nothing
     on a share list.
   - Backstop: `SmbServerVolume`'s write methods keep the trait's typed `NotSupported`; no refusal above relies on it.
3. `..` on a share list navigates to `smb://`, which is the hub; the hub takes a `landOnServer` (the share list's path)
   and puts its cursor on that server's row. `navigate.ts` already special-cases `network`; this replaces
   `setNetworkHost` there, it doesn't add a branch. ❗ `..` and `⌘↑` don't go through `navigate.ts` but through
   `FilePane.navigateToParent` → the loader, which can't reach the hub (branch 11). ❗ `landOnServer` is DERIVED, ❌ not
   threaded: whatever commit lands a pane on `smb://` FROM a `share-list` path (`..`, `⌘↑`, Backspace on the ErrorPane,
   Cancel on the auto-opened sheet, history Back, a mount refusal's Back) sets it from the path it left, in one place in
   the commit. The hub consumes it once (`ServersHub.selectServer` on the row whose host key matches,
   `handleNetworkBack`'s rule today) and drops it. Restore is the one landing with no path to leave (branch 8).
4. The description cell: `extCellText` returns `shareMeta.description` (restic precedent), and `FullListHeader` labels
   the track "Description" on that kind (§ "Decisions", 2). Renderer and measurer stay one contract.
5. `ErrorPane` / `EmptyFolderState`: a typed share-list refusal carries a `sign_in_to_server` action kind; the empty
   state on a `share-list` pane gets its own words (via `empty-folder-text.ts`). Plumbing, checked against the code:
   - `ErrorActionKind` today is handler-free (`open_privacy_settings`, `open_smb_encryption_setting`: ErrorPane calls a
     module function). Sign in follows that shape, ❌ not the `onReviewLinkConsent` prop shape (FilePane is at its cap):
     ErrorPane calls `share-list-sign-in.ts`'s door with its `folderPath` (the share-list path, which the module
     resolves to the host), and on `signedIn` calls its existing `onRetry` (FilePane's `navigateToPath(currentPath)`).
   - The sign-in reason goes into `listing-error.ts`'s `REFUSALS` (no "Send error report…"), category `needs_action` (no
     red icon), `retryHint: false` for the credential classes (a retry without an account gets the same answer, the
     cmdr-reports#10 rule the mount pane follows), `true` for the server classes.
   - ErrorPane is `role="alert"` `aria-live="assertive"`, and the sheet auto-opens over it: VoiceOver would hear the
     alert and the sheet at once. The calm sign-in classes render without the assertive alert (pin in the a11y test).
   - ErrorPane prints `folderPath` raw: on a share list that's the host key, ❌ not the friendly label the path bar
     shows. Pass the label (the same lookup as branch 7).
   - ❗ `EmptyFolderState` is a `pointer-events: none` overlay inside both views with no tab stop (its own doc says a
     focusable control there is a stray tab stop), so a Sign in BUTTON there would need view changes and break the focus
     model. The empty state gets words only, pointing at the path bar's account (M5b); § "Decisions", 6.
6. `SelectionInfo`'s count noun: "8 shares" on a `share-list` pane (the noun comes from the kind, like the snapshot
   pane's).
7. The path bar: on `share-list`, the display path is the server's friendly label (not the host key), followed by a
   muted, clickable "as <account>".
8. Restore and history: a persisted `share-list` path restores to `smb://` with `landOnServer` (§ "Decisions", 1). The
   carrier is named in M5a's landmines (a restored tab has no path to leave, so branch 3's derivation can't apply).
9. `pane-mirror.ts` (Copy path between panes) and the MCP mirror: a share under the cursor mirrors as "open the share
   list, then mount that share" (today's behavior), ❌ not as navigating to the share's `smb://` path.

10. `navigate.ts`'s two network refusals: `navigateToLocation` refuses EVERY `smb://<x>` other than `smb://`
    (`SMB_PATH_REFUSAL`), and `navigateInPlace` refuses every in-place navigation while the pane is on `network`
    (`onNetworkRefusal`). Both stand between the hub and a share list today (hub → share list is a same-volume step).
    After: `smb://<server>` (exactly one authority, no share) is allowed in both; `smb://<server>/<share>` keeps its
    refusal (a share is mounted, never navigated to); MCP `nav_to_path` gets the same rule.
11. The parent step: `loader.navigateToParent` would `loadDirectory('smb:/', …)` (`parentOf('smb://x')` is `smb:/` by
    the TS grammar's slash arithmetic) against a volume that answers `NotADirectory` for the hub. `FilePane`'s
    `navigateToParent` already forks first (`shareParent.leaveTopFolder()`, then the network arm): the `share-list` arm
    goes there, and commits `smb://` with `landOnServer` without a load. The same `smb:/` bug hits
    `pane-mcp-sync.svelte.ts`'s `..` entry (`parentOf(canonicalPath)`), which must read `smb://` to keep the M1 pin.
12. The remembered path: `commitPathFromListing` and `commitVolumeSwitch` record `last-used-path` per volume, and a
    volume select may land on the remembered folder or the other pane's path. Once a share list is a `network` path,
    picking Servers in the switcher (or MCP `select_volume`) would reopen, and LIST, the last server: a #324 breach and
    a change to "Servers lands on the hub". A `share-list` path is never recorded as `network`'s last-used path, and the
    Servers select always lands on `smb://`.
13. Volume-only capability lookups: about 20 call sites outside `volume-capabilities.ts` ask `capabilitiesFor(volumeId)`
    with no path (`pane-commands.ts`'s `isSnapshotPane: !hasBackendListing`, `selected-paths-read.ts`,
    `selection-attachments-read.ts`, `transfer-dialog-opener.ts`, `file-operation-commands.ts`, `DualPaneExplorer`'s
    `volumeHasListing`, `FunctionKeyBar`, the terminal and search gates, `about-this-opener.ts`, …). For archive/git/
    restic that's benign (their base volume lists too); for `network` the base kind says `hasBackendListing: false`, so
    a share-list pane would read as a snapshot pane. Each site that can see a share-list pane moves to
    `capabilitiesForPane`; this is an audit, not new branches. The `share-list` route itself must also gate on
    `volumeId === 'network'`, since `routedCapabilities` today reads the path alone. ❗ A second set the
    `capabilitiesFor(` grep misses: about 15 raw `volumeId === 'network'` checks (`rg "=== 'network'"`), including
    `command-handlers/file-handlers.ts::serverInView` and `servers-handlers.ts`'s `servers.edit` (both read
    `getFocusedPaneNetworkHost`, which M6 deletes), `FunctionKeyBar`'s `canRename` (`caps.kind === 'network'`, so F2
    goes dark on a share list), `VolumeBreadcrumb`, `pane-accessors`, `navigate.ts`'s `onPaneRoot`, and
    `app-status-store.ts`'s restore arm. Same audit, same report line per kept check.
14. The tab label (`tabs/tab-label.ts`, which already special-cases `NETWORK_VOLUME_PATH`): a share-list tab reads the
    server's friendly label, ❌ not the path's host key.

15. Non-pane readers of the volume. Once `network` is registered, every `VolumeManager` caller that resolves an
    `smb://<server>` path reaches `SmbServerVolume::list_directory`: MCP `list_dir` (`tools/read/listing.rs` goes
    through `location::resolve`), `about_item`, `inspect_file`, `folder_importance`, dir sizes. Each is a dial and, from
    M4b, a possible Keychain read that no person asked for, which #324 and the Keychain rule forbid. Only the pane's
    listing runs the full ladder; every other caller gets the cache-only answer (the share cache, else a typed "not
    opened" refusal), § "Decisions", 7. M4a picks the seam (a listing-context flag, or the engine calling a separate
    method) and says which.
16. The footer: `pane-footer.ts::paneFooterVisibility` gives the footer only to `paneViewKind` `'normal'` (with disk
    space) and `'search-results'` (without). A share-list pane routes to `'normal'`, so it would get the disk usage bar,
    and since the network volume never answers a space query, the bar would sit as the "not yet fetched" empty track
    forever, which reads as "0% full". The share list gets `SelectionInfo` and ❌ no volume space: feed the routed kind
    (or a capability) into `paneFooterVisibility`, ❌ not a `volumeId` check.

Sixteen named branches, each at a seam that already branches per kind (or, for 13, a lookup that should have been
pane-scoped), with ONE new seam: branch 2's source guard, since nothing refuses a transfer source today. Zero in the
engine and the two views' row code.

**What does NOT fit, said plainly**: the mount flow. A share isn't a folder you enter; it's a mount that leaves the
network volume for `/Volumes/<share>` (or a direct `SmbVolume`). That flow (wait view, Cancel, slow hint, sign-in retry,
error states) moves intact into a pane-level controller (`pane/share-mount.svelte.ts`), rendered through the ONE
`RemoteConnectView` (§ M2). It isn't shared-list code, so it adds no branch to the views.

**Considered and rejected**: a listing overlay (`host/overlays.rs`) on a fake directory (overlays decorate a real
listing; the share list has no underlying directory, and an overlaid listing is refused by the authoritative oracle,
which is fine here but buys nothing); an in-frontend `InMemory` listing (the backend must own the auth ladder and the
typed refusals, "smart backend, thin frontend"); a per-server volume id (`smb-server:<host>`) registered per visit (a
volume lifecycle for something that has no session to manage; the one `network` volume with path routing is simpler and
keeps the hub and its share lists on one volume, so the switcher, history, and the drop-foreign-listings prefix rule
stay as they are).

## Line estimate

My estimate: **production code about −500 (range −300 to −800); tests roughly flat to −400; total about −500 to
−1,000.** Close to your −200 to −700, a bit more negative because of the host plumbing and the native menu.

- **Removed (~−2,300)**: `PlacesBrowser.svelte` 874, `PlacesHeader.svelte` 129, about 500 of `NetworkMountView.svelte`
  (host switching, delegation, the two bespoke mount panes, the mount MCP mirror), `network-host-state.svelte.ts` 81
  plus its call sites, the `networkHost` threading in `navigate*.ts` / history / `pane-mirror.ts` /
  `share-parent-pane.svelte.ts` (~150), the native SMB host menu and its event (Rust + TS, ~200), `network-store`'s
  pane-only helpers that only `PlacesBrowser` called (~80), the mount panes' CSS.
- **Added (~+1,800)**: `SmbServerVolume` + path resolution + entry mapping (~350 Rust), the typed refusal in `cmdr-fs` +
  its `ListingError` arm + FE wording delegation (~120), `ShareMeta` (~40), the `share-list` kind (~60),
  `share-mount.svelte.ts` (~300, MOVED from `NetworkMountView`, so near-neutral), `share-list-sign-in.ts` (~250, MOVED
  from `PlacesBrowser`), the branches above (~250), path-bar account chip + menu (~120), SMB row-menu items (~120), two
  palette commands (~80), the shared footer height (~20).
- **Tests**: `PlacesBrowser.test.ts` (816) and most of `NetworkMountView.test.ts` (900) retire or move with their
  modules; new Rust volume tests (~300), share-mount and sign-in module tests (moved), and the characterization E2E
  rewrite. Net flat to −400.

The honest caveat: the Rust volume is genuinely new code, and the three account doors add surface that didn't exist. The
win is mostly in deleted duplication and in features the share list gains for free.

## Coverage contract (every current flow → where it lives after)

Each line is a flow the share list, the mount, or the hub hand-off supports today, and its home afterwards. A milestone
that touches a flow keeps its pin green; M7 walks this list.

**Opening a server's share list**

- Enter / double-click on an SMB host row in the hub → `onHostSelect` → share list. AFTER: the hub navigates the pane to
  `shareListPath(host)` (`onVolumeChange` with `targetPath`); the label travels via the hub's display name (path bar
  reads it from the network store by host key).
- A saved host mDNS isn't seeing (synthesized `NetworkHost`) → AFTER: same path; the Rust resolver falls back to the
  manual server's address. Pin: Rust resolver test.
- "More shares on {server}…" row (#1404) → AFTER: same as the host row.
- An unsaved share of a saved server (#1404) and a saved share no mount went through (`onShareViaHost`) → AFTER: open
  the share list with a one-shot `pendingMount: <share>` on the share-mount controller, consumed once the listing lands.
- Add server sheet with an SMB address → hand-off (`onSmbHandOff`, optional `sharePath`) → AFTER: navigate to the
  injected host's share list, with `pendingMount` for `sharePath`.
- `smb://host/share` from go-to-path / a link → the existing go-to-path handling; AFTER: unchanged, but its "open host,
  then auto-mount" leg uses the same `pendingMount`.
- "Copy path between panes" with the cursor on a share (`autoMountShare`) → AFTER: `pane-mirror` sends the target to the
  share list with `pendingMount`; the "share not found" toast stays (fires when the landed listing lacks it).
- The one-shot rule (cmdr-reports#7): a queued share mounts once, only within the visit it was queued with; leaving,
  mounting, or picking another host retires it. AFTER: `pendingMount` is keyed to the listing id it was queued for and
  dropped on any navigation away. Pin: share-mount controller test (ported from `NetworkMountView.test.ts`).
- #324: opening the share list is the person opening the host (it lists); no other path lists a found host. AFTER: only
  a pane listing reaches `SmbServerVolume`; Rust test that prefetch/stale refresh paths are untouched; E2E.

**The list itself**

- Loading ("Connecting to {host}…" with spinner) → AFTER: the standard listing load (instant for a cached list; the
  `LoadingIcon` overlay past 100 ms). The share list's own sentence is dropped (decide in review if wanted).
- Rows: folder icon, name, description right-aligned → AFTER: Full/Brief rows; description in the Ext track (Full) and
  the row tooltip (Brief), § "Decisions", 2.
- Sorting: was fixed, case-insensitive name. AFTER: the pane's sort (name by default, `cmdr-collation`).
- Quick filter, selection, Space, Insert, ⌘A: absent today. AFTER: quick filter works; selection is allowed but no
  operation accepts a share list as source or destination. ❗ No existing refusal covers this (`canBeSource` only greys
  the F-bar): branch 2 names the new arms. Pin: F5, F6, F7, F8, ⌘C then paste in the other pane, a drop onto the pane
  and onto a share row, and MCP `copy` / `mkdir` / `delete` each refuse with words (typed for MCP) and reach no backend
  write.
- Pinned tabs: today hub → share list happens inside `NetworkMountView`, so a pinned tab on Servers stays put. AFTER:
  it's a same-volume navigation to a new path, so the pinned-tab fork opens the share list in a new unpinned tab, as any
  folder does. Intended (pinned means pinned); pin it in M5c.
- Type-to-jump: prefix match on shares, never `..`. AFTER: the folders' own type-to-jump (fuzzy), which never lands on
  `..` either. Pin: existing pane type-to-jump tests plus one share-list case.
- Arrows, PageUp/PageDown, Home/End, ⌥-arrows: `list-cursor.ts` + `handleNavigationShortcut`. AFTER: the file list's own
  keys. (`list-cursor.ts` stays: the hub uses it.)
- The `..` row: always on, row 0, cursor opens on it, Enter/double-click/`⌘↑`/Escape/Backspace go back to Servers with
  the cursor on the server's row; screen readers hear "Back to Servers". AFTER: the standard `..` row; `nav.parent` goes
  to `smb://`; the hub lands on the server via `landOnServer`; the parent row's accessible name reads
  `fileExplorer.network.share.parentRowName` on this kind. Escape and Backspace: the file list's Backspace = parent
  already; Escape on a file list does NOT go to parent, so a share list loses "Escape = back" (§ "Decisions", 4).
- Cursor landing on a share by name (`landOnShare`, #1238) and the hint toast once the cursor is there → AFTER: the
  generic "select this name once the listing lands" of `navigate` (the same one going up a folder uses), and
  `share-parent-pane` raises the hint from that landing's completion.
- Footer: share count in the header ("8 shares") → AFTER: `SelectionInfo` "8 shares" (branch 6), with no disk usage bar
  or free-space text (branch 16); hidden while ErrorPane shows, like any pane's.
- Path bar: was the header's host label + "as guest". AFTER: `Servers › <server> as <account>` (branch 7).
- Back button in the header → AFTER: gone; `..` and the volume chip cover it.
- The Back BUTTON in the sign-in, error, and empty states (the mouse way out when no `..` row is on screen) → AFTER:
  ErrorPane's "Go back" (history, shown when there is one) and "Go home", plus Backspace/`⌘↑`, which must reach the
  `share-list` parent arm while ErrorPane shows (no rows, no cursor). Pin: E2E `..` key from an auth-refused share list
  lands the hub on the server's row.
- Edit server (F2, F4, ⇧F6, `servers.edit`) on a share list edits THAT server today (`editServerInView` with
  `getFocusedPaneNetworkHost`, and F2 is lit by `caps.kind === 'network'`). AFTER: the same, with the host resolved from
  the share-list path; F2 lit on `share-list` too (branch 13). Pin: a handler test on a share-list pane.

**Signing in to list**

- Cached share state hit (prefetch or earlier visit) → AFTER: `smb_client` cache inside the volume, same TTL.
- Cached `auth_required` / `signing_required` → stored creds silently → sheet. AFTER: the volume's ladder (cached
  password → typed account's stored password read ONCE → guest per `GuestAttempt`) answers a typed refusal; the pane
  auto-opens the sheet on a sign-in refusal (today's behavior), and the ErrorPane under it is the calm sign-in state
  with Sign in and the ways back. ❗ "Once per listing" is too loose now that the pane, not a component mount, starts
  listings: servers principle 4 (`#lib/servers/DETAILS.md`: the sheet opens only on user intent). The auto-ask fires for
  a listing the person started (a navigation onto the share list, Retry, ⌘R), ❌ never for a tab coming back into view,
  a retained listing repainting, or any re-list the person didn't trigger. The auto-ask lives in a pane factory that
  watches the listing's refusal, ❌ not in FilePane.
- Sheet attempt `listWithCredentials`: `handed_off` on success, `refused` keeps the sheet open on
  `authentication_rejected` / `needs_credentials`, any other error closes onto the error state. AFTER: identical, in
  `share-list-sign-in.ts`; success re-lists the pane, and keeps the side effects: `setShareState`,
  `setCredentialStatus`, `setListedAccount`, `setSmbAccountPreference`, `saveSmbCredentials` when Remember, the
  credential-file-fallback toast, `updateKnownShare`. ❗ The re-list must land on what the sheet just proved: the
  sheet's `list_shares_with_credentials` primed the share cache and the typed credential is the server's session
  credential (M4b), so the pane re-lists through the normal path (❌ not `list_directory_for_refresh`, which would dial
  again), and the ladder answers from that credential, ❌ never falling to guest.
- Cancel on the auto-opened sheet → back to Servers (cursor on the server). AFTER: same, through `nav.parent`.
- `guestAllowed` per the listing's `authMode`; "One ask at a time" (`signingIn` guard). AFTER: same, in the module.
- Error state (non-auth): red title "Couldn't connect to {host}", sentence from `renderShareListError`, Retry, Sign in,
  Back; `missing_dependency` shows the install command in a `CopyBox` and no Sign in. AFTER: `ErrorPane` with the typed
  share-list refusal: words via `renderShareListError` (single source), Retry = pane refresh, Sign in action, `..` for
  back; the install command keeps its copy box (verify `ErrorPane` has one; else add it there, not in a share view).
- Empty state ("No shares", Sign in, Back) → AFTER: `EmptyFolderState` with the share-list words pointing at the path
  bar's account; the `..` row is on screen for back; Sign in through the account chip, the row menu, or the palette
  (branch 5 says why the overlay holds no button; § "Decisions", 6).
- MCP while the sheet is up: `getItemCount() === 0`, `getShareUnderCursor() === null`. AFTER: the pane's normal
  modal-dialog key suppression; MCP reports the listing (the sheet is a dialog the `ui_state` mirror already reports).

**Account actions**

- "Sign in as…" (header): sheet with no refusal, prefilled with the listed account, no guest option, cancel stays.
  AFTER: three doors (row menu, path-bar account menu, palette) → one `signInAs(host)` in `share-list-sign-in.ts`.
- "Browse as guest" (header, only when the account is a user and a guest listing worked): sets the account preference to
  none first, then lists as guest. AFTER: same three doors, shown under the same condition (§ "Decisions", 3).
- "Forget saved password" (header, when this session read one: `authenticatedCredentials` or `has_creds`, from the
  cache-only `noteCachedCredentials`): forget + success/failure toast. AFTER: the three doors, shown per § "Decisions",
  8 (same rule in every door). The palette door is the EXISTING `servers.forgetSecret` gaining the host arm (M5b), ❌
  not a third new command.
- The hub's native SMB host menu (Edit, Disconnect, Forget saved password, Forget server) and ⌃⏎ → AFTER: the house
  `Menu`, same items as SFTP/WebDAV server rows where they apply (Open, Edit…, Disconnect, Forget saved password, Forget
  server; ❌ no pin: an SMB host has no switcher row to pin) plus Sign in as… and Browse as guest; row identity by
  `rowId` end to end, as today. ❗ `volumeRowMenu` takes a `VolumeInfo` and its actions run on a `volumeId`; an SMB host
  row has none (a many-place server, like an S3 account), so the host set is a sibling builder in `row-menu.ts` keyed by
  the hub row, ❌ not a fake `VolumeInfo`.
- ❗ A Keychain read hides in today's hub menu: `servers-hub-actions.ts::openHostMenu` calls `checkCredentialsForHost`,
  which is `getSmbCredentials` (a Keychain read, and the password crossing into the frontend) on every right-click of a
  host whose status is unknown. The house menu's no-read visibility retires it; M5b deletes `checkCredentialsForHost`
  and, once `PlacesBrowser` and the mount stop calling it (M5c), the frontend's `getSmbCredentials` wrapper (invariants
  2 and 3). The row menu's "Forget saved password" then needs a rule that costs no read (§ "Decisions", 8).
- Sign in as… or Browse as guest from a hub ROW (new: today only the header has them): success opens that server's share
  list in the pane, the way activating the row does (the person pointed at the server, so #324 holds).
- MCP has no tool for any account action today (no context-menu or command-run tool), so nothing to keep in step there;
  the palette and the row menu run the same functions.
- The listed account labels ("as testuser" / "as guest") on the hub row and the share list header must agree
  (`signed-in-as.ts`). AFTER: the path bar reads the same `getListedAccount`; `listing-complete` on a share list carries
  the account the listing used, and the pane notes it into the store.

**Mounting a share**

- Enter on a share: stored-creds attempt when `authMode === 'creds_required'` and none in memory, then
  `mountNetworkShare` with whatever it has; already mounted short-circuits. AFTER: the frontend holds no credentials;
  `mount_network_share` with no credentials uses the server's session credential (the one the listing signed in with,
  including a typed-not-remembered one, § M4b). ❗ A listing often succeeds WITHOUT Cmdr's stored password (a cached
  list, guest, macOS `smbutil` riding the system Keychain), which is exactly why `activateShare` reads Cmdr's stored
  password before mounting today. So the mount, finding no session credential, makes the ONE stored-password read
  itself, through the same per-server "read once this session" memo the listing uses; it never reads a second time for a
  server the listing already read (hit or miss). Keychain-once preserved.
- The wait: `RemoteConnectView` "connecting", share + friendly server + `host:port`, Cancel (and `share.back` as
  Cancel), slow hint at 10 s, a requested cancel returns quietly. AFTER: same view, owned by `share-mount.svelte.ts`.
- Mount `auth_required` → calm pane + sheet (no guest, prefilled, `refusalForMountError`); `auth_failed` /
  `permission_denied` → sheet over the red pane; `remountWith` keeps the sheet open only on a credential refusal; a
  working account becomes the host's account and is saved when Remember. AFTER: same logic in the controller; the two
  panes become `RemoteConnectState` arms (`signed_out` for `auth_required`; `refused` with `retry` + a `back` for the
  rest; ❌ no bespoke pane). Checked: `refused` has no `back` today and `signed_out` has only `signIn`, while today's
  calm mount pane has Sign in AND Back, so both arms gain an optional `back`. `signed_out`'s doc says the sheet never
  opens on its own; that's about a dropped session, and a mount the person just started is user intent, so the doc says
  so rather than the mount skipping the auto-ask.
- Mount error MCP mirror (`mountError: { share, reason, message }`). AFTER: the share-list pane's standard mirror
  carries the controller's `mountError`.
- Success: lands on the mounted volume (`resolvePathVolume`), the saved-share bookkeeping (`remember_mount_since`), the
  OS-mount fallback notice. AFTER: unchanged (Rust side untouched).
- Keys during a mount or a mount error: only `share.back` (Cancel / Back); the rest dead. AFTER: the controller claims
  `share.back` the same way while its state is up. ❗ Today that "rest dead" comes free (NetworkMountView owns all
  keys); after, `pane-key-router.ts` would route Enter, arrows, and selection to the hidden list (FilePane's three
  existing RemoteConnectView arms don't gate keys either). The router needs an early arm for the controller's state, as
  it has for `getIsNetworkView`, and the controller refuses a second mount while one runs (Enter or MCP
  `open_under_cursor` on a retained row). `⌘↑` claimed there must not also reach the document dispatcher's `nav.parent`
  (today's `stopPropagation` comment in `NetworkMountView.handleKeyDown`). Pin: Enter during a mount starts nothing.
- Where it renders: a fourth `RemoteConnectView` arm in FilePane's `{#if}` chain, ahead of `friendlyError` and the list
  arms (the reveal, Cancel, and words stay the view's; ❌ no second renderer). `name` = share, `server` = the friendly
  label, `address` = `host:port`, as today. The 800 ms reveal counts from the view mounting, so the controller enters
  `connecting` only once it actually dials (an already-mounted share short-circuits without a blank flash).
- Focus: the wait and refusal views replace the list, so DOM focus leaves it. Keys must still reach the pane while they
  show (Escape cancels a mount started from the keyboard), and when the list comes back (Back, Cancel, a failed sign-in)
  focus returns through `focusActivePane`, ❌ never a bare `.focus()` (`pane/CLAUDE.md`). Nothing auto-focuses the
  revealed Cancel. Pin: keyboard-only E2E of Enter → Escape → arrows still move the cursor.

**Elsewhere, unchanged but must keep working**

- The direct-connect upgrade (`connectDirectly`, chip yellow dot, switcher switch, fallback notice): not touched; pin
  stays `direct-connect.test.ts` + E2E.
- #1238 `..` out of a share's top folder into its server's share list, landing on that share, and the hint toast: AFTER:
  `share-parent-pane` navigates to `shareListPath` with the select-name landing.
- Tabs and history: each tab keeps its own share list now (the path carries the host); Back/Forward walk share lists as
  normal path history. Persistence: § "Decisions", 1.
- MCP: `move_cursor` / `open_under_cursor` / `cmdr://state` on a share list → the standard pane mirror (volume id
  `network`, path `smb://<server>`, `..` at 0, shares as directories with the description token kept in the name,
  `comment="…"`, so agents and `smb.spec.ts` keep reading it); `select_volume` onto Servers still lands on the hub
  (branch 12). Two things the mirror changes on purpose: the pane path (today `smb://<ip ?? name>/`, after the stable
  key with no trailing slash) and `volumeName` (today `Network > <host>`, after the standard volume name). M1 pins
  neither spelling; M5c pins the new ones. `nav_to_path` onto `smb://<server>` opens the share list (branch 10).
- Tab labels: a share-list tab reads the server's friendly name (branch 14).
- OS-mounted shares (mounted in Finder, or by `mount_smbfs`/`gio` outside Cmdr): Enter on such a share short-circuits to
  the existing mount, no credential asked (today's `activateShare` comment); the controller keeps that. Today the
  stored-password read happens BEFORE that short-circuit on a `creds_required` server; moving it after (read only when
  the mount actually needs a credential) is a free improvement M4b may take, as long as it stays one read.
- Linux and Windows: `SmbServerVolume` and the session credential compile and behave on all three
  (`pnpm check clippy-windows`); Linux mounts through `gio` (`os_mount/platform/linux/`) and keeps its
  `missing_dependency` copy box.
- Focus: the pane's standard focus model (list holds DOM focus).
- a11y: the standard listbox, row names, `..` name override, `network.a11y.test.ts` (hub) stays; a new axe pass on a
  share-list pane.
- The hub footer matches SelectionInfo's height, from one source.

## Decisions (David)

1. **Restore**: a pane (or tab) left on a share list restores, after a restart, to (a) the Servers list with the cursor
   on that server (today's behavior, and nothing lists at launch, #324), or (b) the share list itself (lists that server
   at launch). **Plan default: (a).** History within a session keeps share lists.
2. **Description column**: (a) the Ext track carries it, headed "Description" on a share list, measured like any Ext
   cell (restic precedent, one cell rule, no new column machinery); Size and date tracks stay as for any folder. (b) A
   new dedicated column, hiding Size and date on share lists (more machinery in the column fit and the measurer). **Plan
   default: (a).**
3. **Browse as guest** exists today in the header but isn't in the brief. Plan default: keep it in all three doors,
   shown only when it applies.
4. **Escape** goes back to Servers today; on a file list Escape doesn't mean "parent". Plan default: drop it for
   consistency (Backspace, `⌘↑`, and `..` remain). Say if you want it kept as a share-list key.
5. **The path's host key** (§ "Fit check", path identity): (a) the lowercase Bonjour `host.name` for a discovered server
   and the typed address for a manual one, the key credential status already uses, so `smb://test_user_1's nas` is a
   legal path and the grammar escapes it; (b) `host.id` (stable, but an opaque id in a path the person sees in history,
   MCP, and Copy path); (c) a slug of the name (readable, but two servers can collide). **Plan default: (a)**, with the
   path bar showing the friendly label either way. A server seen both by discovery and as a manual entry must resolve to
   ONE key, the same one the hub's prefetch caches under.
6. **Sign in on an empty share list** (a guest listing that came back with no shares): (a) words only, pointing at the
   path bar's account ("Sign in to see more shares: click the account next to the path"), with the chip, row menu, and
   palette as the doors; (b) a real button, which means a generic action slot in `EmptyFolderState` with pointer events
   and a tab stop, in both views. **Plan default: (a)**: the views stay generic, and the chip is one click away.
7. **Who may list a server besides the pane** (branch 15): (a) the pane only; MCP `list_dir` and every other reader get
   the share cache or a typed "open it first" refusal, never a dial or a Keychain read; (b) an agent's `list_dir` may
   dial with the cache-only ladder (no Keychain), so an agent can see a server's shares before the person opens it.
   **Plan default: (a)**: #324 stays literal.
8. **"Forget saved password" visibility in the hub's row menu**: (a) shown only when this session read the password
   (cache only; a stored password nobody read yet has no hub door, though it does once the share list or a mount read
   it); (b) always shown on a saved SMB server, with the toast saying whether there was one (a delete, so no read
   prompt). **Plan default: (b), in EVERY door** (hub row, share-row menu, path-bar account menu, palette): today's hub
   menu shows it (at the cost of a Keychain read on right-click), (b) keeps that without the read, and it's already the
   house rule for server rows (`row-menu.ts::serverActions`: "offered unconditionally, ❌ never gated on 'is one
   stored?'", the command words a `false`). One rule means one builder for the account items. (Review round 4 changed
   this from "the share list's account menu stays (a)": the same action shown by two rules in two doors reads as a bug.
   Say if you want (a) back for the share list.)
9. **`share.back`'s future**: Escape leaves the share list (Decision 4), but the mount wait still needs Cancel and the
   mount refusal still needs Back. Plan default: keep the command for the controller's states only (Escape = Cancel,
   Backspace/`⌘↑` = Back), off the palette (`showInPalette: false`), with its display shortcuts and label updated so the
   keyboard settings don't promise "Escape = back to Servers".
10. **The context menu on a share row** (today there is none; after, the standard file menu would open with Copy,
    Delete, Rename, Reveal in Finder, Open with, tags, and so on, all wrong for a share): (a) a share-row menu built
    like the host-row sibling builder in `row-menu.ts`: Open (mounts), Copy path, then the server's account items; (b)
    the standard file menu with every item the kind refuses disabled (one menu, but mostly grey); (c) no menu on a
    share-list row. **Plan default: (a)**, built in M5b with the other account doors and wired in M5c. ❗ Open
    sub-question, the SURFACE: every file-list row menu today is NATIVE (`pane-pointer.ts::handleContextMenu` →
    `showFileContextMenu`, or `showParentRowContextMenu` for `..`, items built in Rust's `cmdr-menu`, with the Windows
    in-page fork inside the `show_*` wrapper), while `row-menu.ts` feeds the house `Menu`. (i) A native
    `showShareRowContextMenu`, a tenth `show_*` wrapper forked in `handleContextMenu` beside the `..` arm: consistent
    with the list's other rows, ⌃⏎'s anchor and Windows come free, but the account items then exist twice (Rust and
    `row-menu.ts`). (ii) The house `Menu` from `row-menu.ts`, opened from the same fork at the pointer or
    `cursorRowAnchor`, hosted OUTSIDE FilePane (cap): `VolumeBreadcrumb`'s ONE `openMenu`, which the account chip uses
    anyway, so `isHeaderMenuOpen()` suppresses pane keys with no new source of truth. **Lean: (ii)**, one vocabulary for
    the account items in all three doors.
11. **The footer band's top edge**: (a) one 2px edge everywhere: a normal pane's usage track, and a plain border-colored
    rule on the hub and the share list; (b) drop the hub's border and let the share list and hub start flush, accepting
    the 2px difference from a folder. **Plan default: (a)**.

## Milestones

Sequential, one agent each. Each reports in ≤350 words: what landed, what deviated from this plan and why, which pins it
added.

### M1: characterize what must survive

- **Scope**: pins at levels that outlive `PlacesBrowser` and `NetworkMountView`: (1) rewrite `smb.spec.ts`'s share-list
  steps to read `cmdr://state` / MCP (`files`, `cursorIndex`, volume id, path) instead of `.share-row` DOM; add specs
  for `..` back to the server's row, #1238 landing, Copy path between panes auto-mount once, the sign-in sheet on an
  auth-required server, and the account label. (2) Rust tests on `commands/network.rs` for the account ladder
  (`account_listing_for`, `guest_attempt_for`) if not already pinned. The report maps each new pin to its coverage
  contract line.
- **Intentions**: the E2E is the proof the user flows survive the swap; it must pass on today's code first.
- **Landmines**: MCP names on today's share list carry `comment="…"` and a `..` at `smb://`; the new mirror must
  reproduce those, so pin them as they are now. Don't pin DOM classes or the header's buttons (they're meant to go).
  Don't pin the pane `path` spelling (`smb://<ip>/` today) or `volumeName` (`Network > <host>`): both change on purpose
  (coverage contract, MCP line). Pin the volume id, `files`, `cursorIndex`, and `..` at 0. Also pin #324's "Servers
  lands on the hub" through `select_volume` from a share list, which branch 12 could silently break.
- **Test plan**: `pnpm check desktop-e2e-playwright` with the SMB specs, green on the current code; run each new spec
  red-first by breaking the behavior locally once.
- **DONE**: specs green on `main`'s behavior, no DOM-class selectors left in the share-list steps.

### M2: extract the two flows into modules that survive (behavior-identical)

- **Scope**: (1) `network/share-list-sign-in.ts`: the listing ladder's sheet round (`askForCredentials`,
  `listWithCredentials`, `signInAs`, `useGuest`, `forgetPassword`) with all side effects, taking a host + label and
  returning outcomes; `PlacesBrowser` calls it. (2) `pane/share-mount.svelte.ts`: `NetworkMountView`'s mount flow
  (`mountShare`, attempt id, Cancel, slow timer, `askForMountCredentials`, `remountWith`, retry, `mountError`,
  `pendingMount` one-shot) as a factory; the two mount panes become `RemoteConnectState` arms. Port the relevant
  `PlacesBrowser.test.ts` / `NetworkMountView.test.ts` cases onto the modules.
- **Intentions**: the components shrink to callers, so the later delete is mechanical and these tests survive it.
- **Landmines**: "one ask at a time" guards; cancel semantics differ per site (listing: back; Sign in as: stay; mount:
  back to the list); `retireAutoMount` must still clear the parent's copy; `mountError` set for auth failures too.
- **Test plan**: moved unit tests green; M1 E2E green; `svelte-tests-related`.
- **DONE**: no flow logic left in `PlacesBrowser` beyond rendering; no mount logic left in `NetworkMountView`.

### M3: one footer height

- **Scope**: `ServersHubStatusBar` and `SelectionInfo` take their height from one source (a shared class in
  `app-file-list.css` or a `--pane-footer-*` token set: padding, font size, min height), and the band's top edge is one
  height too (§ "Decisions", 11).
- **Intentions**: switching between Servers, a share list, and a folder doesn't make the footer jump. "Height" means the
  whole band from the list's bottom edge to the function-key bar, top edge included: a folder's band is the 2px usage
  track plus `SelectionInfo`, the hub's is a 1px border plus its bar, and a share list's (from M5c) is `SelectionInfo`
  with no track (branch 16), so equal body metrics alone still leave a 1–2px jump.
- **Landmines**: the hub bar is a `<button>` with a top border; the SelectionInfo comment explains its horizontal inset;
  keep both, unify the vertical metrics. Stylelint token rules. The usage track is a `role="meter"`: whatever fills the
  top edge where no total exists is decoration (no role, no track color), so it can't read as "0% full".
- **Test plan**: a unit test asserting both use the shared class/tokens; a visual check by screenshot (static state).
  The share-list band can't be seen until M5c, whose DONE re-checks it.
- **DONE**: the hub band and a folder's band measure the same height in light and dark (screenshot pair attached to the
  report).

### M4a: the share-list volume (listing only)

- **Scope**: `SmbServerVolume` registered as `network`; `smb://` → `NotADirectory`; `smb://<server>` → share entries;
  `ShareMeta { description }` on `cmdr_fs::FileEntry` (fact only, no words; specta + bindings regen); the path writer
  (TS) and resolver (Rust) as one tested pair; `list_directory_for_refresh` bypasses the share cache. Not yet reached by
  the frontend.
- **Intentions**: the views can render a share list with no special code.
- **Landmines**: registering `network` in the `VolumeManager` must NOT add a second "Servers" row to `volumes-changed`
  or `cmdr://volumes` (`commands/volumes.rs` synthesizes one; check `volume_listing`), must not arm a change watch or
  the drive index, and must not trip the observers (git arming, restic watch, index sizes) on an `smb://` path. Host
  resolution: case- and port-folding must match the TS writer exactly (`#lib/servers/DETAILS.md` § "The path grammar").
  The share cache is keyed by host id: resolve to the SAME id the hub's prefetch uses, or the hub and the pane cache
  twice. The trait's defaults aren't all "no": `supports_local_fs_access` defaults to `true` (Quick Look, and any
  `std::fs` fast path, would then treat `smb://<server>/<share>` as a local path), so override it; check every other
  default against a read-only, unwatched, unindexed, non-exporting volume and test the answers. If the volume's
  capabilities ever reach `volumes-changed`, the frontend's `withBackendCapabilities` lets them WIN over the kind row,
  so they must agree with `share-list`'s row. Duplicate share names are already uniqued (`with_unique_share_names`).
  Registering the volume makes it reachable by every `VolumeManager` caller from this milestone on (MCP `list_dir` on
  `smb://<server>` works the day this lands): branch 15's gate lands HERE, with a test that a non-pane read neither
  dials nor reads the Keychain. ❗ The Rust app-path grammar reads a scheme root as `scheme://authority` (ONE token,
  `cmdr_fs::app_path::shape`), so `smb://<server>` is itself a ROOT in Rust and `smb://` a root with an empty authority,
  while TS's `isPathOnVolume('smb://x', 'smb://')` says "on the volume". Any engine or host code that checks a listing
  path against the volume's `root()` (`ListingPath::on_volume`, `path_under`, `strip_prefix`, the oracle) may refuse or
  mis-relativize. Prove it with the engine test FIRST; if it fails, fix the path check (an empty-authority root is a
  prefix of every path on its scheme) and say so, ❌ not a special case in `cmdr-listing`. ❌ Not a `root()` that
  answers per listed server: `root()` must stay the constant `smb://`, because `location/facts.rs::RootHistory` records
  every root a volume answers and `location/last_roots.rs::root_move` rebases restored tabs from an "earlier root" to
  the live one, so a varying root would read as the volume MOVING and rewrite `smb://a` tabs to `smb://b`. Pin it: a
  test that `restored_tab_path("network", "smb://<x>")` answers `Keep` after two different servers were listed.
  (Registering `network` also changes that call's timing: today an unregistered `network` waits up to 5 s for the
  startup sweep; after, it answers at once.)
- **Test plan**: Rust unit tests on the resolver (discovered, manual-only, IPv6, port), the entry mapping, refresh
  bypass, and an engine-level test that a `Listings::start` on `smb://<server>` completes with folder rows;
  `pnpm check clippy`, `cargo nextest run` on the new tests.
- **DONE**: a listing of `smb://<fixture server>` returns the fixture's shares through the engine.

### M4b: the volume's sign-in ladder, typed refusal, account, and mount credential

- **Scope**: (1) the ladder inside the volume: cached password, then the typed account's stored password read ONCE per
  session per server (blocking task), then guest per `GuestAttempt`. (2) The refusal type moves to `cmdr-fs` as
  vocabulary (`ShareListRefusal`, `cmdr-smb` re-exports or maps), a `VolumeError` arm carrying it, a `ListingError`
  reason + `sign_in_to_server` action kind for the sign-in classes; FE wording delegates to `renderShareListError`. (3)
  `listing-complete` on a share list carries the account the listing used (`shareParent` precedent). (4) A per-server
  session credential that a typed-not-remembered sheet answer also sets, and `mount_network_share` with no credentials
  uses it.
- **Intentions**: the frontend never holds a password; the backend owns who the listing and the mount sign in as.
- **Landmines**: Keychain-once (a cached miss must not re-read on every ⌘R: remember "read, nothing there" for the
  session; the mount shares that memo, coverage contract "Mounting a share"); never from prefetch (`prefetch_shares`
  keeps its cache-only ladder); `auth_required` vs `auth_failed` stay distinct; `signing_required` is a sign-in class;
  `missing_dependency` keeps its install command; the security-log call (`record_security_refusal`) still fires. The
  session credential must be cleared by Forget saved password and by Forget server, and must not leak into another
  server's mount (key by the same `credential_key`).
- **Test plan**: Rust tests with `InMemoryCredentials` / a counting store proving one read; refusal mapping table test;
  FE parity test that every refusal variant has words.
- **DONE**: a listing of an auth-required fixture answers the typed refusal; with a stored password it lists without a
  second read; a mount with no credentials after a typed sign-in goes through.

### M5a: the `share-list` kind and the navigation it needs (not yet reached)

- **Scope**: the `share-list` kind and its route (gated on `volumeId === 'network'`); branches 11–14 (the `share-list`
  parent arm in `FilePane.navigateToParent`, the MCP `..` path, the remembered-path rule, both halves of the capability
  audit, the tab label), branch 16 (the footer), and branch 2's verb audit with its refusal arms (pane-side writes,
  destination guard, the one new source guard, MCP `rename`, `supports_local_fs_access`); branch 10's narrowed rule as a
  tested pure function, ❗ NOT yet wired into `navigateToLocation` / `navigateInPlace` / MCP `nav_to_path`; the
  `share-list-path.ts` writer/parser pair; branch 3's derived `landOnServer` and its hub consumer; `paneViewKind`
  routing `share-list` to the list and `network` to `NetworkMountView`; `pane-mcp-sync` skipping only the `network`
  kind; restore normalization per § "Decisions", 1. Nothing opens a share-list path yet, so `main` behaves as before. ❗
  That holds only while both refusals stand: lifting them here would let go-to-path, a link, or MCP `nav_to_path` open a
  share list whose Enter navigates (refused) instead of mounting and whose sign-in refusal has no button, on `main`. M5c
  wires the rule in the same commit as the entry-point switch.
- **Intentions**: every generic seam knows the kind before any entry point sends a pane there, so M5c is a switch, not a
  hunt.
- **Landmines**: the audit list in branch 13 is a starting point; `rg 'capabilitiesFor\('` is the real one, and each
  kept volume-only call says why in its report line. The navigation refusal for `smb://<server>/<share>` stays (and its
  MCP wording). The new refusal words are new keys (no existing kind's words fit "a share list").
- **Restore, checked against the code (`f08cd856bb`)**:
  - The choke point is `app-status-store.ts::restoreTabLocation`: every persisted tab passes through it, active and
    background, and so does `loadPaneTabs`'s scalar-key migration. Normalize there, FIRST (before
    `restoreSnapshotLocation` / `followMovedRoot`), so `live_volume_path` is never asked about an `smb://<server>` path.
  - `loadAppStatus`'s `leftPath`/`rightPath` arm (the `volumeId === 'network'` pass-through) feeds no pane today (only
    the first-run layout writes those keys); normalize it with the same helper for symmetry, ❌ don't rely on it.
  - The carrier: `LoadedPaneTabs` gains `landOnServerByTabId` beside `registryPendingTabIds`, and
    `pane/initialization.ts` carries it past `toPersistedTab`'s strip the way it carries `unreachableByTabId`. The tab
    manager holds it as a one-shot on the tab, consumed by the hub's first render of THAT tab (the active tab at launch;
    a background tab when first activated) and dropped by any navigation before then. ❌ Not the tab's `cursorFilename`
    (a file-row name, cleared and rewritten on every switch).
  - In-session tabs need no carrier: history keeps the share-list path, and branch 3 derives the landing on the way out.
  - Pins: active tab restored onto a share list → hub, cursor on the server, no listing started; a background tab
    activated later → same; the migration arm → same; an unknown host key → hub with the cursor where it would be.
- **Test plan**: unit tests per branch (kind table, navigate refusals, parent arm, `last-used-path` not recorded,
  `has-parent`, `pane-mcp-sync` `..` path, tab label, `paneFooterVisibility` for the kind, both restore arms); one
  refusal test per verb in branch 2's list (pane-side, destination incl. a share-row drop, source, MCP typed);
  `svelte-tests-related`.
- **DONE**: in a FilePane component test (mocked listing IPC; ❌ not E2E, since the navigation refusals still stand), a
  pane whose tab starts on `network` + `smb://<fixture>` renders the share list, `..` lands on the hub with the cursor
  on that server's row, and Servers in the switcher still lands on the hub; the restore pins above are green.

### M5b: the three account doors (before anything that deletes the header)

- **Scope**: SMB host rows in the hub get the house `Menu` (a host-keyed sibling builder in `row-menu.ts`: Open, Edit…,
  Disconnect, Forget saved password, Forget server, Sign in as…, Browse as guest, as they apply) replacing the native
  menu (delete `show_network_host_context_menu`, its event, the in-page twin, and `checkCredentialsForHost`); the path
  bar's "as <account>" as a button opening a small menu with Sign in as…, Browse as guest, Forget saved password (built
  and tested on a pane a test puts on a share list, visible in the app from M5c); two palette commands (`COMMAND_IDS`,
  registry, handlers); the share-row menu builder (§ "Decisions", 10), built and tested here, wired to
  `file.contextMenu` on a share-list pane in M5c.
- **Intentions**: one action vocabulary, one runner per action (`share-list-sign-in.ts`), three doors, all live before
  M5c deletes the header's buttons. Until M5c the header keeps its buttons too; that brief overlap is the price of a
  `main` that never loses Sign in as… or Browse as guest.
- **Landmines**: row identity by `rowId` end to end (QA 2026-09-25); the menu reads live getters, not snapshots; the
  account chip is pane chrome (`pane-chrome.ts` focus rules, no roving tabindex); ⌃⏎ on a host row opens the same menu.
  ❗ `server-command-target.ts`'s rule is "the hub's cursor row wherever there is one, the pane's own volume otherwise",
  and its target is a `volumeId`, which an SMB host has none of (it stops at a host row today). The palette commands
  follow the same rule: the hub's cursor SMB host row, else the share-list pane's server, else disabled; so the target
  gains a host arm (a discriminated union, ❌ not a fake `volumeId`). On a share-list pane the server comes from
  `getFocusedPaneNetworkHost` until M5c and from the path after. A row-menu Sign in as… or Browse as guest that succeeds
  opens that server's share list (coverage contract, "Account actions").
- **Landmines in `row-menu.ts` itself (checked `f08cd856bb`)**: its shapes assume a volume. `RowActionEntry.action` is
  `VolumeContextActionKind`, a Rust-generated enum (bindings) for the native menus, which has no Sign in as… or Browse
  as guest; `rowMenuSections`/`rowMenuItems`/`entryItem` key every item value by `volumeId` (`row:<volumeId>:…`);
  `RowMenuPick` carries a `VolumeInfo`; and `ServersHubRowMenu.svelte`'s `getSections` draws nothing for a row without
  `volumeId` (that's where it falls to `openHostMenu`). So: the account items get their own TS kind
  (`ServerAccountAction` = `sign-in-as` | `browse-as-guest` | `forget-secret`, ❌ not new Rust enum members for a menu
  Rust never draws), built by ONE `serverAccountEntries(host, facts)` that the hub host row, the path-bar account menu,
  and the share-row menu all call; the section helpers take a row key (`rowId` for hub rows) instead of a `volumeId`;
  and `ServersHubRowMenu.open` loses its native fallback. Precedent for a non-volume row menu:
  `servers-hub-actions.ts::shareMenu` (hub share rows).
- **The palette**: two new commands (Sign in as…, Browse as guest), plus the host arm in the existing `servers.edit`,
  `servers.forgetSecret`, and `servers.disconnect` (`disconnectHost`, which unmounts the host's shares), ❌ not
  `servers.togglePin` / `servers.toggleShareParentRow` (a host has neither). `servers.edit` already resolves a
  share-list pane through `getFocusedPaneNetworkHost`; fold it into the same target union so M6's deletion is one edit.
- **The path-bar account menu** is a third member of `VolumeBreadcrumb`'s ONE `openMenu` (`navigation/CLAUDE.md`), ❌
  not a second `Menu` instance: `isHeaderMenuOpen()` is the single answer the panes suppress keys on.
- **Test plan**: `row-menu` tests for the SMB host set, a path-bar chip test, palette availability tests (hub host row,
  share-list pane, neither), a test that opening the row menu reads no Keychain, E2E for Sign in as from the row menu.
- **DONE**: no native network-host menu left; all three doors run the same functions; no right-click reads a password.

### M5c: every door opens the share list as a listing

- **Scope**: EVERY entry point that opens a server's shares switches to `shareListPath` in this one milestone: the hub
  (host row, "More shares on…", `onShareViaHost`), the add-server hand-off, go-to-path / links to `smb://host/share`,
  `pane-mirror.ts`'s network arm (share list + `pendingMount`), `share-parent-pane.svelte.ts` (#1238: path + select-name
  landing, hint on landing), history Back/Forward, and branch 10's rule wired into the two navigation refusals and MCP
  `nav_to_path`. Then: Enter on a share → share-mount (its FilePane arm, its key-router arm); `..` → hub with
  `landOnServer`; the description cell (`extCellText` + header label); SelectionInfo noun; `ErrorPane` sign-in action
  (branch 5) + the user-intent auto-ask factory; empty state words; the standard MCP mirror for the kind (with the
  `comment="…"` token and the mount error); the `..` accessible name; the palette and Edit server reading the server
  from the path; the share-row context menu; delete `PlacesBrowser.svelte`, `PlacesHeader.svelte`, their tests,
  `NetworkMountView`'s share-list branch, and the frontend's `getSmbCredentials`. Retire `share.selectShare` (Enter is
  the list's own open now): its `COMMAND_IDS` entry, registry row, handler, and keyboard-settings row; check that a
  saved custom binding for a retired id is dropped quietly on load (pin it if nothing does).
- **Rollback (state written from here on, read by an older build)**: this is the first milestone that can persist a
  `network` tab at `smb://<server>`. Check, by reading those paths at `f08cd856bb` and running that build once on a
  state file this milestone wrote, that an older `loadPaneTabs` + `initialization.ts` (trusts the stored `network` id,
  `resolveTabPath` passes it through) + `NetworkMountView` (no `initialNetworkHost`) shows the hub and lists nothing,
  and that `navigate.ts`'s drop-foreign-listings prefix (`smb://`) and the `SMB_PATH_REFUSAL` don't wedge it. Say so in
  the report.
- **Intentions**: every M1 pin green with the new rendering; the views untouched apart from the cell rule. ❗ The
  deletion and the entry-point switch land together: deleting `PlacesBrowser` while any caller still sets a
  `networkHost` would leave that door opening a blank pane on `main`.
- **Landmines**: hasParent offset (index 0 is `..`) everywhere the share-mount reads the cursor entry; retained rows are
  paint-only (Enter must not mount the old listing's share); the drop-foreign-listings rule already accepts `smb://` on
  `network`; FilePane's size cap; the one-shot rule (cmdr-reports#7) across every entry.
- **Test plan**: M1 E2E (macOS and Linux), axe on a share-list pane, ported `pane-mirror` / `share-parent-pane` /
  `navigate` tests, the pinned-tab case (hub on a pinned tab → share list opens a new tab), an E2E that F5 and a drop
  onto a share row refuse on a live share list, the share-row menu by right-click AND ⌃⏎ (same items, anchored at the
  row for the key), `svelte-tests-related`, `clippy`.
- **DONE**: `PlacesBrowser` and `PlacesHeader` are gone; no code path calls `setNetworkHost` with a non-null host; the
  coverage contract's "Opening", "The list itself", and "Signing in to list" sections all point at green pins; a
  screenshot shows the share list's footer band at the hub's height with no usage bar.

### M6: retire the host plumbing

- **Scope**: now-dead code only. The full reader set (checked 2026-11-25; re-run the grep, it may have grown):
  - `pane/network-host-state.svelte.ts` and its test (host and `shareLanding` go; `pendingMount` lives in share-mount).
  - FilePane: `initialNetworkHost` / `onNetworkHostChange` props, `setNetworkHost`, `setNetworkAutoMount`,
    `getNetworkHost`, the host arm of `navigateToParent`, and the quick-filter reset effect's
    `dependOn(listingId, networkHost.host)` (the share list's own listing id now covers it; keep the reset).
  - DualPaneExplorer: `handleNetworkHostChange`, the `onNetworkHostChange` wiring, `getFocusedPaneNetworkHost`, and the
    `networkHost` passed through `onVolumeChange`; `pane/types.ts`'s `VolumeChangePayload.networkHost`;
    `explorer-api.ts`'s `getFocusedPaneNetworkHost` and its entry in `command-dispatch.test-harness.ts`.
  - History: `HistoryEntry.networkHost`, its equality arm in `navigation-history.ts`, `navigate.ts` (switch arm, Back/
    Forward restore), `navigate-return.ts`, `navigate-commit.ts`'s `selectVolume.networkHost` and push-entry field,
    `navigate.test-fixtures.ts`'s `setNetworkHost`, and `routes/debug/DebugHistoryPanel.svelte`'s host column.
  - `pane-mirror.ts`'s `showNetworkState(host, autoMountShare)` and its read of `sourceHistoryEntry.networkHost` (M5c
    moved the behavior to the path; this deletes the leftovers).
  - Docs: `network/CLAUDE.md`'s `onNetworkHostChange` must-know, `navigation/DETAILS.md`'s `HistoryEntry` shape,
    `servers/DETAILS.md`'s `getFocusedPaneNetworkHost` line, `pane/DETAILS.md`.
- **Intentions**: the path is the only record of which server a pane shows.
- **Landmines**: the stale-host bug the `onNetworkHostChange` must-know guarded (now impossible by construction: say so
  in the docs and drop the must-know). No migration: `HistoryEntry` is never persisted, and persisted tabs,
  `leftPath`/`rightPath`, and `last-used-path` have only ever held `smb://` for `network`, which restores to the hub as
  before. The reverse direction (a rollback) is M5c's check, since M5c is the first writer of `smb://<server>`.
- **Test plan**: ported `navigate` / `navigation-history` / `pane-mirror` tests (the `network-host-state` tests go with
  their module); E2E.
- **DONE**:
  `rg -w 'networkHost|initialNetworkHost|onNetworkHostChange|setNetworkHost|getNetworkHost|getFocusedPaneNetworkHost|autoMountShare' apps/desktop/src`
  finds nothing outside discovery types (the `networkHostFound`/`Lost`/`Resolved` events don't match `-w`), and every M1
  pin is green.

### M7: close-out

- **Scope**: dead i18n keys (the header's back arrow, connecting sentence, share-list error titles if unused), `knip`,
  the full SMB E2E on macOS and Linux, `pnpm check --only-slow`, docs audit of every touched C+D.md, walk the coverage
  contract and confirm each line's pin.
- **DONE**: the conformance review against the invariants below finds nothing; this spec is ready to wipe per
  `docs/specs/DETAILS.md`.

## Invariants (the conformance register)

1. A server's shares are listed only by a pane the person pointed at `smb://<server>`; prefetch, stale refresh, and the
   hub's ⌘R keep their saved-host gate; nothing lists at launch; selecting Servers (switcher, MCP `select_volume`)
   always lands on the hub, never on a remembered share list; a non-pane reader of the volume (MCP `list_dir`, About,
   sizes) never dials or reads the Keychain (branch 15).
2. The sign-in sheet auto-opens only for a listing or mount the person started; nothing a tab switch, a repaint, or a
   background re-list does opens it.
3. The Keychain is read at most once per server per session, shared by the listing and the mount (one memo), never from
   a background path, and "is a password stored?" is answered from the cache only.
4. The frontend never holds an SMB password after the sheet hands it over; mounts with no credentials use the server's
   session credential.
5. One sign-in sheet; every share-list and mount credential question goes through `share-list-sign-in.ts` or the
   share-mount controller; no in-pane form.
6. `capabilitiesForPane` is the only place that reads `smb://<server>` as a share list; the listing engine has no
   share-specific code, and the views and `SelectionInfo` have none beyond the cell rule: the Ext header label, the
   count noun, and the `..` name reach them as generic props FilePane fills from the kind, ❌ never a kind check inside
   a view.
7. A queued share mount fires at most once, only within the visit it was queued for.
8. `..` from a share list lands the hub cursor on that server's row; `..` out of a share's top folder lands the share
   list cursor on that share.
9. The share list's MCP mirror is the standard one: `..` at index 0, indices match the screen, the volume id is
   `network`.
10. Every share-list and mount refusal is a typed value worded on the frontend; nothing string-matches a message.
11. The Servers footer and SelectionInfo share one height source, top edge included; a share list's footer shows no disk
    space.
12. No file operation takes a share list as source or destination: every verb in branch 2 refuses at its per-kind seam
    with words (typed for MCP) before any dialog opens or any backend write starts.
