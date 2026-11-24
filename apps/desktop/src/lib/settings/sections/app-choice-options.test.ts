import { describe, expect, it } from 'vitest'
import { appPickerShape } from './app-choice-options'

describe('appPickerShape', () => {
  it('picks a .app bundle from /Applications on macOS', () => {
    expect(appPickerShape('appBundle')).toEqual({ extensions: ['app'], defaultPath: '/Applications' })
  })

  it('picks an .exe on Windows and leaves the start folder to the dialog', () => {
    expect(appPickerShape('exe')).toEqual({ extensions: ['exe'], defaultPath: undefined })
  })

  it('lets any file through where no extension names an app', () => {
    expect(appPickerShape('executable')).toEqual({ extensions: null, defaultPath: undefined })
  })
})
