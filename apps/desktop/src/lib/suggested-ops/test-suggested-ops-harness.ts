/**
 * Shared IPC fakes and fixtures for the AI suggestions state tests
 * (`suggested-ops-trigger.svelte.test.ts`, `suggested-ops-live-facts.svelte.test.ts`).
 *
 * How a test file uses it (the factories are dynamically imported INSIDE the `vi.mock`
 * callbacks, so hoisting can't bite):
 *
 * ```ts
 * vi.mock('#lib/tauri-commands/index.js', async () => (await import('./test-suggested-ops-harness')).tauriCommandsMock())
 * vi.mock('#lib/logging/logger.js', async () => (await import('./test-suggested-ops-harness')).loggerMock())
 * beforeEach(() => { resetHarness(); closeSuggestedOps(); suggestedOpsState.sweeps = [] })
 * ```
 *
 * ❌ Nothing here may import a mocked module at runtime (`#lib/tauri-commands`, the logger, or
 * the trigger that reaches them): this module is what the mock factories load. Types only.
 */

import { vi } from 'vitest'
import type {
  ServerApprovalResultView,
  SuggestedOpPage,
  SuggestedSweepView,
  SuggestionsChanged,
} from '#lib/tauri-commands/index.js'

export const listMock = vi.fn<() => Promise<SuggestedSweepView[]>>()
export const pageMock =
  vi.fn<(payload: { groupId: number; offset: number; limit: number }) => Promise<SuggestedOpPage>>()
export const rejectMock = vi.fn<(g: number) => Promise<{ kind: string }>>()
export const approveMock =
  vi.fn<
    (payload: { groupId: number; revision: number; selection: Record<string, unknown> }) => Promise<{ kind: string }>
  >()
export const runsAsMock = vi.fn<(runsAs: string | null) => void>()
export const factsMock = vi.fn<(payload: { groupId: number; offset: number; limit: number }) => Promise<unknown>>()
export const approveServerMock =
  vi.fn<(payload: { groupId: number; revision: number }) => Promise<ServerApprovalResultView>>()

/** The subscribers the store installs while the dialog is open: suggestion changes and write settles. */
export const listeners: {
  emit: ((payload: SuggestionsChanged) => void) | null
  settle: (() => void) | null
} = { emit: null, settle: null }

export function tauriCommandsMock() {
  return {
    listSuggestedOps: () => listMock(),
    pageSuggestedOps: (g: number, o: number, l: number) => pageMock({ groupId: g, offset: o, limit: l }),
    rejectSuggestedGroup: (g: number) => rejectMock(g),
    approveSuggestedGroup: (g: number, revision: number, selection: Record<string, unknown>, runsAs: string | null) => {
      runsAsMock(runsAs)
      return approveMock({ groupId: g, revision, selection })
    },
    approveSuggestedServer: (g: number, revision: number) => approveServerMock({ groupId: g, revision }),
    liveSuggestedOpFacts: (g: number, o: number, l: number) => factsMock({ groupId: g, offset: o, limit: l }),
    onSuggestionsChanged: (cb: (p: SuggestionsChanged) => void) => {
      listeners.emit = cb
      return Promise.resolve(() => {
        listeners.emit = null
      })
    },
    onWriteSettled: (cb: () => void) => {
      listeners.settle = cb
      return Promise.resolve(() => {
        listeners.settle = null
      })
    },
  }
}

export function loggerMock() {
  return { getAppLogger: () => ({ warn: vi.fn(), info: vi.fn(), debug: vi.fn(), error: vi.fn() }) }
}

/** An op row as the toggles see it: an id, and whether an earlier review turned it off. */
export function pending(opId: number) {
  return {
    opId,
    sourcePath: `/x/${String(opId)}`,
    newName: null,
    status: 'pending',
    snapshotSize: null,
    snapshotModified: null,
  } as const
}
export function excluded(opId: number) {
  return { ...pending(opId), status: 'excluded' } as const
}

export function group(groupId: number, liveOpCount: number, overrides: Record<string, unknown> = {}) {
  return {
    groupId,
    sweepId: 1,
    verb: 'move',
    status: 'pending',
    displayName: 'five invoices',
    rationale: 'They all look like invoices.',
    sourceVolumeId: 'root',
    sourceFolder: '/Users/someone/Downloads',
    destination: '/Users/someone/Documents/Invoices',
    destinationVolumeName: 'Macintosh HD',
    reversible: 'restoreMove',
    destinationState: 'willBeCreated',
    destinationLandsOn: null,
    liveOpCount,
    totalOpCount: liveOpCount,
    retiredOpCount: 0,
    fromSelector: false,
    server: null,
    revision: 1,
    ...overrides,
  } as unknown as SuggestedSweepView['groups'][number]
}

export function sweep(groups: SuggestedSweepView['groups']): SuggestedSweepView {
  return { sweepId: 1, createdAt: 100, rationale: 'Ten new files in Downloads.', groups }
}

export function ops(from: number, count: number) {
  return Array.from({ length: count }, (_, i) => ({
    opId: from + i,
    sourcePath: `/Users/someone/Downloads/file-${String(from + i)}.dmg`,
    newName: null,
    status: 'pending',
    snapshotSize: 1024,
    snapshotModified: 1_780_000_000,
  }))
}

/** The group the default sweep holds: 60,000 ops, which is the size nothing may load whole. */
export const BIG = 7

/** Every fake back to its default answer: one 60,000-op group, every drawn file present. */
export function resetHarness(): void {
  vi.clearAllMocks()
  listMock.mockResolvedValue([sweep([group(BIG, 60_000)])])
  pageMock.mockImplementation(({ offset }) =>
    Promise.resolve({ ops: ops(offset, 200), offset, total: 60_000 } as unknown as SuggestedOpPage),
  )
  factsMock.mockImplementation(({ offset, limit }) =>
    Promise.resolve({
      kind: 'facts',
      facts: Array.from({ length: limit }, (_, i) => ({
        opId: offset + i,
        state: { kind: 'present', size: 2048, modified: 1_790_000_000 },
      })),
    }),
  )
  rejectMock.mockResolvedValue({ kind: 'rejected' })
  approveMock.mockResolvedValue({ kind: 'started' })
  approveServerMock.mockResolvedValue({ kind: 'saved', volumeId: null })
}
