import { describe, expect, it } from 'vitest'
import { accessKeyCandidates, accessKeyLettersOf, allocateAccessKeys, reservedAccessKeys } from './access-keys'

describe('allocateAccessKeys', () => {
  // The same rule as the Linux menu bar (`src-tauri/src/menu/mnemonics.rs`), case for case.
  it('lands on the first free word initial', () => {
    expect(allocateAccessKeys(['New folder…', 'Delete'])).toEqual(['n', 'd'])
  })

  it('moves to the next word, then to a later letter, before giving up', () => {
    expect(allocateAccessKeys(['Copy path', 'Copy filename', 'Copy'])).toEqual(['c', 'f', 'o'])
  })

  it('treats upper and lower case as one key', () => {
    expect(allocateAccessKeys(['Open', 'orange'])).toEqual(['o', 'r'])
  })

  it('gives a label with nothing free no key', () => {
    expect(allocateAccessKeys(['ab', 'ba', 'aabb'])).toEqual(['a', 'b', null])
  })

  it('never hands out a letter the dialog reserved for its own command', () => {
    expect(allocateAccessKeys(['Regular expression', 'Rename'], ['r'])).toEqual(['e', 'n'])
  })

  it('keeps accented and non-Latin letters a keyboard has', () => {
    expect(allocateAccessKeys(['Újranevezés', 'Отмена', 'Ändern'])).toEqual(['ú', 'о', 'ä'])
  })

  it('gives a label without a pressable letter one from a to z, as Windows does for Chinese and Japanese', () => {
    // A Latin label beside them still keeps to its own letters.
    expect(allocateAccessKeys(['保存', 'キャンセル', 'Abbrechen'])).toEqual(['a', 'b', 'r'])
  })

  it('skips digits and punctuation as word boundaries', () => {
    expect(allocateAccessKeys(['(Re)try', '7-Zip'])).toEqual(['r', '7'])
  })
})

describe('accessKeyLettersOf', () => {
  const key = (init: KeyboardEventInit) => new KeyboardEvent('keydown', init)

  it('reads Alt plus a letter, by what it typed and by where it sits', () => {
    expect(accessKeyLettersOf(key({ key: 'R', code: 'KeyR', altKey: true }))).toEqual(['r', 'r'])
    // A Mac's Option key composes `®`: the key's position still names R.
    expect(accessKeyLettersOf(key({ key: '®', code: 'KeyR', altKey: true }))).toEqual(['®', 'r'])
    expect(accessKeyLettersOf(key({ key: '7', code: 'Digit7', altKey: true }))).toEqual(['7', '7'])
  })

  it('reads nothing without Alt, or with another modifier beside it', () => {
    expect(accessKeyLettersOf(key({ key: 'r', code: 'KeyR' }))).toEqual([])
    expect(accessKeyLettersOf(key({ key: 'r', code: 'KeyR', altKey: true, ctrlKey: true }))).toEqual([])
    expect(accessKeyLettersOf(key({ key: 'R', code: 'KeyR', altKey: true, shiftKey: true }))).toEqual([])
    expect(accessKeyLettersOf(key({ key: 'r', code: 'KeyR', altKey: true, metaKey: true }))).toEqual([])
  })

  // Windows types a character from its code with Alt held and digits on the numpad (Alt, 0, 2, 3, 3 for é): each
  // digit arrives as an Alt keydown, which must not press a button whose access key is that digit.
  it('reads nothing from the numpad, where Alt types a character code', () => {
    for (const digit of ['0', '2', '3']) {
      expect(accessKeyLettersOf(key({ key: digit, code: `Numpad${digit}`, altKey: true }))).toEqual([])
    }
    // NumLock off: the same keys arrive as navigation keys.
    expect(accessKeyLettersOf(key({ key: 'End', code: 'Numpad1', altKey: true }))).toEqual([])
  })
})

describe('accessKeyCandidates', () => {
  function scope(html: string): HTMLElement {
    const el = document.createElement('div')
    el.innerHTML = html
    document.body.appendChild(el)
    return el
  }

  it('takes each reachable labeled control in DOM order, with the label a person reads', () => {
    const el = scope(`
      <label for="f">File name</label><input id="f" />
      <button>Rename <span class="btn-key-hint" aria-hidden="true"><kbd>F2</kbd></span></button>
      <button disabled>Disabled</button>
      <button tabindex="-1">×</button>
      <div hidden><button>Hidden</button></div>
      <button aria-label="Close"></button>
      <button><span class="sr-only">Only for screen readers</span></button>
    `)
    expect(accessKeyCandidates(el).map((c) => [c.control.tagName, c.label])).toEqual([
      ['INPUT', 'File name'],
      ['BUTTON', 'Rename'],
    ])
    el.remove()
  })

  it('anchors a field on its label', () => {
    const el = scope(`<label for="g">Go to</label><input id="g" />`)
    const [candidate] = accessKeyCandidates(el)
    expect(candidate.anchor.tagName).toBe('LABEL')
    el.remove()
  })
})

describe('reservedAccessKeys', () => {
  it('reserves the Alt letters a dialog scope binds on this platform', () => {
    // Vitest runs as Linux, whose keymap binds the sheet's options to Alt plus a letter.
    const reserved = reservedAccessKeys('Main window/Multi-rename')
    for (const letter of ['u', 'n', 'g', 'p', 'i', 'f', 'e', 'r', 'w', 'x', 'z'])
      expect(reserved.has(letter)).toBe(true)
    expect(reservedAccessKeys('Main window/Tell me about this')).toEqual(new Set(['s']))
    expect(reservedAccessKeys(undefined)).toEqual(new Set())
  })
})
