/**
 * Access keys (mnemonics) for a dialog, on the platforms that have them (`convention('accessKeys')`
 * isn't `'none'`: Windows and every Linux desktop). Alt plus a control's letter presses it (a
 * button, a checkbox) or focuses it (a text field). Holding Alt underlines each letter in its
 * label, as Win32, GTK, and Qt draw it, with a label that lacks its letter carrying it in
 * parentheses ("保存 (A)"). Where the Highlight API is missing (WebKitGTK), a keytip badge by each
 * control stands in. macOS has none, and `ModalDialog` builds no controller there.
 *
 * The letters are allocated from the labels as rendered, in the user's language, by the same rule
 * the Linux menu bar uses (`src-tauri/src/menu/mnemonics.rs`): word initials first, then any other
 * letter, unique per dialog. A translator never places a marker. A dialog's own Alt+letter
 * commands (multi-rename's options, About this's Stop) keep their letters: they're the dialog's
 * access keys already, so the allocator hands those letters to nobody else.
 *
 * Mechanics and decisions: `DETAILS.md` § Access keys.
 */

import { commands } from '#lib/commands/command-registry.js'
import type { CommandScope } from '#lib/commands/types.js'
// eslint-disable-next-line cmdr/no-raw-shortcut-text -- read as data to reserve letters, never shown as text
import { getEffectiveShortcuts } from '#lib/shortcuts/shortcuts-store.js'
import { isAltGraphHeld } from '#lib/shortcuts/key-capture.js'

/** A letter a person can press: Latin, Greek, or Cyrillic (with any diacritics), or a digit. */
const PRESSABLE = /^[\p{Script=Latin}\p{Script=Greek}\p{Script=Cyrillic}\p{Nd}]$/u
/** Any letter or digit, for telling where a word starts. */
const ALPHANUMERIC = /^[\p{L}\p{N}]$/u
/** The letters a label without any pressable one (Chinese, Japanese, Korean) draws from, as Windows does. */
const FALLBACK_LETTERS = 'abcdefghijklmnopqrstuvwxyz'

/** Character indices that could carry the access key, best first: word initials, then the rest. */
function candidatesIn(chars: readonly string[]): number[] {
  const usable = chars.flatMap((char, index) => (PRESSABLE.test(char) ? [index] : []))
  const initial = usable.filter((index) => index === 0 || !ALPHANUMERIC.test(chars[index - 1]))
  return [...initial, ...usable.filter((index) => !initial.includes(index))]
}

/**
 * One access key per label, in order, never one already `taken` (lowercase) or handed out
 * earlier: the label's first free word initial, else its first free letter, else nothing. A
 * label with no pressable letter at all takes the first free one of `a`–`z`. Pure.
 */
export function allocateAccessKeys(labels: readonly string[], taken: Iterable<string> = []): (string | null)[] {
  const used = new Set(taken)
  return labels.map((label) => {
    // Code points: an access key is one key, so a letter its combining marks compose into (NFC) is one too.
    const chars = Array.from(label.normalize('NFC'))
    const candidates = candidatesIn(chars)
    const pool =
      candidates.length > 0 ? candidates.map((index) => chars[index].toLowerCase()) : Array.from(FALLBACK_LETTERS)
    const letter = pool.find((candidate) => !used.has(candidate)) ?? null
    if (letter !== null) used.add(letter)
    return letter
  })
}

/** The letters `scope`'s own commands bind as Alt plus one key (`Alt+S`), lowercase. */
export function reservedAccessKeys(scope: CommandScope | undefined): Set<string> {
  const reserved = new Set<string>()
  if (scope === undefined) return reserved
  for (const command of commands) {
    if (command.scope !== scope) continue
    for (const shortcut of getEffectiveShortcuts(command.id)) {
      const match = /^Alt\+(.)$/u.exec(shortcut)
      if (match) reserved.add(match[1].toLowerCase())
    }
  }
  return reserved
}

/**
 * The letters an Alt+key press may name, most likely first: the key it typed, then the key's
 * position (`KeyR`), for a layout or a Mac Option key that composes another character. Empty for
 * anything but Alt alone plus one key (AltGr types characters, it never presses access keys), and for any numpad key:
 * Alt plus numpad digits is how Windows types a character by its code (Alt, 0, 2, 3, 3 for é), so a digit there is
 * part of a character, never an access key.
 */
export function accessKeyLettersOf(event: KeyboardEvent): string[] {
  if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || isAltGraphHeld(event)) return []
  if (event.code.startsWith('Numpad')) return []
  const letters: string[] = []
  if (Array.from(event.key).length === 1) letters.push(event.key.toLowerCase())
  const position = /^(?:Key|Digit)([A-Z0-9])$/.exec(event.code)
  if (position) letters.push(position[1].toLowerCase())
  return letters
}

/** A control that can carry an access key, the element its keytip sits by, and its label. */
export interface AccessKeyCandidate {
  control: HTMLElement
  anchor: HTMLElement
  label: string
}

const CONTROL_SELECTOR = [
  'button',
  'a[href]',
  'input:not([type="hidden"])',
  'textarea',
  '[role="button"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="radio"]',
  '[role="tab"]',
  '[role="combobox"]',
].join(', ')

/** The class of the "(A)" an underline desktop appends to a label that lacks its letter. */
const ACCESS_KEY_SUFFIX_CLASS = 'access-key-suffix'

/** The `::highlight()` name the underlines paint under; global per document, so it carries the app's prefix. */
const ACCESS_KEY_HIGHLIGHT = 'cmdr-access-key'

/**
 * Text that no sighted person reads as the label: hidden, key caps, screen-reader-only, icons. And
 * the "(A)" this module appends, which is the key, not the label (reading it back would hand the
 * label a letter it doesn't have).
 */
const UNSEEN_SELECTOR = `[aria-hidden="true"], [hidden], kbd, .sr-only, svg, .${ACCESS_KEY_SUFFIX_CLASS}`

/** The text nodes in `el` a person reads as its label, in order. */
function visibleTextNodes(el: Element): Text[] {
  const nodes: Text[] = []
  const walker = el.ownerDocument.createTreeWalker(el, NodeFilter.SHOW_TEXT)
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const unseen = node.parentElement?.closest(UNSEEN_SELECTOR)
    if (unseen && unseen !== el && el.contains(unseen)) continue
    nodes.push(node as Text)
  }
  return nodes
}

/** The text a person sees in `el`, collapsed. */
function visibleText(el: Element): string {
  return visibleTextNodes(el)
    .map((node) => node.data)
    .join('')
    .replace(/\s+/g, ' ')
    .trim()
}

/**
 * Where `letter` (lowercase) sits in `anchor`'s visible label, as the allocator picked it: its
 * first word-initial occurrence, else its first occurrence. `null` when the label lacks it (a
 * Chinese label's fallback letter), or spells it in a way one code point can't match.
 */
function accessKeyRangeIn(anchor: Element, letter: string): Range | null {
  let first: { node: Text; offset: number; length: number } | null = null
  let previous = ''
  for (const node of visibleTextNodes(anchor)) {
    let offset = 0
    for (const char of node.data) {
      if (char.normalize('NFC').toLowerCase() === letter) {
        const hit = { node, offset, length: char.length }
        const initial = previous === '' || !ALPHANUMERIC.test(previous)
        if (initial) return rangeOver(hit)
        first ??= hit
      }
      previous = char
      offset += char.length
    }
  }
  return first ? rangeOver(first) : null
}

function rangeOver({ node, offset, length }: { node: Text; offset: number; length: number }): Range {
  const range = node.ownerDocument.createRange()
  range.setStart(node, offset)
  range.setEnd(node, offset + length)
  return range
}

/** The element whose text names `control` on screen, and that text. */
function labelOf(control: HTMLElement): { anchor: HTMLElement; label: string } | null {
  const labelledBy = control.getAttribute('aria-labelledby')
  const labels = 'labels' in control ? Array.from((control as HTMLInputElement).labels ?? []) : []
  const named = labels[0] ?? (labelledBy ? control.ownerDocument.getElementById(labelledBy.split(/\s+/)[0]) : null)
  if (named instanceof HTMLElement) {
    const label = visibleText(named)
    if (label !== '') return { anchor: named, label }
  }
  const own = visibleText(control)
  return own === '' ? null : { anchor: control, label: own }
}

function isAvailable(control: HTMLElement): boolean {
  if (control.matches(':disabled') || control.getAttribute('tabindex') === '-1') return false
  if (control.closest('[hidden], [inert], [aria-hidden="true"]') !== null) return false
  return typeof control.checkVisibility === 'function' ? control.checkVisibility() : true
}

/** Every labeled control in `scope` a person can reach, in DOM order. */
export function accessKeyCandidates(scope: HTMLElement): AccessKeyCandidate[] {
  const controls = Array.from(scope.querySelectorAll<HTMLElement>(CONTROL_SELECTOR)).filter(isAvailable)
  return controls
    .filter((control) => !controls.some((other) => other !== control && other.contains(control)))
    .flatMap((control) => {
      const named = labelOf(control)
      return named ? [{ control, ...named }] : []
    })
}

/** Where a keytip shows, relative to the layer that draws it. */
export interface AccessKeyTip {
  letter: string
  x: number
  y: number
}

/** A control's access key: what Alt+`letter` presses. */
export interface AccessKeyAssignment extends AccessKeyCandidate {
  letter: string
}

/** The candidates in `scope` with the letters they get, `reserved` ones left to their commands. */
export function assignAccessKeys(scope: HTMLElement, reserved: ReadonlySet<string>): AccessKeyAssignment[] {
  const candidates = accessKeyCandidates(scope)
  const letters = allocateAccessKeys(
    candidates.map((candidate) => candidate.label),
    reserved,
  )
  return candidates.flatMap((candidate, index) => {
    const letter = letters[index]
    return letter === null ? [] : [{ ...candidate, letter }]
  })
}

/** A field a person types into: Alt+letter focuses it rather than clicking it. */
function takesText(control: HTMLElement): boolean {
  if (control instanceof HTMLTextAreaElement) return true
  if (!(control instanceof HTMLInputElement)) return false
  return !['checkbox', 'radio', 'button', 'submit', 'reset', 'image', 'file', 'color', 'range'].includes(control.type)
}

/** Presses `control` as its access key does: focus a field, focus and click anything else. */
export function pressAccessKey(control: HTMLElement): void {
  control.focus()
  if (!takesText(control)) control.click()
}

interface HighlightRegistry {
  set: (name: string, highlight: unknown) => void
  delete: (name: string) => void
}

type HighlightConstructor = new (...ranges: Range[]) => unknown

/** The CSS Custom Highlight API, which paints an underline without touching the label's text nodes. */
function highlightApi(): { registry: HighlightRegistry; Highlight: HighlightConstructor } | null {
  const registry = (globalThis.CSS as { highlights?: HighlightRegistry } | undefined)?.highlights
  const Highlight = (globalThis as { Highlight?: HighlightConstructor }).Highlight
  if (!registry || !Highlight) return null
  return { registry, Highlight }
}

/** The "(A)" an underline desktop puts after `anchor`'s label, if there is one. */
function suffixOf(anchor: Element): HTMLElement | null {
  const last = anchor.lastElementChild
  return last instanceof HTMLElement && last.classList.contains(ACCESS_KEY_SUFFIX_CLASS) ? last : null
}

/** The underline under `letter` in `anchor`: in its label, else in the "(A)" after it. */
function underlineRangeOf(anchor: Element, letter: string): Range | null {
  const inLabel = accessKeyRangeIn(anchor, letter)
  if (inLabel) return inLabel
  const text = suffixOf(anchor)?.firstChild
  if (!(text instanceof Text)) return null
  const at = text.data.toLowerCase().indexOf(letter)
  return at < 0 ? null : rangeOver({ node: text, offset: at, length: letter.length })
}

export interface AccessKeysOptions {
  /** The panel whose controls get access keys. */
  scope: () => HTMLElement | undefined
  /** The element the keytips are positioned against. */
  layer: () => HTMLElement | undefined
  /** The dialog's own command scope, whose Alt+letter commands keep their letters. */
  commandScope: () => CommandScope | undefined
  /** The keytips to draw now; empty hides them. */
  onTips: (tips: AccessKeyTip[]) => void
}

export interface AccessKeys {
  /**
   * The dialog's keydown, AFTER the dialog's own handler had its turn (a key it claimed is
   * `defaultPrevented` and passes untouched). True when the key was an access key's.
   */
  handleKeydown: (event: KeyboardEvent) => boolean
  /** Starts following the dialog (Alt released, focus lost, labels changing); returns the teardown. */
  attach: () => () => void
}

/** The value a keytip's control announces, `Alt+R`, in ARIA's spelling. */
function shortcutOf(letter: string): string {
  return `Alt+${letter.toUpperCase()}`
}

/** What this controller wrote on a control: the dialog's own value under it, and the merged one. */
interface Announced {
  base: string | null
  written: string
}

export function createAccessKeys(options: AccessKeysOptions): AccessKeys {
  let shown = false
  /**
   * The `aria-keyshortcuts` this controller wrote, so it never overwrites or leaves behind a
   * dialog's own: a default button's `Enter` stays and gains `Alt+S` beside it.
   */
  const announced = new Map<HTMLElement, Announced>()
  /** The "(A)"s this controller appended, removed with it. */
  const suffixes = new Set<HTMLElement>()
  /** Underlines need the Highlight API; without it (WebKitGTK), the keytips draw instead. */
  const highlights = highlightApi()

  function assignments(): AccessKeyAssignment[] {
    const scope = options.scope()
    return scope ? assignAccessKeys(scope, reservedAccessKeys(options.commandScope())) : []
  }

  function hide(): void {
    if (!shown) return
    shown = false
    if (highlights) highlights.registry.delete(ACCESS_KEY_HIGHLIGHT)
    else options.onTips([])
  }

  function show(): void {
    if (highlights) {
      shown = true
      const ranges = assignments().flatMap(({ anchor, letter }) => underlineRangeOf(anchor, letter) ?? [])
      highlights.registry.set(ACCESS_KEY_HIGHLIGHT, new highlights.Highlight(...ranges))
      return
    }
    const layer = options.layer()
    if (!layer) return
    const origin = layer.getBoundingClientRect()
    shown = true
    options.onTips(
      assignments().map(({ anchor, letter }) => {
        const rect = anchor.getBoundingClientRect()
        return {
          letter: letter.toUpperCase(),
          x: rect.left + rect.width / 2 - origin.left,
          y: rect.bottom - origin.top,
        }
      }),
    )
  }

  /**
   * On an underline desktop, a label that lacks its letter carries it after the label, "保存 (A)",
   * all the time, as GTK and Qt apps write a CJK label's mnemonic. `aria-hidden`: the key is
   * announced through `aria-keyshortcuts`, and read into the name it would garble it.
   */
  function placeSuffixes(all: readonly AccessKeyAssignment[]): void {
    const wanted = new Map<HTMLElement, string>()
    if (highlights) {
      for (const { anchor, letter } of all) {
        if (accessKeyRangeIn(anchor, letter) === null) wanted.set(anchor, ` (${letter.toUpperCase()})`)
      }
    }
    for (const suffix of suffixes) {
      const anchor = suffix.parentElement
      if (anchor && wanted.get(anchor) === suffix.textContent && suffixOf(anchor) === suffix) continue
      suffix.remove()
      suffixes.delete(suffix)
    }
    for (const [anchor, text] of wanted) {
      if (suffixOf(anchor)?.textContent === text) continue
      const suffix = anchor.ownerDocument.createElement('span')
      suffix.className = ACCESS_KEY_SUFFIX_CLASS
      suffix.setAttribute('aria-hidden', 'true')
      suffix.textContent = text
      anchor.append(suffix)
      suffixes.add(suffix)
    }
  }

  /** Tells screen readers each control's key, beside any key the dialog named itself. */
  function announce(): void {
    const all = assignments()
    placeSuffixes(all)
    const next = new Map(all.map(({ control, letter }) => [control, shortcutOf(letter)]))
    for (const [control, entry] of announced) {
      // Rewritten since (the dialog changed its own key): it's theirs now, merged afresh below.
      if (control.getAttribute('aria-keyshortcuts') !== entry.written) {
        announced.delete(control)
        continue
      }
      if (next.has(control) && entry.written === merged(entry.base, next.get(control) ?? '')) continue
      restore(control, entry)
      announced.delete(control)
    }
    for (const [control, value] of next) {
      if (announced.has(control)) continue
      const base = control.getAttribute('aria-keyshortcuts')
      // A dialog that named its own Alt key for this control keeps it alone.
      if (base !== null && /(^|\s)Alt\+/u.test(base)) continue
      const written = merged(base, value)
      control.setAttribute('aria-keyshortcuts', written)
      announced.set(control, { base, written })
    }
  }

  function handleKeydown(event: KeyboardEvent): boolean {
    if (event.key === 'Alt') {
      if (!event.repeat && !event.ctrlKey && !event.metaKey && !event.shiftKey) show()
      return false
    }
    const letters = accessKeyLettersOf(event)
    hide()
    if (letters.length === 0 || event.defaultPrevented) return false
    const reserved = reservedAccessKeys(options.commandScope())
    if (letters.some((letter) => reserved.has(letter))) return false
    const all = assignments()
    const hit = letters.map((letter) => all.find((a) => a.letter === letter)).find((a) => a !== undefined)
    if (!hit) return false
    event.preventDefault()
    pressAccessKey(hit.control)
    return true
  }

  function attach(): () => void {
    const onKeyup = (event: KeyboardEvent) => {
      if (event.key === 'Alt') hide()
    }
    window.addEventListener('keyup', onKeyup, true)
    window.addEventListener('blur', hide)
    window.addEventListener('pointerdown', hide, true)
    let frame: number | null = null
    const scheduleAnnounce = () => {
      if (frame !== null) return
      frame = requestAnimationFrame(() => {
        frame = null
        announce()
      })
    }
    const scope = options.scope()
    const observer = new MutationObserver(scheduleAnnounce)
    // `aria-keyshortcuts` too: a dialog that rewrites a control's own key gets its Alt key merged back in.
    if (scope) {
      observer.observe(scope, {
        childList: true,
        subtree: true,
        characterData: true,
        attributes: true,
        attributeFilter: ['aria-keyshortcuts'],
      })
    }
    announce()
    return () => {
      window.removeEventListener('keyup', onKeyup, true)
      window.removeEventListener('blur', hide)
      window.removeEventListener('pointerdown', hide, true)
      observer.disconnect()
      if (frame !== null) cancelAnimationFrame(frame)
      for (const [control, entry] of announced) {
        if (control.getAttribute('aria-keyshortcuts') === entry.written) restore(control, entry)
      }
      announced.clear()
      for (const suffix of suffixes) suffix.remove()
      suffixes.clear()
      hide()
    }
  }

  return { handleKeydown, attach }
}

/** A dialog's own `aria-keyshortcuts` with the access key added after it. */
function merged(base: string | null, value: string): string {
  return base === null || base.trim() === '' ? value : `${base} ${value}`
}

/** Puts back what the dialog had on `control` before this controller wrote there. */
function restore(control: HTMLElement, entry: Announced): void {
  if (entry.base === null) control.removeAttribute('aria-keyshortcuts')
  else control.setAttribute('aria-keyshortcuts', entry.base)
}
