/**
 * Every command the app can call (`bindings.ts`), accounted for: answered by a TS table, an engine,
 * or a page host, or left out on purpose with the reason a browser can't. A new command that's in
 * none of those fails `svelte-check` here (`notInDemo` must name every unanswered one) and
 * `coverage.test.ts`, on the change that adds it: an unanswered pane-boot command once made every
 * pane say "didn't respond in time", and only the nightly demo suite would have caught it. See
 * `DETAILS.md` § Answering a command.
 */
import type { CommandName } from './answer-types.js'
import { appAnswers, rawAnswers, silentCommands } from './answers.js'
import { SET_APP_THEME } from './color-scheme.js'
import { DRAG_COMMANDS } from './drag-session.js'
import { LISTING_ENGINE_COMMANDS, MENU_ENGINE_COMMANDS } from './engine-command-names.js'
import { pluginHandler } from './plugins.js'
import { RELAYED_COMMANDS } from './relayed.js'
import { volumeAnswers } from './volumes.js'
import { WEBVIEW_COMMANDS } from './own-window.js'
import { WINDOW_HOST_COMMANDS, WINDOW_PLUGIN_COMMANDS } from './windows.js'

/** A wire name as the bindings spell it: `live_volume_path` → `liveVolumePath`. */
type CamelCase<S extends string> = S extends `${infer Head}_${infer Tail}` ? `${Head}${Capitalize<CamelCase<Tail>>}` : S

/** The wire (snake_case) names something other than a TS table answers. */
const WIRE_ANSWERED = [...LISTING_ENGINE_COMMANDS, ...MENU_ENGINE_COMMANDS, ...WINDOW_HOST_COMMANDS, ...DRAG_COMMANDS]

type Answered =
  | keyof typeof appAnswers
  | keyof typeof volumeAnswers
  | (typeof silentCommands)[number]
  | (typeof RELAYED_COMMANDS)[number]
  | CamelCase<(typeof WIRE_ANSWERED)[number]>

/** The bindings commands nothing in the demo answers. */
type Unanswered = Exclude<CommandName, Answered>

const camelCase = (snake: string) => snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase())

/** Every bindings command the demo answers, by its bindings (camelCase) name. */
export const answeredCommands: ReadonlySet<string> = new Set<string>([
  ...Object.keys(appAnswers),
  ...Object.keys(volumeAnswers),
  ...silentCommands,
  ...RELAYED_COMMANDS,
  ...WIRE_ANSWERED.map(camelCase),
])

// Why a command stays out. One line each, so a reader sees what the demo lacks at a glance.
const OS_SHELL =
  'asks the OS shell (Finder, Explorer, Quick Look, the Dock, System Settings), which a page has no door to'
const OS_CLIPBOARD = "the OS file clipboard: a page can't put files on it or read them back"
const CLIPBOARD_READ =
  "a page reads the clipboard only behind the browser's permission prompt, which opening Go to path mustn't raise, so it opens without the prefill"
const NETWORK = 'the demo has no network hosts, servers, or saved places: its one volume is in memory'
const DEVICES = 'the demo has no USB phones or cameras'
const ARCHIVES = "the demo doesn't browse or pack archives (`DETAILS.md` § Archives)"
const GIT = 'no repo on the demo volume: `getGitRepoInfo` answers none'
const INDEX = "no drive or media index: folder sizes come from the engine's fold (`get_dir_stats*`)"
const AI = 'no AI in a page: no local model, no provider key, so Ask Cmdr and the AI modes stay off'
const SERVICES = 'no license server, updater, report upload, or newsletter from a page'
const LIFECYCLE = "a page can't quit, relaunch, or wipe its tab"
const DEV = 'developer and diagnostics tooling (MCP, logs, debug fixtures) the demo has no backend for'
const SUBSYSTEM_SETTING = "a setting pushed to a backend subsystem the demo doesn't run"
const IN_PAGE_MENUS =
  "the demo draws its bar and menus itself (`menu-bar.svelte.ts`, `context-menu.ts`); answer these from `DemoMenuBar` once the app's own in-page bar asks them (task 63 steps 6 and 7)"

/**
 * The commands left out of the demo on purpose, by bindings name, each with its reason. An
 * unanswered value command rejects with Tauri's "no such command" string, a void one resolves:
 * the app's own path for a backend that can't. `satisfies` names every unanswered command, and
 * rejects one that something answers.
 */
export const notInDemo = {
  // The OS shell and its integrations
  toggleTags: OS_SHELL,
  revealInFileManager: OS_SHELL,
  quickLookOpen: OS_SHELL,
  quickLookSetPath: OS_SHELL,
  quickLookClose: OS_SHELL,
  getInfo: OS_SHELL,
  openInEditor: OS_SHELL,
  googleDriveLinks: OS_SHELL,
  copyShareLink: OS_SHELL,
  cloudMakeAvailableOffline: OS_SHELL,
  cloudRemoveDownload: OS_SHELL,
  refreshDirectoryIcons: OS_SHELL,
  clearExtensionIconCache: OS_SHELL,
  clearDirectoryIconCache: OS_SHELL,
  getTrashDir: OS_SHELL,
  openTrashView: OS_SHELL,
  releasePromiseDrop: OS_SHELL,
  listTerminalApps: OS_SHELL,
  openTerminalHere: OS_SHELL,
  terminalAppDisplayName: OS_SHELL,
  listTextEditors: OS_SHELL,
  addCmdrToDock: OS_SHELL,
  openPrivacySettings: OS_SHELL,
  openAppearanceSettings: OS_SHELL,
  openSystemSettingsUrl: OS_SHELL,
  setRevealHandlerEnabled: OS_SHELL,
  setFinderExtensionEnabled: OS_SHELL,
  installCliCommand: OS_SHELL,
  uninstallCliCommand: OS_SHELL,
  getNotificationPermission: OS_SHELL,
  showNotification: OS_SHELL,
  goToLatestDownload: OS_SHELL,
  downloadsWatcherStatus: OS_SHELL,
  recheckDownloadsWatcherGate: OS_SHELL,
  setGlobalGoToLatestShortcut: OS_SHELL,
  viewerForwardEditActionToPage: OS_SHELL,
  // The file clipboard
  copyPathsToClipboard: OS_CLIPBOARD,
  cutPathsToClipboard: OS_CLIPBOARD,
  readClipboardFiles: OS_CLIPBOARD,
  readClipboardText: CLIPBOARD_READ,
  pasteClipboardAsFile: OS_CLIPBOARD,
  clearClipboardCutState: OS_CLIPBOARD,
  // Network volumes, servers, and saved places
  listSmbVolumes: NETWORK,
  getSmbDiagnostics: NETWORK,
  refreshVolumes: NETWORK,
  setDirectSmbConnection: NETWORK,
  setSmbConcurrencyCmd: NETWORK,
  setRequireSmbEncryption: NETWORK,
  resolveHost: NETWORK,
  connectToServer: NETWORK,
  listSharesOnHost: NETWORK,
  getKnownShareByName: NETWORK,
  updateKnownShare: NETWORK,
  getUsernameHint: NETWORK,
  allowSmbLinkTarget: NETWORK,
  forgetSmbLinkTarget: NETWORK,
  listSmbLinkConsents: NETWORK,
  saveSmbCredentials: NETWORK,
  hasCachedSmbCredentials: NETWORK,
  deleteSmbCredentials: NETWORK,
  isUsingCredentialFileFallback: NETWORK,
  listSharesWithCredentials: NETWORK,
  mountNetworkShare: NETWORK,
  upgradeToSmbVolume: NETWORK,
  upgradeToSmbVolumeWithCredentials: NETWORK,
  systemHasSavedSmbPassword: NETWORK,
  upgradeToSmbVolumeUsingSavedPassword: NETWORK,
  getSmbDirectConnectionEnabled: NETWORK,
  setSmbDirectConnectionEnabled: NETWORK,
  getSmbShareParent: NETWORK,
  setSmbShareParentRow: NETWORK,
  reconnectVolume: NETWORK,
  reconnectVolumeWithCredentials: NETWORK,
  rememberVolumeSignIn: NETWORK,
  getVolumeSignInState: NETWORK,
  disconnectSmbVolume: NETWORK,
  ejectVolume: NETWORK,
  ejectAllOnDisk: NETWORK,
  setSmbAccountPreference: NETWORK,
  disconnectNetworkHost: NETWORK,
  smbHostHasMounts: NETWORK,
  noteNetworkAction: NETWORK,
  setNetworkEnabled: NETWORK,
  approveSftpHostKey: NETWORK,
  forgetSftpHostKey: NETWORK,
  listTrustedSftpHostKeys: NETWORK,
  saveSftpCredentials: NETWORK,
  getKnownSftpServers: NETWORK,
  getSftpUnattendedReconnect: NETWORK,
  approveWebdavCertificate: NETWORK,
  forgetWebdavCertificate: NETWORK,
  listTrustedWebdavCertificates: NETWORK,
  saveWebdavCredentials: NETWORK,
  getKnownWebdavServers: NETWORK,
  getWebdavUnattendedReconnect: NETWORK,
  saveS3Credentials: NETWORK,
  getS3UnattendedReconnect: NETWORK,
  getKnownS3Places: NETWORK,
  listNfsExports: NETWORK,
  guessNfsIdentity: NETWORK,
  getKnownNfsPlaces: NETWORK,
  listSavedServers: NETWORK,
  savedServerOfShareList: NETWORK,
  // Only the Servers list lands a cursor by a row's path
  findPathIndex: NETWORK,
  connectSavedPlace: NETWORK,
  connectServer: NETWORK,
  cancelServerConnect: NETWORK,
  disconnectPlace: NETWORK,
  setPlacePinned: NETWORK,
  setPlaceAutoReconnect: NETWORK,
  forgetServer: NETWORK,
  hasServerSecret: NETWORK,
  forgetServerSecret: NETWORK,
  updateSavedServer: NETWORK,
  savedServerId: NETWORK,
  updateSavedSmbHost: NETWORK,
  ejectAndUpdateSavedSmbHost: NETWORK,
  updateSavedS3Account: NETWORK,
  saveS3AccountRoot: NETWORK,
  forgetSavedSmbHost: NETWORK,
  forgetSavedSmbHostPassword: NETWORK,
  // A rename that runs as a move exists for S3 folders; the demo volume renames in place.
  renameByMove: NETWORK,
  // Phones and cameras
  setMtpEnabled: DEVICES,
  connectMtpDevice: DEVICES,
  getPtpcameradWorkaroundCommand: DEVICES,
  setAdbSettings: DEVICES,
  connectAdbDevice: DEVICES,
  cancelAdbConnect: DEVICES,
  getAdbInstallStatus: DEVICES,
  recheckAdbInstall: DEVICES,
  // Archives and encrypted containers
  unlockContainer: ARCHIVES,
  forgetContainerPassword: ARCHIVES,
  forgetSavedContainerPassword: ARCHIVES,
  cancelContainerUnlock: ARCHIVES,
  notifyArchivePasswordPrompt: ARCHIVES,
  notifyArchivePasswordDismissed: ARCHIVES,
  compressFiles: ARCHIVES,
  // Git
  subscribeGitState: GIT,
  unsubscribeGitState: GIT,
  fillGitCommitCounts: GIT,
  countAllGitCommits: GIT,
  getGitStatusForPaths: GIT,
  // The drive and media indexes
  startDriveIndex: INDEX,
  clearDriveIndex: INDEX,
  getIndexDiskUsage: INDEX,
  setIndexingEnabled: INDEX,
  getIndexDebugStatus: INDEX,
  enableDriveIndex: INDEX,
  disableDriveIndex: INDEX,
  forgetDriveIndex: INDEX,
  rescanDriveIndex: INDEX,
  setIndexPauseFreeSpacePercent: INDEX,
  setImageIndexEnabled: INDEX,
  mediaIndexSearchOcr: INDEX,
  mediaIndexThumbnailToken: INDEX,
  mediaIndexDropThumbnailTokens: INDEX,
  mediaIndexSetNetworkVolumeEnabled: INDEX,
  mediaIndexSetAlwaysIndexVolume: INDEX,
  mediaIndexSetAlwaysIndexFolder: INDEX,
  mediaIndexSetScope: INDEX,
  mediaIndexSetExcludedFolder: INDEX,
  mediaIndexSetImportanceThreshold: INDEX,
  mediaIndexSetParallelism: INDEX,
  mediaIndexMaxParallelism: INDEX,
  mediaIndexSetSemanticSearchEnabled: INDEX,
  mediaIndexCoveredCount: INDEX,
  mediaIndexReclaimPreview: INDEX,
  mediaIndexPruneBelowThreshold: INDEX,
  mediaIndexFindSimilar: INDEX,
  mediaIndexDedupClusters: INDEX,
  mediaIndexSearchTag: INDEX,
  mediaIndexSearchSemantic: INDEX,
  mediaIndexClipModelStatus: INDEX,
  mediaIndexDownloadClipModel: INDEX,
  mediaIndexDeleteClipModel: INDEX,
  mediaIndexFileStatus: INDEX,
  mediaIndexFolderCoverage: INDEX,
  // AI, Ask Cmdr, and the AI-backed tools
  getAiModelInfo: AI,
  // The local model's memory check (Settings > AI).
  getSystemMemoryInfo: AI,
  stopAiServer: AI,
  checkAiConnection: AI,
  getAiModelLimits: AI,
  cloudAiHostVerdicts: AI,
  acceptCloudAiConsent: AI,
  revokeCloudAiConsent: AI,
  cloudAiConsentRevokePendingChanged: AI,
  cancelAiDownload: AI,
  uninstallAi: AI,
  saveAiApiKey: AI,
  getAiApiKeyStatus: AI,
  deleteAiApiKey: AI,
  setLogLlmCalls: AI,
  answerToolAsk: AI,
  translateSearchQuery: AI,
  translateSelectionQuery: AI,
  askCmdrSendMessage: AI,
  askCmdrCancel: AI,
  askCmdrKeepGoing: AI,
  askCmdrRecordSlotChange: AI,
  askCmdrGetConversation: AI,
  askCmdrToolResult: AI,
  askCmdrListConversations: AI,
  askCmdrSearchConversations: AI,
  askCmdrRenameConversation: AI,
  askCmdrArchiveConversation: AI,
  askCmdrResolveAttachments: AI,
  askCmdrConversationCost: AI,
  askCmdrCostSummary: AI,
  askCmdrModelWindow: AI,
  askCmdrSlotStatus: AI,
  askCmdrMemoryFolder: AI,
  askCmdrForgetMemory: AI,
  preflightBulkRename: AI,
  applyBulkRename: AI,
  reviseBulkRenameRow: AI,
  cancelBulkRenameProposal: AI,
  cancelBulkRenameCheck: AI,
  suggestedOpsPage: AI,
  suggestedOpsLiveFacts: AI,
  suggestedOpsReject: AI,
  suggestedOpsApprove: AI,
  suggestedOpsApproveServer: AI,
  // License, updates, reports, and feedback
  verifyLicense: SERVICES,
  commitLicense: SERVICES,
  getLicenseInfo: SERVICES,
  markExpirationModalShown: SERVICES,
  markCommercialReminderDismissed: SERVICES,
  resetLicense: SERVICES,
  hasLicenseBeenValidated: SERVICES,
  validateLicenseWithServer: SERVICES,
  recordUpdateCheck: SERVICES,
  checkForUpdate: SERVICES,
  updateWriteBlocker: SERVICES,
  updateBundlePath: SERVICES,
  downloadUpdate: SERVICES,
  installUpdate: SERVICES,
  getSecurityAdvisories: SERVICES,
  betaSignup: SERVICES,
  dismissCrashReport: SERVICES,
  sendCrashReport: SERVICES,
  sendErrorReport: SERVICES,
  sendCrashLogReport: SERVICES,
  amendErrorReport: SERVICES,
  saveErrorReportToDisk: SERVICES,
  writeErrorReportPreviewFile: SERVICES,
  sendFeedback: SERVICES,
  writeFeedbackLogsPreviewFile: SERVICES,
  // Quitting, relaunching, and wiping the app's data
  quitConfirm: LIFECYCLE,
  quitCancel: LIFECYCLE,
  relaunchApp: LIFECYCLE,
  dataWipeSummary: LIFECYCLE,
  deleteAllAppData: LIFECYCLE,
  quitAfterDataWipe: LIFECYCLE,
  // Developer tooling
  getMcpRunning: DEV,
  getMcpPort: DEV,
  getMcpToken: DEV,
  checkPortAvailable: DEV,
  findAvailablePort: DEV,
  setLogLevel: DEV,
  getDebugLogPath: DEV,
  benchmarkLog: DEV,
  revealSecurityLog: DEV,
  setMaxLogStorageMb: DEV,
  createDialogGalleryFixtures: DEV,
  previewFriendlyError: DEV,
  getMemoryDiagnostics: DEV,
  // Settings for subsystems the demo doesn't run
  setDiskSpaceThreshold: SUBSYSTEM_SETTING,
  setLowDiskSpaceConfig: SUBSYSTEM_SETTING,
  setShowSafeSaveFilesCmd: SUBSYSTEM_SETTING,
  setShowStagingTempFilesCmd: SUBSYSTEM_SETTING,
  // The in-page menus' commands (D5)
  pickMenuBarItem: IN_PAGE_MENUS,
  openContextMenuInPage: IN_PAGE_MENUS,
  pickContextMenuItem: IN_PAGE_MENUS,
} satisfies Record<Unanswered, string>

/**
 * Commands outside the bindings (a raw `invoke`, a Tauri plugin's own) left out on purpose, by
 * wire name, each with its reason. `ipc-calls.test.ts` finds every one the app sends.
 */
export const wireNotInDemo: Readonly<Record<string, string>> = {
  // The new folder dialog's AI name suggestions and the local model's setup: `getAiStatus` says
  // unavailable, so the app offers neither.
  stream_folder_suggestions: AI,
  cancel_folder_suggestions: AI,
  start_ai_download: AI,
  start_ai_server: AI,
  set_mcp_enabled: DEV,
  set_mcp_port: DEV,
  prepare_error_report_preview: SERVICES,
  get_auto_sent_report_preview: SERVICES,
  // `app.html`'s boot guard quits an app whose webview is too old to run it.
  'plugin:process|exit': LIFECYCLE,
}

/** The reason the demo leaves `command` (a wire name) out, or `undefined` for one it doesn't know. */
export function whyNotInDemo(command: string): string | undefined {
  return (notInDemo as Partial<Record<string, string>>)[camelCase(command)] ?? wireNotInDemo[command]
}

/** Every wire name an engine or a page host answers (the backend's routing, `backend.ts`'s `invoke`). */
const wireAnswered: ReadonlySet<string> = new Set<string>([
  ...WIRE_ANSWERED,
  ...WEBVIEW_COMMANDS,
  ...WINDOW_PLUGIN_COMMANDS,
  SET_APP_THEME,
])

/** Whether the demo answers `command` (a wire name: a bindings command, a raw `invoke`, or a plugin's). */
export function isWireAnswered(command: string): boolean {
  return (
    wireAnswered.has(command) ||
    pluginHandler(command) !== undefined ||
    rawAnswers[command] !== undefined ||
    answeredCommands.has(camelCase(command))
  )
}

const STORE = ['load', 'get', 'set', 'has', 'delete', 'clear', 'reset', 'keys', 'values', 'entries', 'length', 'save']

/**
 * Every value the app imports from a Tauri JS package (`module#name`, as `ipc-calls.ts` reads
 * them), with the commands calling it sends: `[]` for one that sends none (a type-like value, a
 * transport). `ipc-calls.test.ts` fails on an import this doesn't name, and on a command here the
 * demo neither answers nor leaves out with a reason.
 */
export const TAURI_API_CALLS: Readonly<Record<string, readonly string[]>> = {
  // `invoke` is scanned call by call (`findRawInvokes`); a `Channel` rides along in an invoke's args.
  'api/core#invoke': [],
  'api/core#Channel': [],
  // Builds a URL, no IPC: `webview.ts` stands in for it.
  'api/core#convertFileSrc': [],
  'api/dpi#LogicalPosition': [],
  'api/dpi#LogicalSize': [],
  'api/window#Effect': [],
  'api/window#EffectState': [],
  'api/window#LogicalSize': [],
  'api/window#availableMonitors': ['plugin:window|available_monitors'],
  'api/window#getAllWindows': ['plugin:window|get_all_windows'],
  'api/event#listen': ['plugin:event|listen', 'plugin:event|unlisten'],
  'api/event#emit': ['plugin:event|emit'],
  'api/event#emitTo': ['plugin:event|emit_to'],
  'api/app#getVersion': ['plugin:app|version'],
  'api/app#setTheme': [SET_APP_THEME],
  'api/path#tempDir': ['plugin:path|resolve_directory'],
  'api/path#appLogDir': ['plugin:path|resolve_directory'],
  'api/path#join': ['plugin:path|join'],
  // `load` hands back a `Store`, whose methods send the rest.
  'plugin-store#load': STORE.map((command) => `plugin:store|${command}`),
  'plugin-fs#writeFile': ['plugin:fs|write_file'],
  'plugin-fs#remove': ['plugin:fs|remove'],
  'plugin-opener#openUrl': ['plugin:opener|open_url'],
  'plugin-opener#revealItemInDir': ['plugin:opener|reveal_item_in_dir'],
  // `ask` is a `message` with two buttons.
  'plugin-dialog#ask': ['plugin:dialog|message'],
  'plugin-dialog#open': ['plugin:dialog|open'],
  'plugin-dialog#save': ['plugin:dialog|save'],
}

/**
 * Tauri APIs that hand back a window or webview handle, whose methods send `plugin:window|*` and
 * `plugin:webview|*` commands. Which methods a caller uses can't be read off an import, so these
 * stay with the runtime net: an unanswered one lands in `window.__cmdrDemo.unanswered`, which the
 * website's demo suite fails on (`DETAILS.md` § Answering a command).
 */
export const TAURI_API_HANDLES: Readonly<Record<string, string>> = {
  'api/window#getCurrentWindow': 'the main window and each panel: `webview.ts` and `windows.ts` answer its methods',
  'api/webview#getCurrentWebview': "the page's own webview: `webview.ts` answers its methods",
  'api/webviewWindow#WebviewWindow': 'opens and finds panels: `windows.ts` answers its methods',
}
