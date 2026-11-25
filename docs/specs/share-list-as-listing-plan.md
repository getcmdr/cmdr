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
  milestone from M5 on, and Linux E2E on M5 and M8.

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
- **Footers**: `ServersHubStatusBar` pads `--spacing-xs --spacing-sm` with a 1px top border; `SelectionInfo` pads
  `--spacing-sm --spacing-md`, no border. Their heights differ.
- **Precedents to reuse**: per-volume entry facts on `FileEntry` (`git_meta`, `restic_meta`, worded on the FE);
  `extCellText` as the one Ext-cell rule (restic puts its host there); `VolumeError::LinkNeedsConsent` as a typed
  listing refusal that `ErrorPane` turns into an action; `capabilitiesForPane` path routing (archive, git portal,
  restic); `listing-complete` carrying extra facts (`shareParent`); `has-parent.ts`'s `topFolderLeadsOut`.

## Fit check

**Verdict: it fits cleanly, with a provider behind the existing `Volume` trait and a path-routed pane kind. The listing
engine needs no change at all.** The share list is a flat, read-only, non-watched directory of folders, which is the
easiest shape `cmdr-listing` serves. All share-specific behavior lands on seams that already branch per kind.

**The shape.**

- A Rust **`SmbServerVolume`** (app side, `src-tauri/src/network/share_list_volume.rs`; it needs discovery, manual
  servers, and the Keychain, so it can't live in `cmdr-smb`) registered in the `VolumeManager` under the id `network`.
  `list_directory(smb://)` answers `NotADirectory` (the hub never lists; a stray read gets a typed no).
  `list_directory(smb://<server>)` resolves the server, runs the account ladder, and returns one directory `FileEntry`
  per share: name = share name, path = `smb://<server>/<share>`, `share_meta: Some(ShareMeta { description })`.
  `list_directory_for_refresh` bypasses the share cache (⌘R on a share list re-reads, as the hub's ⌘R does today). No
  watcher (nothing to watch), not indexable, not writable.
- **Path identity**: `smb://<host>[:<port>]`, `host` the server's stable name (the resolved hostname, else the Bonjour
  name, lowercased per the path grammar), the port only off 445. ONE TS writer (`shareListPath(host)`, beside
  `server-path-utils.ts`) and ONE Rust reader (resolve to a `NetworkHost` via discovery by name/hostname/IP, else the
  manual servers, the way `smb_share_parent.rs` already makes a host from a mount's server). ❌ Not the IP: DHCP drift
  would break history and the restore normalization.
- A **routed pane kind `share-list`** in `volume-capabilities.ts`: `capabilitiesForPane('network', 'smb://<x>')` answers
  it (`hasBackendListing: true`, `syncsToMcp: true`, `hasParentRow: true`, `canWrite`/`canBeSource`/
  `canBeIndexed`/`canShareLinks`/favorites `false`); `smb://` stays the `network` (hub) kind. Since the network volume's
  root is `smb://`, `computeHasParent` gives the share list a `..` with no new rule.

**Every share-specific branch, named** (all at existing per-kind seams):

1. `volume-capabilities.ts`: the `share-list` kind and its routing (the one path read).
2. The pane's open path (Enter, double-click, `openCursorItem`, MCP `open_under_cursor`): a folder on a `share-list`
   pane MOUNTS (the share-mount controller) instead of navigating. One branch where the pane already forks archives.
3. `..` on a share list navigates to `smb://`, which is the hub; the hub takes a `landOnServer` (the share list's path)
   and puts its cursor on that server's row. `navigate.ts` already special-cases `network`; this replaces
   `setNetworkHost` there, it doesn't add a branch.
4. The description cell: `extCellText` returns `shareMeta.description` (restic precedent), and `FullListHeader` labels
   the track "Description" on that kind (§ "Decisions", 2). Renderer and measurer stay one contract.
5. `ErrorPane` / `EmptyFolderState`: a typed share-list refusal carries a `sign_in_to_server` action kind; the empty
   state on a `share-list` pane gets its own words and a Sign in action (via `empty-folder-text.ts`).
6. `SelectionInfo`'s count noun: "8 shares" on a `share-list` pane (the noun comes from the kind, like the snapshot
   pane's).
7. The path bar: on `share-list`, the display path is the server's friendly label (not the host key), followed by a
   muted, clickable "as <account>".
8. Restore and history: a persisted `share-list` path restores to `smb://` with `landOnServer` (§ "Decisions", 1).
9. `pane-mirror.ts` (Copy path between panes) and the MCP mirror: a share under the cursor mirrors as "open the share
   list, then mount that share" (today's behavior), ❌ not as navigating to the share's `smb://` path.

Nine named branches, each at a seam that already branches per kind; zero in the engine and the two views' row code.

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
that touches a flow keeps its pin green; M8 walks this list.

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
  operation accepts a share-list source (`canBeSource: false`), so F5/F6/F8 refuse with the existing kind refusal.
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
- Footer: share count in the header ("8 shares") → AFTER: `SelectionInfo` "8 shares" (branch 6).
- Path bar: was the header's host label + "as guest". AFTER: `Servers › <server> as <account>` (branch 7).
- Back button in the header → AFTER: gone; `..` and the volume chip cover it.

**Signing in to list**

- Cached share state hit (prefetch or earlier visit) → AFTER: `smb_client` cache inside the volume, same TTL.
- Cached `auth_required` / `signing_required` → stored creds silently → sheet. AFTER: the volume's ladder (cached
  password → typed account's stored password read ONCE → guest per `GuestAttempt`) answers a typed refusal; the pane
  auto-opens the sheet ONCE per listing on a sign-in refusal (today's behavior), and the ErrorPane under it is the calm
  sign-in state with Sign in and (via `..`) back.
- Sheet attempt `listWithCredentials`: `handed_off` on success, `refused` keeps the sheet open on
  `authentication_rejected` / `needs_credentials`, any other error closes onto the error state. AFTER: identical, in
  `share-list-sign-in.ts`; success re-lists the pane (`refresh`), and keeps the side effects: `setShareState`,
  `setCredentialStatus`, `setListedAccount`, `setSmbAccountPreference`, `saveSmbCredentials` when Remember, the
  credential-file-fallback toast, `updateKnownShare`.
- Cancel on the auto-opened sheet → back to Servers (cursor on the server). AFTER: same, through `nav.parent`.
- `guestAllowed` per the listing's `authMode`; "One ask at a time" (`signingIn` guard). AFTER: same, in the module.
- Error state (non-auth): red title "Couldn't connect to {host}", sentence from `renderShareListError`, Retry, Sign in,
  Back; `missing_dependency` shows the install command in a `CopyBox` and no Sign in. AFTER: `ErrorPane` with the typed
  share-list refusal: words via `renderShareListError` (single source), Retry = pane refresh, Sign in action, `..` for
  back; the install command keeps its copy box (verify `ErrorPane` has one; else add it there, not in a share view).
- Empty state ("No shares", Sign in, Back) → AFTER: `EmptyFolderState` with the share-list words and a Sign in action.
- MCP while the sheet is up: `getItemCount() === 0`, `getShareUnderCursor() === null`. AFTER: the pane's normal
  modal-dialog key suppression; MCP reports the listing (the sheet is a dialog the `ui_state` mirror already reports).

**Account actions**

- "Sign in as…" (header): sheet with no refusal, prefilled with the listed account, no guest option, cancel stays.
  AFTER: three doors (row menu, path-bar account menu, palette) → one `signInAs(host)` in `share-list-sign-in.ts`.
- "Browse as guest" (header, only when the account is a user and a guest listing worked): sets the account preference to
  none first, then lists as guest. AFTER: same three doors, shown under the same condition (§ "Decisions", 3).
- "Forget saved password" (header, when this session read one: `authenticatedCredentials` or `has_creds`, from the
  cache-only `noteCachedCredentials`): forget + success/failure toast. AFTER: the three doors; visibility asks
  `has_cached_smb_credentials` (cache only, no Keychain) when the menu opens.
- The hub's native SMB host menu (Edit, Disconnect, Forget saved password, Forget server) and ⌃⏎ → AFTER: the house
  `Menu` from `row-menu.ts`, same items as SFTP/WebDAV server rows where they apply (Open, Edit…, Disconnect, Forget
  saved password, Forget server, the pin where it applies) plus Sign in as… and Browse as guest; row identity by `rowId`
  end to end, as today.
- The listed account labels ("as testuser" / "as guest") on the hub row and the share list header must agree
  (`signed-in-as.ts`). AFTER: the path bar reads the same `getListedAccount`; `listing-complete` on a share list carries
  the account the listing used, and the pane notes it into the store.

**Mounting a share**

- Enter on a share: stored-creds attempt when `authMode === 'creds_required'` and none in memory, then
  `mountNetworkShare` with whatever it has; already mounted short-circuits. AFTER: the frontend holds no credentials;
  `mount_network_share` with no credentials uses the server's session credential (the one the listing signed in with,
  including a typed-not-remembered one, § M4b). Keychain-once preserved.
- The wait: `RemoteConnectView` "connecting", share + friendly server + `host:port`, Cancel (and `share.back` as
  Cancel), slow hint at 10 s, a requested cancel returns quietly. AFTER: same view, owned by `share-mount.svelte.ts`.
- Mount `auth_required` → calm pane + sheet (no guest, prefilled, `refusalForMountError`); `auth_failed` /
  `permission_denied` → sheet over the red pane; `remountWith` keeps the sheet open only on a credential refusal; a
  working account becomes the host's account and is saved when Remember. AFTER: same logic in the controller; the two
  panes become `RemoteConnectState` arms (`signed_out` for `auth_required`; `refused` with `retry` + a `back` for the
  rest; add `back` to `refused` if missing, ❌ no bespoke pane).
- Mount error MCP mirror (`mountError: { share, reason, message }`). AFTER: the share-list pane's standard mirror
  carries the controller's `mountError`.
- Success: lands on the mounted volume (`resolvePathVolume`), the saved-share bookkeeping (`remember_mount_since`), the
  OS-mount fallback notice. AFTER: unchanged (Rust side untouched).
- Keys during a mount or a mount error: only `share.back` (Cancel / Back); the rest dead. AFTER: the controller claims
  `share.back` the same way while its state is up.

**Elsewhere, unchanged but must keep working**

- The direct-connect upgrade (`connectDirectly`, chip yellow dot, switcher switch, fallback notice): not touched; pin
  stays `direct-connect.test.ts` + E2E.
- #1238 `..` out of a share's top folder into its server's share list, landing on that share, and the hint toast: AFTER:
  `share-parent-pane` navigates to `shareListPath` with the select-name landing.
- Tabs and history: each tab keeps its own share list now (the path carries the host); Back/Forward walk share lists as
  normal path history. Persistence: § "Decisions", 1.
- MCP: `move_cursor` / `open_under_cursor` / `cmdr://state` on a share list → the standard pane mirror (volume id
  `network`, path `smb://<server>`, `..` at 0, shares as directories with the description token kept in the name,
  `comment="…"`, so agents and `smb.spec.ts` keep reading it); `select_volume` onto Servers still lands on the hub.
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
  `app-file-list.css` or a `--pane-footer-*` token set: padding, font size, min height, and the border accounted for).
- **Intentions**: switching between Servers and a share list or folder doesn't make the footer jump.
- **Landmines**: the hub bar is a `<button>` with a top border; the SelectionInfo comment explains its horizontal inset;
  keep both, unify the vertical metrics. Stylelint token rules.
- **Test plan**: a unit test asserting both use the shared class/tokens; a visual check by screenshot (static state).
- **DONE**: both bars measure the same height in light and dark (screenshot pair attached to the report).

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
  twice. Duplicate share names are already uniqued (`with_unique_share_names`).
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
  session); never from prefetch (`prefetch_shares` keeps its cache-only ladder); `auth_required` vs `auth_failed` stay
  distinct; `signing_required` is a sign-in class; `missing_dependency` keeps its install command; the security-log call
  (`record_security_refusal`) still fires. The session credential must be cleared by Forget saved password and by Forget
  server, and must not leak into another server's mount (key by the same `credential_key`).
- **Test plan**: Rust tests with `InMemoryCredentials` / a counting store proving one read; refusal mapping table test;
  FE parity test that every refusal variant has words.
- **DONE**: a listing of an auth-required fixture answers the typed refusal; with a stored password it lists without a
  second read; a mount with no credentials after a typed sign-in goes through.

### M5: the pane renders the share list as a listing

- **Scope**: the `share-list` kind; the hub opens a host by navigating to its path; Enter on a share → share-mount; `..`
  → hub with `landOnServer`; the description cell (`extCellText` + header label); SelectionInfo noun; `ErrorPane`
  sign-in action + the once-per-listing auto-ask; empty state words + Sign in; the standard MCP mirror for the kind
  (with the `comment="…"` token and the mount error); the `..` accessible name; delete `PlacesBrowser.svelte`,
  `PlacesHeader.svelte`, their tests, and `NetworkMountView`'s share-list branch.
- **Intentions**: every M1 pin green with the new rendering; the views untouched apart from the cell rule.
- **Landmines**: hasParent offset (index 0 is `..`) everywhere the share-mount reads the cursor entry; retained rows are
  paint-only (Enter must not mount the old listing's share); the drop-foreign-listings rule already accepts `smb://` on
  `network`; FilePane's size cap; the hub keeps its own MCP push and `pane-mcp-sync` must skip only the `network` kind,
  not `share-list`; `paneViewKind` must route `share-list` to the list, `network` to `NetworkMountView`.
- **Test plan**: M1 E2E (macOS and Linux), axe on a share-list pane, the pane's existing kind/capability tables
  extended, `svelte-tests-related`, `clippy`.
- **DONE**: `PlacesBrowser` and `PlacesHeader` are gone; the coverage contract's "The list itself" and "Signing in to
  list" sections all point at green pins.

### M6: retire the host plumbing

- **Scope**: `network-host-state.svelte.ts` (host and `shareLanding` go; `pendingMount` lives in share-mount),
  `NavigationHistory.networkHost`, `setNetworkHost` / `onNetworkHostChange` / `initialNetworkHost`, `navigate*.ts`
  network host branches, `pane-mirror.ts`'s network arm (share under cursor → share list + `pendingMount`),
  `share-parent-pane.svelte.ts` (path + select-name landing, hint on landing), the add-server hand-off, and restore
  normalization per § "Decisions", 1.
- **Intentions**: the path is the only record of which server a pane shows.
- **Landmines**: the one-shot rule (cmdr-reports#7) across every entry; the stale-host bug the `onNetworkHostChange`
  must-know guarded (now impossible by construction: say so in the docs and drop the must-know); history entries from
  before the change exist only in memory, so no migration, but the persisted path on a share list must normalize.
- **Test plan**: ported `network-host-state` / `pane-mirror` / `share-parent-pane` / `navigate` tests; E2E.
- **DONE**: `rg networkHost apps/desktop/src` finds only discovery types, and every M1 pin is green.

### M7: the three account doors

- **Scope**: SMB host rows in the hub get the house `Menu` via `row-menu.ts` (Open, Edit…, Disconnect, Forget saved
  password, Forget server, Sign in as…, Browse as guest, as they apply) replacing the native menu (delete
  `show_network_host_context_menu`, its event, and the in-page twin); the path bar's "as <account>" becomes a button
  opening a small menu with Sign in as…, Browse as guest, Forget saved password; two palette commands (`COMMAND_IDS`,
  registry, handlers) enabled while the focused pane is on a share list, targeting that server.
- **Intentions**: one action vocabulary, one runner per action (`share-list-sign-in.ts`), three doors.
- **Landmines**: row identity by `rowId` end to end (QA 2026-09-25); the menu reads live getters, not snapshots;
  `server-command-target.ts` owns "which server a command acts on" and needs the share-list arm; the account chip is
  pane chrome (`pane-chrome.ts` focus rules, no roving tabindex); ⌃⏎ on a host row opens the same menu.
- **Test plan**: `row-menu` tests for the SMB host set, a path-bar chip test, palette command availability tests, E2E
  for Sign in as from the row menu.
- **DONE**: no native network-host menu left; all three doors run the same functions.

### M8: close-out

- **Scope**: dead i18n keys (the header's back arrow, connecting sentence, share-list error titles if unused), `knip`,
  the full SMB E2E on macOS and Linux, `pnpm check --only-slow`, docs audit of every touched C+D.md, walk the coverage
  contract and confirm each line's pin.
- **DONE**: the conformance review against the invariants below finds nothing; this spec is ready to wipe per
  `docs/specs/DETAILS.md`.

## Invariants (the conformance register)

1. A server's shares are listed only by a pane the person pointed at `smb://<server>`; prefetch, stale refresh, and the
   hub's ⌘R keep their saved-host gate; nothing lists at launch.
2. The Keychain is read at most once per server per session for a listing, never from a background path, and "is a
   password stored?" is answered from the cache only.
3. The frontend never holds an SMB password after the sheet hands it over; mounts with no credentials use the server's
   session credential.
4. One sign-in sheet; every share-list and mount credential question goes through `share-list-sign-in.ts` or the
   share-mount controller; no in-pane form.
5. `capabilitiesForPane` is the only place that reads `smb://<server>` as a share list; views, `SelectionInfo`, and the
   listing engine have no share-specific code beyond the cell rule.
6. A queued share mount fires at most once, only within the visit it was queued for.
7. `..` from a share list lands the hub cursor on that server's row; `..` out of a share's top folder lands the share
   list cursor on that share.
8. The share list's MCP mirror is the standard one: `..` at index 0, indices match the screen, the volume id is
   `network`.
9. Every share-list and mount refusal is a typed value worded on the frontend; nothing string-matches a message.
10. The Servers footer and SelectionInfo share one height source.
