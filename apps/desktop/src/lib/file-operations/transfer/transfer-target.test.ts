/** The single-item Copy/Move target the dialog resolves before preflight, on Windows paths (#1118). */
import { describe, it, expect, beforeEach, onTestFinished } from 'vitest'
import type { VolumeInfo } from '#lib/file-explorer/types.js'
import { withPlatform } from '#lib/platform/test-platform.js'
import { resolveTransferTarget } from './transfer-target'

const volumes: VolumeInfo[] = [
  { id: 'root', name: 'Local Disk (C:)', path: 'C:/', category: 'main_volume', isEjectable: false },
  { id: 'local-d', name: 'Backup (D:)', path: 'D:/', category: 'attached_volume', isEjectable: true },
  { id: 'mtp-1:65537', name: 'Phone', path: 'mtp://phone/65537', category: 'mobile_device', isEjectable: true },
]

function target(enteredPath: string, selectedVolumeId = 'root') {
  return resolveTransferTarget({
    enteredPath,
    sourceFolderPath: 'C:/Users/test_user_1',
    sourcePath: 'C:/Users/test_user_1/notes.txt',
    sourceVolumeId: 'root',
    selectedVolumeId,
    volumes,
    homePath: 'C:/Users/test_user_1',
  })
}

describe('a Windows destination', () => {
  beforeEach(() => {
    onTestFinished(withPlatform('windows'))
  })

  it('lands a typed drive path on its own drive, never under the source folder', () => {
    expect(target('D:\\backup\\notes.txt')).toEqual({
      parent: 'D:/backup',
      name: 'notes.txt',
      volumeId: 'local-d',
      path: '/backup',
      fullPath: 'D:/backup/notes.txt',
    })
  })

  it('reads a drive path the same with either separator', () => {
    expect(target('C:/Users/test_user_1/Documents/notes.txt')?.fullPath).toBe(
      'C:/Users/test_user_1/Documents/notes.txt',
    )
    expect(target('c:\\Users\\test_user_1\\Documents\\notes.txt')?.fullPath).toBe(
      'C:/Users/test_user_1/Documents/notes.txt',
    )
  })

  it('takes a drive path while a phone is the picked volume', () => {
    expect(target('D:\\notes.txt', 'mtp-1:65537')).toMatchObject({ volumeId: 'local-d', fullPath: 'D:/notes.txt' })
  })

  it('takes a drive path for an item copied off a phone', () => {
    const fromPhone = resolveTransferTarget({
      enteredPath: 'D:\\photos\\IMG_1.JPG',
      sourceFolderPath: 'mtp://phone/65537/DCIM',
      sourcePath: 'mtp://phone/65537/DCIM/IMG_1.JPG',
      sourceVolumeId: 'mtp-1:65537',
      selectedVolumeId: 'mtp-1:65537',
      volumes,
      homePath: 'C:/Users/test_user_1',
    })
    expect(fromPhone).toMatchObject({ volumeId: 'local-d', path: '/photos', fullPath: 'D:/photos/IMG_1.JPG' })
  })

  it('resolves a relative path from the source folder', () => {
    expect(target('..\\notes copy.txt')).toMatchObject({ volumeId: 'root', fullPath: 'C:/Users/notes copy.txt' })
  })

  it('puts the item into a folder the path ends in a separator at', () => {
    expect(target('D:\\backup\\')).toMatchObject({ name: 'notes.txt', fullPath: 'D:/backup/notes.txt' })
  })

  // A POSIX path is the picked drive's own, as `wire_path::anchored` reads it. Collapsed
  // to `/`, a copy typed into `\Users\x\` landed in `C:\`.
  it.each(['/Users/x/', '\\Users\\x\\'])('reads %s as a folder on the picked drive, never its root', (typed) => {
    expect(target(typed)).toMatchObject({ volumeId: 'root', path: '/Users/x', fullPath: 'C:/Users/x/notes.txt' })
  })
})

/**
 * #1384: a share path stays on its share, end to end. The destination used to be
 * picked among LOCAL-kind rows only, so a share (a mapped drive's row is a network
 * one, and a share nobody mapped has no row at all) fell back to the boot drive, and
 * the off-volume path collapsed to its root: `C:/`.
 */
describe('a Windows destination on a share', () => {
  beforeEach(() => {
    onTestFinished(withPlatform('windows'))
  })

  const shareId = 'smb-testnas-local-445-media'
  const mapped: VolumeInfo = {
    id: shareId,
    name: 'media (\\\\testnas.local) (Z:)',
    path: 'Z:/',
    category: 'network',
    fsType: 'smbfs',
    isEjectable: false,
  }

  function onShare(enteredPath: string, selectedVolumeId: string, list: VolumeInfo[]) {
    return resolveTransferTarget({
      enteredPath,
      sourceFolderPath: 'C:/Users/test_user_1',
      sourcePath: 'C:/Users/test_user_1/notes.txt',
      sourceVolumeId: 'root',
      selectedVolumeId,
      volumes: list,
      homePath: 'C:/Users/test_user_1',
    })
  }

  it('keeps a share nobody mapped (no row) as the volume, its UNC path whole', () => {
    expect(onShare('//testnas.local/media/dst/', shareId, volumes)).toMatchObject({
      volumeId: shareId,
      fullPath: '//testnas.local/media/dst/notes.txt',
    })
  })

  it('lands a mapped drive path on the mapped drive, never on the boot drive', () => {
    expect(onShare('Z:\\dst\\', 'root', [...volumes, mapped])).toMatchObject({
      volumeId: shareId,
      path: '/dst',
      fullPath: 'Z:/dst/notes.txt',
    })
  })

  it('never hangs a path no listed volume holds off the boot drive root', () => {
    const resolved = onShare('F:/x/', 'root', volumes)
    expect(resolved?.fullPath).toBe('F:/x/notes.txt')
    expect(resolved?.path).toBe('F:/x')
  })
})
