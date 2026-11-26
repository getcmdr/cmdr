# Network browser

SMB discovery and browsing: the servers hub, a server's share list, the direct-connect upgrade, and the reconnect
manager. Signing in is `#lib/servers`' one sheet; this module only says what to ask and what to do with the answer.

## Module map

- `network-store.svelte.ts`: the `$state` singleton behind all network data. `lazy-trigger.ts`: the network-action hook.
- `ServersHub.svelte` (+ `servers-hub-{rows,items,shares,keys,actions,mcp,columns}.ts`, `servers-hub-list.svelte.ts`,
  `host-status.ts`, `ServersHubRowMenu.svelte`, `ServersHubStatusBar.svelte`): the hub table on the `network` volume.
  `share-list-sign-in.ts`: a share list's sign-in and every door's account actions. `ShareListAccountChip.svelte`: the
  path bar's account and a share row's menu. `share-list-label.ts`: a share-list path's friendly name. `smb-sign-in.ts`:
  SMB's side of the sheet. `list-cursor.ts`: the hub's arrow keys, jump rule, and scrolling. `server-row-words.ts`,
  `server-row-of-entry.ts`, `ServersListRowMenu.svelte`: the Servers listing (DETAILS). `mount-error-messages.ts` (+
  `mount-error.ts`): a refused mount's words. `share-parent-*`: `..` out of a share's top folder (DETAILS).
- `SmbLinkConsentDialog.svelte`: the ask before a link signs in to another server, opened from the pane's error screen
  (`crates/cmdr-smb/DETAILS.md` § "Signing in through a link").
- `direct-connect.ts` (+ `upgrade-messages.ts`, `LocalNetworkBlockedToastContent.svelte`): the "Connect directly"
  upgrade. `os-mount-notice-bridge.ts` + `SmbOsMountFallbackToastContent.svelte`: the slow-connection notice.
  `smb-reconnect-manager.svelte.ts`: the per-volume backoff cycle on `volume-connection-changed` (backend-neutral; SMB
  first).

## Must-knows

- **❌ Never import raw `$state` from `network-store.svelte.ts`; use the exported getters.** Svelte 5 `$state` is
  reactive only inside `.svelte` / `.svelte.ts`: a plain `.ts` import silently loses it.
- **`triggerNetworkDiscovery()` is the single chokepoint for a networking intent**; ❌ don't gate on `network.enabled`
  yourself. The browse runs while a Servers view holds it (`holdDiscoveryForServersView`).
- **❌ Never list the shares of a host Cmdr only found, unless the person opens it** (#324): listing signs in to it.
  Prefetch, the stale refresh, and ⌘R all gate on `savedSmbHostIds`; a new `listSharesOnHost` caller must too. Even a
  saved server's prefetch waits for a Servers view on screen: nothing is listed at launch.
- **The hub's cursor counts what's on screen; MCP indexes the full list** (a collapsed nearby group hides rows). Cross
  with `servers-hub-items.ts`, ❌ never index `rows` by the cursor. DETAILS § "The nearby group".
- **❌ The frontend never reads the Keychain**: each access can raise a system prompt, so the backend reads a server's
  stored password at most once per session, inside a listing or a mount the person started
  (`src-tauri/src/network/share_list_sign_in.rs`). "Is one stored?" for a button reads the backend's in-memory cache
  only (`noteCachedCredentials`); a menu never asks it.
- **`direct-connect.ts::connectDirectly` is the ONE upgrade flow** (the chip's yellow dot, the switcher's
  direct-connection switch, the fallback notice): route a new entry point through it. Every answer is a typed
  `UpgradeResult` status worded by `upgrade-messages.ts`; ❌ never toast `String(e)`.
- **A credential is asked for on the sheet and ❌ never in the pane**, through `smb-sign-in.ts`. The three sites, what
  each `attempt` runs, and where cancelling lands: `../../servers/DETAILS.md`.
- **"as testuser" / "as guest" comes only from what's known** (`signed-in-as.ts`), ❌ never a Keychain read. A SHARE
  row: its live mount's account. A SERVER row and its share list's path-bar account: the same server-level account (the
  listing's, `getListedAccount`, else the one it's set to be used with), ❌ never a share's mount.
- **Credential status is keyed by lowercase `host.name`** (the stable Bonjour name); IP and hostname both drift.
- **On the `network` volume, `smb://` is the Servers list and `smb://<server>` a server's share list**, both real
  listings (`../pane/share-list-pane.svelte.ts`). The path is the only record of the server: every door opens one
  through the pane's arrival, ❌ never by holding a host. Mounted shares are their own volumes.

Architecture, the auth hand-offs, the reconnect flow, and decision rationale: `DETAILS.md`. Read it before any
non-trivial work here: editing, planning, reorganizing, or advising.
