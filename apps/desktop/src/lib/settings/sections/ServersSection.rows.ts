/**
 * The searchable non-setting rows of `ServersSection.svelte`: the trusted host keys,
 * certificates, and link sign-ins, one Forget per row. The page's one setting
 * (`network.showServerParentRow`) is what puts it in the sidebar.
 */

import type { SearchableRow } from '../types'

export const serversRows: SearchableRow[] = [
  {
    // The trusted host keys card: host and port, algorithm, fingerprint, when it
    // was trusted, and the Forget that makes the next connection first contact.
    id: 'row:network.trustedHostKeys',
    section: ['File systems', 'Servers (SFTP, WebDAV)'],
    labelKey: 'settings.servers.card.trustedHostKeys',
    cardKey: 'settings.servers.card.trustedHostKeys',
    keywordsKey: 'settings.network.trustedHostKeys.keywords',
    terms: ['sftp', 'webdav', 'ssh'],
  },
  {
    // The trusted certificates card: the server certificates (a NAS's self-signed
    // one) a person told Cmdr to trust, each with the Forget that asks again.
    id: 'row:network.trustedCertificates',
    section: ['File systems', 'Servers (SFTP, WebDAV)'],
    labelKey: 'settings.servers.card.trustedCertificates',
    cardKey: 'settings.servers.card.trustedCertificates',
    keywordsKey: 'settings.network.trustedCertificates.keywords',
    terms: ['webdav', 'https', 'tls', 'ssl'],
  },
  {
    // The SMB link sign-ins card: each (share's server, other server) pair a person
    // allowed, with the Forget that makes those links ask again.
    id: 'row:network.smbLinkConsents',
    section: ['File systems', 'Servers (SFTP, WebDAV)'],
    labelKey: 'settings.servers.card.smbLinkConsents',
    cardKey: 'settings.servers.card.smbLinkConsents',
    keywordsKey: 'settings.network.smbLinkConsents.keywords',
    terms: ['smb', 'dfs'],
  },
]
