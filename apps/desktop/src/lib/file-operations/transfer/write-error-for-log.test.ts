import { describe, expect, it } from 'vitest'

import { describeForLog } from '#lib/logging/describe-for-log.js'
import { stripLogMarkers } from '#lib/logging/log-markers.js'
import type { WriteOperationError } from '../write-operation-types'
import { writeErrorForLog } from './write-error-for-log'

describe('writeErrorForLog', () => {
  /**
   * ERR-RB2XQ: the frontend's `Trash error` line carried macOS's sentence, and in it the
   * item's name in clear (“Test App.app” couldn’t be moved…). Nothing in the payload keys
   * that name, so neither this side nor a report could tell it from the words around it.
   */
  it('logs a refused trash without the OS sentence that names the item', () => {
    const error: WriteOperationError = {
      type: 'trash_refused',
      itemCount: 1,
      reason: 'trashNotWritable',
      codes: 'NSCocoaErrorDomain 513 < NSOSStatusErrorDomain -5000',
      message:
        '/Applications/Test App.app: the Trash refused it (TrashNotWritable): “Test App.app” couldn’t be moved to the trash because you don’t have permission to access it.',
      onlineOnly: false,
    }

    const logged = stripLogMarkers(describeForLog(writeErrorForLog(error)))

    expect(logged).not.toContain('Test App')
    expect(logged).toBe(
      '{"type":"trash_refused","itemCount":1,"reason":"trashNotWritable","codes":"NSCocoaErrorDomain 513 < NSOSStatusErrorDomain -5000","onlineOnly":false}',
    )
  })

  it('keeps every typed field, marked paths included, and drops the words at every depth', () => {
    const error: WriteOperationError = {
      type: 'originals_kept_aside',
      cause: { type: 'write_error', path: '/Volumes/Backup/Report.pdf', message: 'couldn’t write “Report.pdf”' },
      recovered: [],
    }

    const logged = writeErrorForLog(error)

    expect(logged).toEqual({
      type: 'originals_kept_aside',
      cause: { type: 'write_error', path: '/Volumes/Backup/Report.pdf' },
      recovered: [],
    })
  })
})
