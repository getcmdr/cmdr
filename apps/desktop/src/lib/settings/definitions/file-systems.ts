/**
 * File systems section settings (data only). Logic lives in `../settings-registry.ts`,
 * which concatenates this array into the full registry in section order.
 */

import type { SettingDefinitionSource } from '../types'

export const fileSystemsSettings: SettingDefinitionSource[] = [
  // ========================================================================
  // File systems › SMB/Network shares
  // ========================================================================
  {
    id: 'network.enabled',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.enabled.label',
    descriptionKey: 'settings.network.enabled.description',
    descriptionInTip: { summaryKey: 'settings.network.enabled.summary' },
    cardKey: 'settings.network.card.connection',
    keywordsKey: 'settings.network.enabled.keywords',
    terms: ['smb', 'mdns', 'bonjour'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },
  {
    id: 'network.firstTriggerDone',
    kind: 'state',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.firstTriggerDone.label',
    descriptionKey: 'settings.network.firstTriggerDone.description',
    type: 'boolean',
    default: false,
    component: 'switch',
    hidden: true,
  },
  {
    // Internal: hidden from the Settings UI. Remembers whether the person last
    // opened or collapsed the servers hub's "found nearby" group; `auto` until
    // they toggle it, which is when the hub picks by whether they saved a server.
    id: 'network.nearbyServersGroup',
    kind: 'state',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.nearbyServersGroup.label',
    // A `string`, not an `enum`: an enum's options each need a rendered label, and
    // nothing ever renders these. `NearbyServersGroupChoice` keeps the three
    // tokens type-safe at every call site.
    type: 'string',
    default: 'auto',
    component: 'text-input',
    hidden: true,
  },
  {
    id: 'network.directSmbConnection',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.directSmbConnection.label',
    descriptionKey: 'settings.network.directSmbConnection.description',
    descriptionInTip: true,
    cardKey: 'settings.network.card.connection',
    keywordsKey: 'settings.network.directSmbConnection.keywords',
    terms: ['smb', 'smb2'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },
  {
    // The backend ORs it with the managed policy's `RequireSMBEncryption`, which also locks the row on
    // (`managed_policy/locked.rs`). What it reaches: `crates/cmdr-smb/DETAILS.md` § "Requiring encryption".
    id: 'network.requireSmbEncryption',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.requireSmbEncryption.label',
    descriptionKey: 'settings.network.requireSmbEncryption.description',
    descriptionInTip: { summaryKey: 'settings.network.requireSmbEncryption.summary' },
    cardKey: 'settings.network.card.connection',
    keywordsKey: 'settings.network.requireSmbEncryption.keywords',
    terms: ['smb', 'smb3', 'aes'],
    type: 'boolean',
    default: false,
    component: 'switch',
    // ❌ Not even on: an MCP client must not be able to weaken how Cmdr talks to a server.
    mcpSettable: false,
  },
  {
    id: 'network.shareCacheDuration',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.shareCacheDuration.label',
    descriptionKey: 'settings.network.shareCacheDuration.description',
    descriptionInTip: true,
    cardKey: 'settings.network.card.performanceAndTimeouts',
    keywordsKey: 'settings.network.shareCacheDuration.keywords',
    terms: ['smb', 'ttl'],
    type: 'duration',
    default: 30000, // 30 seconds in ms
    component: 'select',
    constraints: {
      unit: 's',
      options: [
        { value: 30000, labelKey: 'settings.network.shareCacheDuration.opt.s30' },
        { value: 300000, labelKey: 'settings.network.shareCacheDuration.opt.min5' },
        { value: 3600000, labelKey: 'settings.network.shareCacheDuration.opt.hour1' },
        { value: 86400000, labelKey: 'settings.network.shareCacheDuration.opt.day1' },
        { value: 2592000000, labelKey: 'settings.network.shareCacheDuration.opt.days30' },
      ],
      allowCustom: true,
      customMin: 1000,
      customMax: 2592000000,
    },
  },
  {
    // One stepper, in seconds. It replaced a Normal (15 s) / Slow network (45 s) / Custom radio
    // list plus a custom field; `settings-migrations.ts` migration 10 carries old choices across.
    id: 'network.timeout',
    section: ['File systems', 'SMB/Network shares'],
    labelKey: 'settings.network.timeout.label',
    descriptionKey: 'settings.network.timeout.description',
    descriptionInTip: true,
    cardKey: 'settings.network.card.performanceAndTimeouts',
    keywordsKey: 'settings.network.timeout.keywords',
    terms: ['vpn'],
    type: 'duration',
    default: 15000,
    component: 'duration',
    constraints: {
      unit: 's',
      minMs: 5000,
      maxMs: 120000,
    },
  },

  // ❗ `File systems › Servers (SFTP, WebDAV)` has NO entry here, on purpose. Its
  // whole content is the trusted-host-key list, one Forget per row, and the saved
  // servers live in the hub rather than here
  // (`src/lib/file-explorer/network/DETAILS.md` § `ServersHub.svelte` says why a
  // pane state and not a Settings list). Its sidebar row and its search hits come
  // from `sections/ServersSection.rows.ts`.

  // ========================================================================
  // File systems › MTP (Android/Kindle/cameras)
  // ========================================================================
  {
    id: 'fileOperations.mtpEnabled',
    section: ['File systems', 'MTP (Android/Kindle/cameras)'],
    labelKey: 'settings.fileOperations.mtpEnabled.label',
    descriptionKey: 'settings.fileOperations.mtpEnabled.description',
    descriptionInTip: { summaryKey: 'settings.fileOperations.mtpEnabled.summary' },
    keywordsKey: 'settings.fileOperations.mtpEnabled.keywords',
    terms: ['mtp', 'android', 'usb', 'ptpcamerad'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },
  {
    id: 'fileOperations.mtpConnectionWarning',
    section: ['File systems', 'MTP (Android/Kindle/cameras)'],
    labelKey: 'settings.fileOperations.mtpConnectionWarning.label',
    descriptionKey: 'settings.fileOperations.mtpConnectionWarning.description',
    descriptionInTip: true,
    keywordsKey: 'settings.fileOperations.mtpConnectionWarning.keywords',
    terms: ['mtp', 'android'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },

  // ========================================================================
  // File systems › Android (ADB)
  //
  // Both are live-applied together (`adb-settings.ts` re-reads and pushes both
  // through `set_adb_settings`, wired in `settings-applier.ts`), because the
  // backend restarts the device tracker under whichever binary the path names.
  // `AdbSection.svelte` renders them, plus the status, Re-check, and Browse rows,
  // which are not settings.
  // ========================================================================
  {
    id: 'fileOperations.adbEnabled',
    section: ['File systems', 'Android (ADB)'],
    labelKey: 'settings.fileOperations.adbEnabled.label',
    descriptionKey: 'settings.fileOperations.adbEnabled.description',
    descriptionInTip: { summaryKey: 'settings.fileOperations.adbEnabled.summary' },
    keywordsKey: 'settings.fileOperations.adbEnabled.keywords',
    terms: ['adb', 'android', 'platform-tools'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },
  {
    id: 'fileOperations.adbBinaryPath',
    section: ['File systems', 'Android (ADB)'],
    labelKey: 'settings.fileOperations.adbBinaryPath.label',
    descriptionKey: 'settings.fileOperations.adbBinaryPath.description',
    descriptionInTip: { summaryKey: 'settings.fileOperations.adbBinaryPath.summary' },
    keywordsKey: 'settings.fileOperations.adbBinaryPath.keywords',
    terms: ['adb', 'android', 'platform-tools', 'sdk'],
    type: 'string',
    default: '',
    component: 'text-input',
  },

  // ========================================================================
  // File systems › Git
  // ========================================================================
  {
    id: 'fileExplorer.git.showRepoChip',
    section: ['File systems', 'Git'],
    labelKey: 'settings.fileExplorer.git.showRepoChip.label',
    descriptionKey: 'settings.fileExplorer.git.showRepoChip.description',
    descriptionInTip: true,
    keywordsKey: 'settings.fileExplorer.git.showRepoChip.keywords',
    terms: ['git'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },
  {
    id: 'fileExplorer.git.showStatusColumn',
    section: ['File systems', 'Git'],
    labelKey: 'settings.fileExplorer.git.showStatusColumn.label',
    descriptionKey: 'settings.fileExplorer.git.showStatusColumn.description',
    descriptionInTip: true,
    keywordsKey: 'settings.fileExplorer.git.showStatusColumn.keywords',
    terms: ['git'],
    type: 'boolean',
    default: false,
    component: 'switch',
  },
  {
    id: 'fileExplorer.git.showVirtualGitPortal',
    section: ['File systems', 'Git'],
    labelKey: 'settings.fileExplorer.git.showVirtualGitPortal.label',
    descriptionKey: 'settings.fileExplorer.git.showVirtualGitPortal.description',
    descriptionInTip: true,
    keywordsKey: 'settings.fileExplorer.git.showVirtualGitPortal.keywords',
    terms: ['git'],
    type: 'boolean',
    default: true,
    component: 'switch',
  },
]
