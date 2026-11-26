/**
 * Mounting a share from a server's share list: the dial, its wait and Cancel, the sign-in
 * a refused mount asks for, and the refusal the pane shows.
 *
 * A factory, ❌ not a component: the pane renders `view` through the ONE `RemoteConnectView`
 * (connecting, the calm `signed_out` sign-in state, a `refused` one with Try again), and the
 * share list's host is the caller's (`deps.getServer`). The share a door queued to mount
 * once the list lands is the pane's arrival (`share-list-arrival.svelte.ts`), ❌ not this
 * controller's: a pinned tab opens the list in a new pane component.
 *
 * ❗ **No credential lives here.** A mount with none signs in with the server's session
 * credential on the backend (`network/share_list_sign_in.rs::mount_with_session`); only an
 * answer the person types on the sheet crosses, once, for that one remount, and ❌ nothing
 * here keeps its password: Try again mounts with none.
 */

import type { MountError, NetworkHost, ShareInfo } from '../types'
import {
  cancelServerConnect,
  mountNetworkShare,
  newServerAttemptId,
  resolvePathVolume,
  saveSmbCredentials,
  setSmbAccountPreference,
} from '#lib/tauri-commands/index.js'
import type { MountErrorInfo } from '#lib/ipc/bindings.js'
import { getMountTimeoutMs } from '#lib/settings/network-settings.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { tString } from '#lib/intl/messages.svelte.js'
import { eventMatchesCommand } from '#lib/shortcuts/index.js'
import { claimKey } from '#lib/shortcuts/claim-key.js'
import { isMountSignInQuestion, openSmbSignInSheet, refusalForMountError } from '../network/smb-sign-in'
import { asMountError } from '../network/mount-error'
import { renderMountError } from '../network/mount-error-messages'
import type { SignInAttemptOutcome } from '#lib/servers/sign-in-contract.js'
import type { RemoteConnectState } from './remote-connect-state'
import type { VolumeChangePayload } from './types'

const log = getAppLogger('fileExplorer')

/**
 * How long a mount runs before the pane says it's still waiting. Matches the backend's
 * `netfs_wedge::SUSPECT_AFTER`: a healthy mount takes a second or two.
 */
export const MOUNT_SLOW_AFTER_MS = 10_000

/** The server the share list is open on, and what the person calls it. */
export interface ShareMountServer {
  host: NetworkHost
  /** The hub's label for it, when it has one; `host.name` otherwise. */
  label: string | undefined
}

/** An account to mount with (an empty password is valid SMB). */
export interface ShareMountCredentials {
  username: string
  password: string
}

export interface ShareMountDeps {
  /** The share list's server, or `null` when the pane shows none. */
  getServer: () => ShareMountServer | null
  /** Takes the pane to the mounted share's volume. */
  onVolumeChange: (change: VolumeChangePayload) => void
}

/** What `RemoteConnectView` renders while a mount runs or after it stopped short. */
export interface ShareMountView {
  /** The share. */
  name: string
  /** The server's friendly name, or `null` when that's only its address (`address` shows it). */
  server: string | null
  /** `host:port`, an IPv6 host bracketed. */
  address: string
  state: RemoteConnectState
}

export interface ShareMount {
  /** The wait, the sign-in state, or the refusal; `null` while the share list is on screen. */
  readonly view: ShareMountView | null
  /** True while the sign-in sheet is up over a refused mount. */
  readonly signingIn: boolean
  /**
   * A mount that didn't go through, for `cmdr://state`'s `mountError` (the refusal and the
   * sign-in state a credential question routes to alike), or `null`. Without it a failed
   * mount is invisible there: the pane's `files` still describe the share list it replaced.
   */
  readonly mountErrorInfo: MountErrorInfo | null
  /** Mounts `share`, and asks for a credential if that's what's missing (Enter on a share). */
  mount: (share: ShareInfo, credentials: ShareMountCredentials | null) => Promise<void>
  /** Back from a refused mount to the share list (the host stays). */
  back: () => void
  /** A different host, or none: drops the refusal and the attempt it would retry. */
  reset: () => void
  /**
   * The keyboard while the controller's state is up: `share.back` cancels a running mount
   * or steps back from a refusal, and every other key is dead. Answers whether the
   * controller owns the keyboard right now (the caller then delegates nothing).
   */
  handleKeyDown: (e: KeyboardEvent) => boolean
}

/** Returns the hostname or IP to connect to (port goes separately), falling back to the hostname for loopback. */
function resolveServerAddress(host: NetworkHost): string {
  const ip = host.ipAddress
  const isLoopback = ip === '127.0.0.1' || ip === '::1'
  return (isLoopback ? host.hostname : ip) ?? host.hostname ?? host.name
}

/** `host:port` the way the other connecting views show it (`serverAddressOf`), an IPv6 host bracketed. */
function dialedAddress(server: string, port: number): string {
  return `${server.includes(':') ? `[${server}]` : server}:${String(port)}`
}

/**
 * What the person calls the host (the hub's label, else its name), or `null` when that's
 * only its address: the dialed `host:port` shows under it, and one address twice says nothing.
 */
function friendlyServerName(host: NetworkHost, label: string | undefined, dialed: string): string | null {
  const friendly = label ?? host.name
  const isAddress = [dialed, host.ipAddress].some((a) => a?.toLowerCase() === friendly.toLowerCase())
  return isAddress ? null : friendly
}

export function createShareMount(deps: ShareMountDeps): ShareMount {
  /** The share, its server's friendly name, and the address it dials: what the views name. */
  interface MountTarget {
    share: string
    server: string | null
    address: string
  }

  /**
   * The running mount. Its own copy, ❌ not derived from the server: a mount that went
   * through leaves the host before it lands on the share, and the wait is still on screen then.
   */
  let mounting = $state<MountTarget | null>(null)
  let mountError = $state<MountError | null>(null)
  /**
   * The share a retry mounts again, the account last offered (the sheet's prefill), and what
   * the refusal views name. ❗ No password: a typed answer the server never judged is the
   * backend's session credential (`share_list_sign_in.rs::mount_with_sheet_answer`), so Try
   * again mounts with none.
   */
  let lastMountAttempt = $state<{
    share: ShareInfo
    username: string | null
    target: MountTarget
  } | null>(null)
  let signingIn = $state(false)
  /** Set once the running mount outlasted `MOUNT_SLOW_AFTER_MS`. */
  let mountingSlow = $state(false)
  /** The running mount's id, which Cancel calls it off by. `null` when none runs. */
  let mountAttemptId: string | null = null
  /** Whether the person pressed Cancel on the running mount, so its `cancelled` answer goes back quietly. */
  let mountCancelRequested = false

  const mountErrorInfo = $derived.by((): MountErrorInfo | null => {
    const share = lastMountAttempt?.share.name
    if (!mountError || share === undefined) return null
    return { share, reason: mountError.type, message: renderMountError(mountError, deps.getServer()?.host.name) }
  })

  /**
   * Files a new mount as the running one: its id for Cancel, and the timer that says it's
   * still waiting. `finish` undoes both once the mount answers.
   */
  function startMountAttempt(): { attemptId: string; finish: () => void } {
    const attemptId = newServerAttemptId()
    mountAttemptId = attemptId
    mountCancelRequested = false
    mountingSlow = false
    const slowTimer = setTimeout(() => {
      if (mountAttemptId === attemptId) mountingSlow = true
    }, MOUNT_SLOW_AFTER_MS)
    const finish = () => {
      clearTimeout(slowTimer)
      if (mountAttemptId === attemptId) mountAttemptId = null
      mountingSlow = false
    }
    return { attemptId, finish }
  }

  /**
   * Calls off the running mount. The backend stops WAITING and answers; the NetFS call
   * itself can't be taken back (`src-tauri/src/network/netfs_wedge.rs`).
   */
  function cancelMount() {
    if (!mountAttemptId) return
    mountCancelRequested = true
    void cancelServerConnect(mountAttemptId).catch((e: unknown) => {
      log.warn('Calling off the mount did not go through: {error}', { error: e })
    })
  }

  /** Takes the pane to the share mounted at `mountPath`. */
  async function landOnMount(mountPath: string) {
    // The real volume, for the breadcrumb and the volume context. The mount's own
    // volumes-changed broadcast refreshes the list; no need to ask for it here.
    const { volume } = await resolvePathVolume(mountPath)
    if (volume) {
      deps.onVolumeChange({ volumeId: volume.id, volumePath: volume.path, targetPath: mountPath })
    } else {
      // The volume list hasn't caught up yet: the mount path is both.
      deps.onVolumeChange({ volumeId: mountPath, volumePath: mountPath, targetPath: mountPath })
    }
  }

  /**
   * Mounts `share` and takes the pane to it. Answers the failure rather than throwing, so a
   * caller inside the sign-in sheet can offer a retry and one outside it can show the refusal.
   */
  async function mountShare(share: ShareInfo, credentials: ShareMountCredentials | null): Promise<MountError | null> {
    const current = deps.getServer()
    if (!current) return null
    const { host, label } = current

    const server = resolveServerAddress(host)
    const target: MountTarget = {
      share: share.name,
      server: friendlyServerName(host, label, server),
      address: dialedAddress(server, host.port),
    }
    lastMountAttempt = { share, username: credentials?.username ?? null, target }
    mountError = null
    mounting = target
    const { attemptId, finish } = startMountAttempt()

    try {
      // The host's own name rides along: a mount that goes through is saved as a share
      // place under the host the person knows (the Servers list).
      const result = await mountNetworkShare(
        server,
        share.name,
        credentials?.username ?? null,
        credentials?.password ?? null,
        host.port,
        getMountTimeoutMs(),
        host.name,
        attemptId,
      )
      lastMountAttempt = null
      await landOnMount(result.mountPath)
      return null
    } catch (e) {
      const refusal = asMountError(e)
      // The person called it off: back to the share list, ❌ no refusal. A cancel after a
      // long wait on a server that answers comes back `sign_in_helper_stuck` instead, and
      // that one the pane explains.
      if (mountCancelRequested && refusal?.type === 'cancelled') return null
      // A value that isn't a typed refusal means the IPC call itself broke down, which the
      // catch-all words honestly; its text goes to the log below.
      mountError = refusal ?? { type: 'unexpected', server, share: share.name, detail: String(e) }
      // WARN, not ERROR: the pane renders this with a retry, so it's an outcome the person
      // is looking at, not a defect to report (an error-level line captured a backtrace and
      // a state snapshot into the error-report bundle for every unreachable NAS).
      log.warn('Mount of {share} on {host} did not go through: {error}', {
        share: share.name,
        host: deps.getServer()?.host.name ?? 'unknown host',
        // Pre-stringified: the log bridge renders a property with `String()`.
        error: JSON.stringify(mountError),
      })
      return mountError
    } finally {
      finish()
      mounting = null
    }
  }

  async function mount(share: ShareInfo, credentials: ShareMountCredentials | null) {
    const error = await mountShare(share, credentials)
    if (error && isMountSignInQuestion(error)) await askForMountCredentials(share, error)
  }

  /**
   * Hands a mount's credential question to the one sign-in sheet, retrying the mount inside
   * it for as long as the person keeps answering.
   *
   * ❗ Cancelling goes back to the SHARE LIST (`back`), which is where the person was: they
   * picked a share, and the one they wanted is still on screen behind the sheet.
   */
  async function askForMountCredentials(share: ShareInfo, firstError: MountError) {
    const current = deps.getServer()
    // ❗ One ask at a time. A second `openSignInSheet` closes the first as `cancelled`, and
    // this caller reads that as "the person said no" and clears the failure behind a sheet
    // that's still up.
    if (!current || signingIn) return
    const { host } = current
    signingIn = true
    try {
      const result = await openSmbSignInSheet({
        host,
        label: current.label,
        shareName: share.name,
        // ❗ No guest option: an unauthenticated mount is exactly what just came back
        // refused, so offering it again would be inert.
        guestAllowed: false,
        initialUsername: lastMountAttempt?.username ?? undefined,
        refusal: refusalForMountError(firstError),
        attempt: (answer) => remountWith(share, host.name, answer),
      })
      if (result.kind === 'cancelled') back()
    } finally {
      signingIn = false
    }
  }

  /** One mount round-trip with what the person offered, remembering the credential ❗ only once it went through. */
  async function remountWith(
    share: ShareInfo,
    hostName: string,
    answer: { username: string | null; password: string | null; remember: boolean },
  ): Promise<SignInAttemptOutcome> {
    const credentials = answer.username === null ? null : { username: answer.username, password: answer.password ?? '' }
    const error = await mountShare(share, credentials)
    if (!error) {
      // ❗ The account that got in becomes the host's account, as "Sign in as…" does, so the
      // next visit lists and mounts as it. Without this, a share guests can list but not
      // open asked for the password on every mount (cmdr-reports#27).
      if (credentials) void setSmbAccountPreference(hostName, credentials.username).catch(() => {})
      if (credentials && answer.remember && answer.password !== null) {
        try {
          await saveSmbCredentials(hostName, null, credentials.username, answer.password)
        } catch (e) {
          log.warn('Mount succeeded but saving credentials failed: {error}', { error: e })
        }
      }
      return { kind: 'handed_off' }
    }
    // ❗ Only a refusal another credential can answer keeps the sheet open. Anything else is
    // about the SHARE or the server, and the pane's refusal is what has the words and the retry.
    if (!isMountSignInQuestion(error)) return { kind: 'handed_off' }
    return { kind: 'refused', refusal: refusalForMountError(error) }
  }

  function back() {
    mountError = null
  }

  /** The calm sign-in state's button: asks again, for the share that wanted an account. */
  function signInAgain() {
    if (lastMountAttempt && mountError) void askForMountCredentials(lastMountAttempt.share, mountError)
  }

  function retry() {
    if (lastMountAttempt) void mount(lastMountAttempt.share, null)
  }

  /** The running mount as the ONE remote-wait state every pane renders. */
  const view = $derived.by((): ShareMountView | null => {
    if (mounting) {
      return {
        name: mounting.share,
        server: mounting.server,
        address: mounting.address,
        state: {
          kind: 'connecting',
          cancel: cancelMount,
          hint: mountingSlow ? tString('fileExplorer.networkMount.mountingSlow') : undefined,
        },
      }
    }
    if (!mountError || !lastMountAttempt) return null
    const { target } = lastMountAttempt
    const current = deps.getServer()
    // ❗ A share that wants an account is a normal step, not a failure: the calm state, no
    // Try again, which without an account gets the same answer (cmdr-reports#10). The sheet
    // sits on top of it.
    const state: RemoteConnectState =
      mountError.type === 'auth_required'
        ? {
            kind: 'signed_out',
            title: tString('fileExplorer.networkMount.signInTitle', { share: mountError.share }),
            hint: tString('fileExplorer.networkMount.signInMessage', {
              server: current?.label ?? current?.host.name ?? mountError.server,
            }),
            signIn: signInAgain,
            back,
          }
        : { kind: 'refused', refusal: renderMountError(mountError, current?.host.name), retry, back }
    return { name: target.share, server: target.server, address: target.address, state }
  })

  return {
    get view() {
      return view
    },
    get signingIn() {
      return signingIn
    },
    get mountErrorInfo() {
      return mountErrorInfo
    },
    mount,
    back,
    reset: () => {
      mountError = null
      lastMountAttempt = null
    },
    handleKeyDown: (e) => {
      // The sign-in sheet is modal and owns the keyboard while it's up.
      if (signingIn) return true
      if (mounting) {
        // The share list isn't on screen while a mount runs; `share.back` is Cancel.
        if (eventMatchesCommand(e, 'share.back')) {
          claimKey(e)
          cancelMount()
        }
        return true
      }
      if (mountError) {
        // The refusal replaced the share list, so nothing else takes keys: without this
        // branch the whole keyboard went dead and the buttons were the only way out.
        // `claimKey` keeps it to ONE step: `⌘↑` is also `nav.parent` in the document
        // dispatch map, which would step out of the host as well and skip the share list
        // (the hazard `pane-key-router.ts`'s `handleOpenOrParentKey` stops for).
        if (eventMatchesCommand(e, 'share.back')) {
          claimKey(e)
          back()
        }
        return true
      }
      return false
    },
  }
}
