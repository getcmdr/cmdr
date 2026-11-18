// The menu spike's page: two lists and a text field (so keys land where Cmdr's would), the mode B
// title strip, the mode C bar, and the event log. Throwaway; see ../README.md.
'use strict'

const { invoke } = window.__TAURI__.core
const { listen } = window.__TAURI__.event

const state = { mode: 'native', bar: [], theme: 'system', windows: false }
const logEl = document.getElementById('log')
const barEl = document.getElementById('bar')
const dropdownsEl = document.getElementById('dropdowns')
let lastFocus = null

// ---- Log ----------------------------------------------------------------------------------------

const layerClass = {
  'page keydown': 'page',
  'page keyup': 'page',
  page: 'page',
  'menu event': 'menu',
  'in-page menu': 'menu',
  host: 'host',
  bridge: 'host',
  'webview2 accel': 'wv',
  'native popup': 'menu',
}

function addLog(layer, text) {
  const li = document.createElement('li')
  li.className = (layerClass[layer] || '') + (layer === 'selftest' && text.startsWith('---') ? ' sep' : '')
  const span = document.createElement('span')
  span.textContent = layer
  li.append(span, ' ', text)
  logEl.prepend(li)
  while (logEl.childElementCount > 400) logEl.lastElementChild.remove()
}

function pageLog(layer, text) {
  addLog(layer, text)
  invoke('page_log', { text: `${layer}: ${text}` }).catch(() => {})
}

function describe(e) {
  const mods = []
  if (e.ctrlKey) mods.push('Ctrl')
  if (e.altKey) mods.push('Alt')
  if (e.shiftKey) mods.push('Shift')
  if (e.metaKey) mods.push('Meta')
  if (e.getModifierState && e.getModifierState('AltGraph')) mods.push('AltGraph')
  const where = document.activeElement ? document.activeElement.id || document.activeElement.tagName : '-'
  return `key="${e.key}" code=${e.code} [${mods.join('+')}]${e.repeat ? ' repeat' : ''} focus=${where}`
}

// ---- Lists ----------------------------------------------------------------------------------------

const names = ['..', 'Documents', 'Photos', 'report.pdf', 'notes.txt', 'budget.xlsx', 'song.mp3', 'archive.zip']
function fillList(el) {
  for (const n of names) {
    const li = document.createElement('li')
    li.textContent = n
    el.append(li)
  }
  el.firstElementChild.classList.add('cursor')
  el.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    const cur = el.querySelector('.cursor')
    const next = e.key === 'ArrowDown' ? cur.nextElementSibling : cur.previousElementSibling
    if (next) {
      cur.classList.remove('cursor')
      next.classList.add('cursor')
    }
    e.preventDefault()
  })
  el.addEventListener('mousedown', (e) => {
    if (e.target.tagName !== 'LI') return
    el.querySelector('.cursor').classList.remove('cursor')
    e.target.classList.add('cursor')
  })
}
const left = document.getElementById('left')
const right = document.getElementById('right')
fillList(left)
fillList(right)

document.addEventListener('contextmenu', (e) => {
  e.preventDefault()
  pageLog('page', `contextmenu at ${e.clientX},${e.clientY}`)
  invoke('show_context_menu', { x: e.clientX, y: e.clientY })
})

// ---- Mnemonic helpers ----------------------------------------------------------------------------

function mnemonicChar(item) {
  return item.mnemonic == null ? null : [...item.label][item.mnemonic].toLowerCase()
}

function labelNode(item) {
  const frag = document.createDocumentFragment()
  const chars = [...item.label]
  chars.forEach((c, i) => {
    if (i === item.mnemonic) {
      const u = document.createElement('u')
      u.textContent = c
      frag.append(u)
    } else frag.append(c)
  })
  return frag
}

// ---- Bar strip (modes B and C) -------------------------------------------------------------------

let openIndex = null

function renderBar() {
  barEl.replaceChildren()
  state.bar.forEach((menu, i) => {
    const b = document.createElement('button')
    b.type = 'button'
    b.setAttribute('role', 'menuitem')
    b.setAttribute('aria-haspopup', 'true')
    b.tabIndex = -1
    b.append(labelNode(menu))
    b.addEventListener('mousedown', (e) => {
      e.preventDefault()
      rememberFocus()
      openMenu(i)
    })
    barEl.append(b)
  })
}

function titles() {
  return [...barEl.querySelectorAll('button')]
}

function rememberFocus() {
  const a = document.activeElement
  if (a && !barEl.contains(a) && !dropdownsEl.contains(a)) lastFocus = a
}

function restoreFocus() {
  document.body.classList.remove('keys-visible')
  ;(lastFocus || left).focus()
}

function focusTitle(i) {
  rememberFocus()
  const t = titles()
  if (!t.length) return
  document.body.classList.add('keys-visible')
  t[(i + t.length) % t.length].focus()
}

function openMenu(i) {
  const t = titles()
  i = (i + t.length) % t.length
  if (state.mode === 'hybrid') {
    const anchors = t.map((el) => {
      const r = el.getBoundingClientRect()
      return [r.left, r.bottom]
    })
    pageLog('page', `asks for native popup ${i}`)
    invoke('open_bar_popup', { index: i, anchors })
  } else if (state.mode === 'inpage') {
    openDropdown(i)
  }
}

listen('hybrid-open', (e) => {
  titles().forEach((t, i) => t.classList.toggle('open', i === e.payload))
})
listen('hybrid-closed', () => {
  titles().forEach((t) => t.classList.remove('open'))
  restoreFocus()
})

// ---- Mode C dropdowns ----------------------------------------------------------------------------

let stack = [] // [{ rows, el, active }]

function closeDropdowns(restore) {
  stack = []
  dropdownsEl.replaceChildren()
  openIndex = null
  titles().forEach((t) => t.classList.remove('open'))
  if (restore) restoreFocus()
}

function openDropdown(i) {
  closeDropdowns(false)
  openIndex = i
  const t = titles()[i]
  t.classList.add('open')
  t.focus()
  const r = t.getBoundingClientRect()
  pushPanel(state.bar[i].rows, r.left, r.bottom)
}

function pushPanel(rows, x, y) {
  const el = document.createElement('div')
  el.className = 'dropdown'
  el.setAttribute('role', 'menu')
  el.style.left = `${x}px`
  el.style.top = `${y}px`
  const level = { rows, el, active: -1, items: [] }
  rows.forEach((row) => {
    if (row.kind === 'separator') {
      el.append(document.createElement('hr'))
      level.items.push(null)
      return
    }
    const div = document.createElement('div')
    div.className = 'row' + (row.kind === 'item' && !row.enabled ? ' disabled' : '')
    div.setAttribute('role', row.checked != null ? 'menuitemcheckbox' : 'menuitem')
    const label = document.createElement('span')
    if (row.checked) label.append('✓ ')
    label.append(labelNode(row))
    const key = document.createElement('span')
    key.className = 'key'
    key.textContent = row.kind === 'submenu' ? '›' : row.accelerator || row.shown || ''
    div.append(label, key)
    div.addEventListener('mouseenter', () => setActive(level, level.items.indexOf(div)))
    div.addEventListener('mousedown', (e) => {
      e.preventDefault()
      activate(level, level.items.indexOf(div))
    })
    el.append(div)
    level.items.push(div)
  })
  dropdownsEl.append(el)
  stack.push(level)
  return level
}

function setActive(level, idx) {
  const depth = stack.indexOf(level)
  while (stack.length > depth + 1) stack.pop().el.remove()
  level.items.forEach((d, i) => d && d.classList.toggle('active', i === idx))
  level.active = idx
}

function walk(level, step) {
  const n = level.items.length
  let i = level.active
  for (let k = 0; k < n; k++) {
    i = (i + step + n) % n
    if (level.items[i]) return setActive(level, i)
  }
}

function activate(level, idx) {
  const row = level.rows[idx]
  if (!row || row.kind === 'separator') return
  if (row.kind === 'submenu') {
    setActive(level, idx)
    const r = level.items[idx].getBoundingClientRect()
    const sub = pushPanel(row.rows, r.right, r.top - 4)
    walk(sub, 1)
    return
  }
  if (!row.enabled) return
  invoke('page_click', { id: row.id })
  closeDropdowns(true)
}

function dropdownKey(e) {
  const level = stack[stack.length - 1]
  const k = e.key
  if (k === 'ArrowDown') walk(level, 1)
  else if (k === 'ArrowUp') walk(level, -1)
  else if (k === 'ArrowRight') {
    const row = level.rows[level.active]
    if (row && row.kind === 'submenu') activate(level, level.active)
    else openDropdown(openIndex + 1)
  } else if (k === 'ArrowLeft') {
    if (stack.length > 1) stack.pop().el.remove()
    else openDropdown(openIndex - 1)
  } else if (k === 'Enter' || k === ' ') activate(level, level.active)
  else if (k === 'Escape') {
    if (stack.length > 1) stack.pop().el.remove()
    else {
      closeDropdowns(false)
      focusTitle(titles().indexOf(document.activeElement))
    }
  } else if (k.length === 1 && !e.ctrlKey) {
    const idx = level.rows.findIndex((r) => r.kind !== 'separator' && mnemonicChar(r) === k.toLowerCase())
    if (idx >= 0) activate(level, idx)
  } else return false
  return true
}

document.addEventListener('mousedown', (e) => {
  if (stack.length && !dropdownsEl.contains(e.target) && !barEl.contains(e.target)) closeDropdowns(false)
})

// ---- Keys ----------------------------------------------------------------------------------------

let altAlone = false
// F10 isn't here: it belongs to the menu bar, and a `preventDefault` on it keeps it from Windows.
const ownedByPage = (e) =>
  /^F([1-9]|11|12)$/.test(e.key) ||
  (e.ctrlKey && e.key.toLowerCase() === 'r') ||
  (e.altKey && e.key.startsWith('Arrow'))

window.addEventListener(
  'keydown',
  (e) => {
    pageLog('page keydown', describe(e))
    altAlone = e.key === 'Alt' && !e.repeat ? true : e.key === 'Alt' ? altAlone : false
    const pageDrawsBar = state.mode === 'hybrid' || state.mode === 'inpage'

    if (pageDrawsBar) {
      // F10 or Alt+letter reach the strip.
      if (e.key === 'F10' && !e.shiftKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault()
        if (barEl.contains(document.activeElement) || stack.length) {
          closeDropdowns(true)
        } else focusTitle(0)
        return
      }
      if (e.altKey && !e.ctrlKey && !(e.getModifierState && e.getModifierState('AltGraph')) && e.key.length === 1) {
        const i = state.bar.findIndex((m) => mnemonicChar(m) === e.key.toLowerCase())
        if (i >= 0) {
          e.preventDefault()
          rememberFocus()
          openMenu(i)
          return
        }
      }
      if (stack.length && state.mode === 'inpage') {
        if (dropdownKey(e)) e.preventDefault()
        return
      }
      if (barEl.contains(document.activeElement)) {
        const i = titles().indexOf(document.activeElement)
        if (e.key === 'ArrowRight') focusTitle(i + 1)
        else if (e.key === 'ArrowLeft') focusTitle(i - 1)
        else if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') openMenu(i)
        else if (e.key === 'Escape') restoreFocus()
        else if (e.key.length === 1 && !e.ctrlKey) {
          const j = state.bar.findIndex((m) => mnemonicChar(m) === e.key.toLowerCase())
          if (j >= 0) openMenu(j)
          else return
        } else return
        e.preventDefault()
        return
      }
    }
    if (document.getElementById('pageOwns').checked && ownedByPage(e)) {
      e.preventDefault()
      pageLog('page', `handled ${e.key} (preventDefault, as a Cmdr shortcut would)`)
    }
    if (e.shiftKey && e.key === 'F10') {
      const cur = document.activeElement.querySelector && document.activeElement.querySelector('.cursor')
      if (cur) {
        const r = cur.getBoundingClientRect()
        invoke('show_context_menu', { x: r.left + 20, y: r.bottom })
      }
    }
  },
  true,
)

window.addEventListener(
  'keyup',
  (e) => {
    if (e.key === 'Alt' || e.key === 'F10' || e.key === 'AltGraph') pageLog('page keyup', describe(e))
    if (e.key === 'Alt' && altAlone && (state.mode === 'hybrid' || state.mode === 'inpage')) {
      e.preventDefault()
      if (barEl.contains(document.activeElement) || stack.length) {
        closeDropdowns(true)
      } else focusTitle(0)
    }
    altAlone = false
  },
  true,
)

window.addEventListener('blur', () => pageLog('page', 'window blur (the page lost focus)'))
window.addEventListener('focus', () => pageLog('page', 'window focus'))

// ---- Controls ------------------------------------------------------------------------------------

function applyTheme() {
  const dark =
    state.theme === 'dark' || (state.theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.dataset.theme = dark ? 'dark' : 'light'
}
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme)

function showMode() {
  document.querySelectorAll('input[name=mode]').forEach((r) => (r.checked = r.value === state.mode))
  document.body.classList.toggle('has-bar', state.mode !== 'native')
  barEl.setAttribute('role', state.mode === 'native' ? 'none' : 'menubar')
  closeDropdowns(false)
}

async function setMode(mode, bridge) {
  state.mode = mode
  const guard = mode !== 'native'
  document.getElementById('guard').checked = guard
  if (bridge != null) document.getElementById('bridge').checked = bridge
  showMode()
  await invoke('set_mode', { mode })
  await sendFlags()
}

function sendFlags() {
  return invoke('set_flags', {
    bridge: document.getElementById('bridge').checked,
    guard: document.getElementById('guard').checked,
    browserKeys: document.getElementById('browserKeys').checked,
  })
}

document.querySelectorAll('input[name=mode]').forEach((r) => r.addEventListener('change', () => setMode(r.value)))
for (const id of ['bridge', 'guard', 'browserKeys']) document.getElementById(id).addEventListener('change', sendFlags)
document.getElementById('theme').addEventListener('change', (e) => {
  state.theme = e.target.value
  applyTheme()
  invoke('set_theme', { theme: state.theme })
})
document.getElementById('clear').addEventListener('click', () => logEl.replaceChildren())

listen('spike-log', (e) => addLog(e.payload.layer, e.payload.text))
listen('selftest-mode', (e) => setMode(e.payload[0], e.payload[1]))
listen('selftest-focus', (e) => (e.payload === 'text' ? document.getElementById('filter') : left).focus())

async function boot() {
  const s = await invoke('get_state')
  state.mode = s.mode
  state.bar = s.bar
  state.theme = s.theme
  state.windows = s.windows
  document.getElementById('bridge').checked = s.bridge
  document.getElementById('guard').checked = s.guard
  document.getElementById('browserKeys').checked = s.browser_keys
  document.getElementById('theme').value = s.theme
  applyTheme()
  renderBar()
  showMode()
  left.focus()
  pageLog('page', `loaded (mode ${s.mode}${s.windows ? '' : ', not on Windows: host hooks are off'})`)
}
boot()
