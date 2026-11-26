/**
 * A row's name as `cmdr://state` lists it: the name, then the facts the row shows as
 * locale-independent `key=value` tokens, so an agent reads what a person sees without the
 * translated words. A share row's description is `comment="…"` (the server's own text); a
 * saved share adds `status=` and `account=`; a Servers row (`ServerMeta`) says `protocol=`,
 * `status=`, `address=`, and `account=` (a username, or `guest`). Two spaces before the
 * tokens, like every other MCP row annotation.
 */

import type { ShareListAccount } from '#lib/ipc/bindings.js'
import type { FileEntry } from '../types'

export function mcpRowName(entry: FileEntry): string {
  const tokens: string[] = []
  const server = entry.serverMeta
  if (server) {
    tokens.push(`protocol=${server.protocol}`, `status=${server.status}`, `address=${server.address}`)
    if (server.account) tokens.push(accountToken(server.account))
  }
  const share = entry.shareMeta
  if (share?.description) tokens.push(`comment="${share.description}"`)
  if (share?.saved) {
    tokens.push(`status=${share.saved.status}`)
    if (share.saved.account) tokens.push(accountToken(share.saved.account))
  }
  return tokens.length === 0 ? entry.name : `${entry.name}  ${tokens.join(' ')}`
}

function accountToken(account: ShareListAccount): string {
  return `account=${account.kind === 'guest' ? 'guest' : account.username}`
}
