/**
 * Every command the app can call (`bindings.ts`), accounted for: answered by a TS table, an engine,
 * or a page host, or left out on purpose with the reason a browser can't. A new command that's in
 * none of those fails `svelte-check` here (`notInDemo` must name every unanswered one) and
 * `coverage.test.ts`, on the change that adds it: an unanswered pane-boot command once made every
 * pane say "didn't respond in time", and only the nightly demo suite would have caught it. See
 * `DETAILS.md` § Answering a command.
 */
import type { CommandName } from './answer-types.js'
import { appAnswers, silentCommands } from './answers.js'
import { DRAG_COMMANDS } from './drag-session.js'
import { LISTING_ENGINE_COMMANDS, MENU_ENGINE_COMMANDS } from './engine-command-names.js'
import { volumeAnswers } from './volumes.js'
import { WINDOW_HOST_COMMANDS } from './windows.js'

/** A wire name as the bindings spell it: `live_volume_path` → `liveVolumePath`. */
type CamelCase<S extends string> = S extends `${infer Head}_${infer Tail}` ? `${Head}${Capitalize<CamelCase<Tail>>}` : S

/** The wire (snake_case) names something other than a TS table answers. */
const WIRE_ANSWERED = [...LISTING_ENGINE_COMMANDS, ...MENU_ENGINE_COMMANDS, ...WINDOW_HOST_COMMANDS, ...DRAG_COMMANDS]

type Answered =
  | keyof typeof appAnswers
  | keyof typeof volumeAnswers
  | (typeof silentCommands)[number]
  | CamelCase<(typeof WIRE_ANSWERED)[number]>

/** The bindings commands nothing in the demo answers. */
type Unanswered = Exclude<CommandName, Answered>

const camelCase = (snake: string) => snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase())

/** Every bindings command the demo answers, by its bindings (camelCase) name. */
export const answeredCommands: ReadonlySet<string> = new Set<string>([
  ...Object.keys(appAnswers),
  ...Object.keys(volumeAnswers),
  ...silentCommands,
  ...WIRE_ANSWERED.map(camelCase),
])

// Why a command stays out. One line each, so a reader sees what the demo lacks at a glance.
const OS_SHELL =
  'asks the OS shell (Finder, Explorer, Quick Look, the Dock, System Settings), which a page has no door to'
const OS_CLIPBOARD = "the OS file clipboard: a page can't put files on it or read them back"
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
const NOT_YET = 'not built for the demo yet: no store behind it, so the app gets its "unavailable" path'
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
  confirmWithCheckbox: OS_SHELL,
  goToLatestDownload: OS_SHELL,
  downloadsWatcherStatus: OS_SHELL,
  recheckDownloadsWatcherGate: OS_SHELL,
  setGlobalGoToLatestShortcut: OS_SHELL,
  getSystemMemoryInfo: OS_SHELL,
  getMemoryDiagnostics: OS_SHELL,
  viewerForwardEditActionToPage: OS_SHELL,
  // The file clipboard
  copyPathsToClipboard: OS_CLIPBOARD,
  cutPathsToClipboard: OS_CLIPBOARD,
  readClipboardFiles: OS_CLIPBOARD,
  readClipboardText: OS_CLIPBOARD,
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
  prefetchShares: NETWORK,
  getKnownShareByName: NETWORK,
  updateKnownShare: NETWORK,
  getUsernameHint: NETWORK,
  allowSmbLinkTarget: NETWORK,
  forgetSmbLinkTarget: NETWORK,
  listSmbLinkConsents: NETWORK,
  saveSmbCredentials: NETWORK,
  getSmbCredentials: NETWORK,
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
  reconnectVolume: NETWORK,
  reconnectVolumeWithCredentials: NETWORK,
  rememberVolumeSignIn: NETWORK,
  getVolumeSignInState: NETWORK,
  disconnectSmbVolume: NETWORK,
  ejectVolume: NETWORK,
  ejectAllOnDisk: NETWORK,
  setSmbAccountPreference: NETWORK,
  disconnectNetworkHost: NETWORK,
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
  countFolderSizes: INDEX,
  cancelFolderSizeCount: INDEX,
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
  aboutCancel: AI,
  aboutStopVerdict: AI,
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
  quitApp: LIFECYCLE,
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
  // Settings for subsystems the demo doesn't run
  setDiskSpaceThreshold: SUBSYSTEM_SETTING,
  setLowDiskSpaceConfig: SUBSYSTEM_SETTING,
  setShowSafeSaveFilesCmd: SUBSYSTEM_SETTING,
  setShowStagingTempFilesCmd: SUBSYSTEM_SETTING,
  setOperationLogEnabled: SUBSYSTEM_SETTING,
  // Features the demo could have and doesn't yet
  persistRestrictedWindowSetting: NOT_YET,
  isSearchRunLive: NOT_YET,
  resolveGoToPath: NOT_YET,
  getRecentPaths: NOT_YET,
  addRecentPath: NOT_YET,
  removeRecentPath: NOT_YET,
  clearRecentPaths: NOT_YET,
  addFavorite: NOT_YET,
  removeFavorite: NOT_YET,
  renameFavorite: NOT_YET,
  setFavoriteShortcut: NOT_YET,
  reorderFavorites: NOT_YET,
  getRecentSelections: NOT_YET,
  addRecentSelection: NOT_YET,
  removeRecentSelection: NOT_YET,
  clearRecentSelections: NOT_YET,
  applyRecentSelectionsMaxCount: NOT_YET,
  // The in-page menus' commands (D5)
  getMenuBarTitleMnemonics: IN_PAGE_MENUS,
  pickMenuBarItem: IN_PAGE_MENUS,
  openContextMenuInPage: IN_PAGE_MENUS,
  pickContextMenuItem: IN_PAGE_MENUS,
} satisfies Record<Unanswered, string>

/** The reason the demo leaves `command` (a wire name) out, or `undefined` for one it doesn't know. */
export function whyNotInDemo(command: string): string | undefined {
  return (notInDemo as Partial<Record<string, string>>)[camelCase(command)]
}
