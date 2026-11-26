/**
 * The words for a typed `ShareListError`: what the network pane says when a
 * server's share list didn't load, and the tooltip on that server's row.
 *
 * Classification is the backend's (`crates/cmdr-smb/src/types.rs`, filled by the
 * smb2 client and its `smbutil` / `smbclient` fallbacks); the words are all here,
 * pulled from the `errors.shareList.*` catalog so every locale gets its own. Same
 * split as `mount-error-messages.ts`; `docs/guides/error-handling.md` is the map.
 *
 * ❌ The variant's `message` is never the message: the backend documents it as
 * diagnostic detail for the log, and it's English (often a tool's own stderr).
 * A `missing_dependency`'s install command renders separately, in the pane's copy
 * box.
 */
import type { ShareListError } from '#lib/ipc/bindings.js'
import { getMessage, tString } from '#lib/intl/messages.svelte.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'
import { escapeMarkdown } from '#lib/error-messages/markdown-escape.js'
import type { FriendlyErrorMessage } from '#lib/error-messages/friendly-error-message.js'

/** Fills `{token}` placeholders in ONE pass, so a server literally named `{server}` stays a name. */
function raw(key: MessageKey, params: Record<string, string> = {}): string {
  return getMessage(key).replace(/\{([a-zA-Z]+)\}/g, (token, name: string) => params[name] ?? token)
}

/**
 * One renderer per `ShareListError` variant. A record rather than a `switch`:
 * the mapped type demands every variant, so a backend that grows one stops the
 * frontend compiling until it has words.
 */
const SHARE_LIST_MESSAGE: { [K in ShareListError['type']]: (server: string) => string } = {
  host_unreachable: (server) => raw('errors.shareList.hostUnreachable', { server }),
  timeout: (server) => raw('errors.shareList.timeout', { server }),
  auth_required: (server) => raw('errors.shareList.authRequired', { server }),
  signing_required: (server) => raw('errors.shareList.signingRequired', { server }),
  auth_failed: (server) => raw('errors.shareList.authFailed', { server }),
  signature_invalid: (server) => raw('errors.shareList.signatureInvalid', { server }),
  negotiation_invalid: (server) => raw('errors.shareList.negotiationInvalid', { server }),
  encryption_unavailable: (server) => raw('errors.shareList.encryptionUnavailable', { server }),
  encryption_required: (server) => raw('errors.shareList.encryptionRequired', { server }),
  protocol_error: (server) => raw('errors.shareList.protocolError', { server }),
  resolution_failed: (server) => raw('errors.shareList.resolutionFailed', { server }),
  missing_dependency: () => raw('errors.shareList.missingDependency'),
}

/**
 * The sentence a share listing that didn't load says. `serverName` is what the
 * pane and the servers list call the host (like `Testnas`).
 */
export function renderShareListError(error: ShareListError, serverName: string): string {
  return SHARE_LIST_MESSAGE[error.type](serverName)
}

/**
 * A share list that didn't list, as the pane's error screen words it (the listing
 * reasons `shareListSignIn` / `shareListUnavailable`): the share list's own title and
 * sentence, so the screen and the servers list's tooltip say one thing.
 *
 * ❗ The sentence lands in markdown, so the server's name goes in escaped; the title is
 * plain text. A server that wants an account reads as a sign-in, ❌ never "Couldn't
 * connect" (cmdr-reports#10).
 */
export function shareListListingMessage(
  reason: 'shareListSignIn' | 'shareListUnavailable',
  error: ShareListError,
  server: string,
): FriendlyErrorMessage {
  if (reason === 'shareListSignIn') {
    return {
      title: tString('fileExplorer.network.share.signInTitle', { hostName: server }),
      message:
        error.type === 'auth_required'
          ? tString('fileExplorer.network.share.signInMessage')
          : renderShareListError(error, escapeMarkdown(server)),
      suggestion: '',
    }
  }
  return {
    title: tString('fileExplorer.network.share.connectFailedTitle', { hostName: server }),
    message: renderShareListError(error, escapeMarkdown(server)),
    suggestion: '',
  }
}
