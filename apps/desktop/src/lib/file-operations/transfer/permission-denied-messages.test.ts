import { describe, expect, it } from 'vitest'
import type { WriteOperationError } from '#lib/file-explorer/types.js'
import type { ProgressAtStop } from '#lib/tauri-commands/index.js'
import { formatInteger } from '#lib/intl/number-format.js'
import { getUserFriendlyMessage } from './transfer-error-messages'

describe('a refused permanent delete', () => {
  const error: WriteOperationError = {
    type: 'permission_denied',
    path: '/Applications/Test App.app/Contents/Helpers',
    message: 'Permission denied (os error 13)',
    errno: 13,
    refusal: 'folderPermissions',
    refusedFolder: '/Applications/Test App.app/Contents/Helpers',
    side: null,
  }
  const progress = (filesDone: number): ProgressAtStop => ({
    filesDone,
    filesTotal: 1529,
    bytesDone: 0,
    bytesTotal: 0,
    sourcesRemoved: null,
    sourcesLeft: null,
  })

  // A permanent delete can't be undone, so one the OS stopped partway says what's already gone.
  // ERR-RB2XQ (cmdr-reports#122): an app bundle's root-owned folder stopped it, and the dialog
  // said only that the folder was refused.
  it('says how many files it already removed', () => {
    expect(getUserFriendlyMessage(error, 'delete', progress(312)).message).toContain(
      `Cmdr had already deleted ${formatInteger(312)} of ${formatInteger(1529)} files when it stopped.`,
    )
  })

  it('says nothing about it when nothing was removed, or for another operation', () => {
    expect(getUserFriendlyMessage(error, 'delete', progress(0)).message).not.toContain('already deleted')
    expect(getUserFriendlyMessage(error, 'delete').message).not.toContain('already deleted')
    expect(getUserFriendlyMessage(error, 'copy', progress(312)).message).not.toContain('already deleted')
  })
})
