/**
 * The window-wide right-click decision: a text field keeps WebKit's own editing
 * menu (Cut / Copy / Paste / Select All), everything else keeps Cmdr's menus only.
 *
 * The load-bearing case is the TARGET, not focus: a right-click lands on a field
 * that isn't focused yet, so `isTextInputFocused()` would say "no" and the field
 * would lose its menu.
 */
import { describe, it, expect, afterEach } from 'vitest'
import { installContextMenuOwnership, resolveGlobalContextMenuAction } from './context-menu-ownership'

describe('installContextMenuOwnership', () => {
  let uninstall: (() => void) | undefined

  afterEach(() => {
    uninstall?.()
    uninstall = undefined
    document.body.replaceChildren()
  })

  /** Dispatches a right-click on `el` and reports what the webview would do with it. */
  function rightClick(el: Element): MouseEvent {
    if (!el.isConnected) document.body.appendChild(el)
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    el.dispatchEvent(event)
    return event
  }

  // Regression anchor: the queue window had no suppressor, so a right-click on a row
  // showed WebKit's "Reload" / "Inspect Element" (2026-10-14 QA).
  it("cancels WebKit's menu on a plain element", () => {
    uninstall = installContextMenuOwnership()
    expect(rightClick(document.createElement('div')).defaultPrevented).toBe(true)
  })

  it("leaves a text field's editing menu alone", () => {
    uninstall = installContextMenuOwnership()
    expect(rightClick(document.createElement('textarea')).defaultPrevented).toBe(false)
  })

  it("stops a text field's right-click before a row's own menu handler hears it", () => {
    uninstall = installContextMenuOwnership()
    const row = document.createElement('div')
    const input = document.createElement('input')
    row.appendChild(input)
    document.body.appendChild(row)
    let rowHeard = false
    row.addEventListener('contextmenu', () => {
      rowHeard = true
    })
    rightClick(input)
    expect(rowHeard).toBe(false)
  })

  it("still lets a row's own handler open Cmdr's menu", () => {
    uninstall = installContextMenuOwnership()
    const row = document.createElement('div')
    let rowHeard = false
    row.addEventListener('contextmenu', () => {
      rowHeard = true
    })
    rightClick(row)
    expect(rowHeard).toBe(true)
  })

  it('stops suppressing once uninstalled', () => {
    installContextMenuOwnership()()
    expect(rightClick(document.createElement('div')).defaultPrevented).toBe(false)
  })
})

/** A right-click whose target is `el`, as the document-level listener sees it. */
function rightClickOn(el: Element): MouseEvent {
  document.body.appendChild(el)
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
  el.dispatchEvent(event)
  el.remove()
  return event
}

describe('resolveGlobalContextMenuAction', () => {
  it('hands an <input> to the native text menu', () => {
    expect(resolveGlobalContextMenuAction(rightClickOn(document.createElement('input')))).toBe('native-text-menu')
  })

  it('hands a <textarea> to the native text menu', () => {
    expect(resolveGlobalContextMenuAction(rightClickOn(document.createElement('textarea')))).toBe('native-text-menu')
  })

  it('hands a contenteditable host to the native text menu', () => {
    const host = document.createElement('div')
    host.contentEditable = 'true'
    expect(resolveGlobalContextMenuAction(rightClickOn(host))).toBe('native-text-menu')
  })

  it('hands an element INSIDE a contenteditable host to the native text menu', () => {
    const host = document.createElement('div')
    host.contentEditable = 'true'
    const inner = document.createElement('span')
    host.appendChild(inner)
    document.body.appendChild(host)
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    inner.dispatchEvent(event)
    host.remove()
    expect(resolveGlobalContextMenuAction(event)).toBe('native-text-menu')
  })

  it('hands the rename editor inside a file row to the native text menu', () => {
    const row = document.createElement('div')
    row.className = 'file-row'
    const input = document.createElement('input')
    row.appendChild(input)
    document.body.appendChild(row)
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    input.dispatchEvent(event)
    row.remove()
    expect(resolveGlobalContextMenuAction(event)).toBe('native-text-menu')
  })

  it("suppresses on a file row, so only Cmdr's own menu opens", () => {
    const row = document.createElement('div')
    row.className = 'file-row'
    expect(resolveGlobalContextMenuAction(rightClickOn(row))).toBe('suppress')
  })

  it('suppresses on a button', () => {
    expect(resolveGlobalContextMenuAction(rightClickOn(document.createElement('button')))).toBe('suppress')
  })

  it('suppresses a targetless event', () => {
    expect(resolveGlobalContextMenuAction(new MouseEvent('contextmenu'))).toBe('suppress')
  })
})

/** Where the page draws its menus (Windows), a text field's menu is Cmdr's in-page one, and a control without text gets none. */
describe('resolveGlobalContextMenuAction where menus are in-page', () => {
  const resolveInPage = (el: Element) => resolveGlobalContextMenuAction(rightClickOn(el), 'inPage')

  function input(type: string, setup: (el: HTMLInputElement) => void = () => {}): HTMLInputElement {
    const el = document.createElement('input')
    el.type = type
    setup(el)
    return el
  }

  it.each(['text', 'search', 'url', 'tel', 'email', 'password', 'number'])(
    'opens the in-page edit menu on a %s input',
    (type) => {
      expect(resolveInPage(input(type))).toBe('in-page-text-menu')
    },
  )

  it('opens it on a <textarea>, a read-only field, and inside a contenteditable host', () => {
    expect(resolveInPage(document.createElement('textarea'))).toBe('in-page-text-menu')
    expect(
      resolveInPage(
        input('text', (el) => {
          el.readOnly = true
        }),
      ),
    ).toBe('in-page-text-menu')
    const host = document.createElement('div')
    host.contentEditable = 'true'
    const inner = document.createElement('span')
    host.appendChild(inner)
    document.body.appendChild(host)
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    inner.dispatchEvent(event)
    host.remove()
    expect(resolveGlobalContextMenuAction(event, 'inPage')).toBe('in-page-text-menu')
  })

  it('gives a disabled field, a checkbox, a slider, and a <select> no menu at all', () => {
    const disabled = input('text', (el) => {
      el.disabled = true
    })
    expect(resolveInPage(disabled)).toBe('no-menu-in-control')
    expect(resolveInPage(input('checkbox'))).toBe('no-menu-in-control')
    expect(resolveInPage(input('range'))).toBe('no-menu-in-control')
    expect(resolveInPage(document.createElement('select'))).toBe('no-menu-in-control')
  })

  it("suppresses everywhere else, so a row's own handler opens Cmdr's menu", () => {
    expect(resolveInPage(document.createElement('div'))).toBe('suppress')
  })
})
