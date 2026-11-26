/**
 * Writing and reading the path of a server's share list: `smb://<key>[:<port>]` on the
 * `network` volume, where `smb://` alone is the Servers hub.
 *
 * ❗ **The key never changes for one server.** A discovered server's is its Bonjour
 * name, a manual one's the address the person typed, both lowercased. ❌ Not the IP and
 * ❌ not "the resolved hostname, else the name": an IP drifts with DHCP and resolution
 * is async, so one server would get two paths depending on when it was clicked, which
 * splits its history, tabs, and landings. The path bar shows the friendly label; this
 * key is identity only.
 *
 * ❗ **The Rust reader is `src-tauri/src/network/share_list_path.rs`**, and the two agree
 * through one case table (`share-list-path-cases.json`). The port rides only off 445.
 *
 * Escaping keeps the authority ONE token in both path grammars (`#lib/path/grammar.ts`,
 * `cmdr_fs::app_path`): a name's `%`, `/`, `\`, `:`, `[`, and `]` go as `%XX`, and an
 * IPv6 address goes in brackets with its colons kept. So a bare `:` is always the port.
 *
 * ❌ No second parse at a call site: read the key back through {@link shareListHostKey}.
 */

import type { NetworkHost } from '#lib/file-explorer/types.js'

/** SMB's own port, which a share-list path leaves unsaid. */
const SMB_PORT = 445

const SCHEME = 'smb://'

/** The characters a key escapes: the grammar's separators, the port's colon, the brackets, and `%` itself. */
const ESCAPED = /[%/\\:[\]]/g

/** An IPv6 literal, as loosely as a bracket needs: hex digits, dots, and at least two colons. */
const IPV6 = /^(?=.*:.*:)[0-9a-f:.]+$/

/** What a share-list path names: the server's key and its port. */
export interface ShareListAddress {
  /** The server's key, lowercased and unescaped. */
  key: string
  port: number
}

/** The key a server's share list goes by: see the module comment. */
function keyOf(host: NetworkHost): string {
  const name = host.source === 'manual' ? (host.hostname ?? host.name) : host.name
  return name.toLowerCase()
}

/** The share-list path of `host`. */
export function shareListPath(host: NetworkHost): string {
  const key = keyOf(host)
  const authority = IPV6.test(key)
    ? `[${key}]`
    : key.replace(ESCAPED, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, '0')}`)
  return host.port === SMB_PORT ? `${SCHEME}${authority}` : `${SCHEME}${authority}:${String(host.port)}`
}

/**
 * The server a share-list path names, or `null` for any other path: the hub (`smb://`),
 * a share (`smb://<key>/<share>`, which is mounted, never listed here), another scheme,
 * or a spelling that doesn't read. Folds case, an explicit `:445`, and the trailing `/`
 * Rust's grammar spells a scheme root with.
 */
export function shareListHostKey(path: string): ShareListAddress | null {
  if (!path.startsWith(SCHEME)) return null
  // Rust spells a scheme root with a trailing `/` (`smb://testnas/`): the same server.
  const rest = path.slice(SCHEME.length)
  const authority = rest.endsWith('/') ? rest.slice(0, -1) : rest
  if (authority === '' || authority.includes('/')) return null

  const split = splitAuthority(authority)
  if (split === null) return null
  const { key, portText } = split
  const port = portText === null ? SMB_PORT : readPort(portText)
  if (port === null || key === null || key === '') return null
  return { key: key.toLowerCase(), port }
}

/**
 * Whether a pane on `volumeId` at `path` shows a server's share list: the `network`
 * volume at `smb://<server>`. ❗ The ONE routing rule; `capabilitiesForPane` answers its
 * `share-list` kind from it. Ask `capabilitiesForPane` instead, except from a leaf that
 * mustn't pull in the volume store (the restore in `app-status-store.ts`).
 */
export function isShareListLocation(volumeId: string, path: string): boolean {
  return volumeId === 'network' && shareListHostKey(path) !== null
}

/** Whether two paths name one server's share list (case, an explicit `:445`, and a trailing `/` folded). */
export function sameShareList(a: string, b: string): boolean {
  const left = shareListHostKey(a)
  const right = shareListHostKey(b)
  return left !== null && right !== null && left.key === right.key && left.port === right.port
}

/**
 * An authority's key (unescaped, or the bracketed IPv6 literal) and its port text, or
 * `null` when its shape doesn't read. A key that doesn't unescape comes back `null`.
 */
function splitAuthority(authority: string): { key: string | null; portText: string | null } | null {
  if (authority.startsWith('[')) {
    const close = authority.indexOf(']')
    if (close < 0) return null
    const literal = authority.slice(1, close).toLowerCase()
    const key = IPV6.test(literal) ? literal : null
    const rest = authority.slice(close + 1)
    if (rest === '') return { key, portText: null }
    return rest.startsWith(':') ? { key, portText: rest.slice(1) } : null
  }
  const colons = authority.split(':')
  if (colons.length > 2) return null
  return { key: unescapeKey(colons[0]), portText: colons.length === 2 ? colons[1] : null }
}

function readPort(text: string): number | null {
  if (!/^[0-9]{1,5}$/.test(text)) return null
  const port = Number(text)
  return port >= 1 && port <= 65535 ? port : null
}

/**
 * `text` with every `%XX` decoded, or `null` when it isn't one the writer could have
 * written: an escape that doesn't read or names no ASCII character, or a character the
 * writer always escapes standing bare.
 */
function unescapeKey(text: string): string | null {
  let out = ''
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (c !== '%') {
      if (c === '\\' || c === '[' || c === ']') return null
      out += c
      continue
    }
    const hex = text.slice(i + 1, i + 3)
    if (!/^[0-9a-fA-F]{2}$/.test(hex)) return null
    const code = parseInt(hex, 16)
    if (code > 0x7f) return null
    out += String.fromCharCode(code)
    i += 2
  }
  return out
}
